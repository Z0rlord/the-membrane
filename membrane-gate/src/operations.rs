//! Durable fail-closed write reservations. A transport error never permits retry.
use crate::{github::ToolInvokeRequest, GateError};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

pub struct OperationJournal {
    path: PathBuf,
}
impl OperationJournal {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn from_env() -> Self {
        Self::new(
            std::env::var_os("MEMBRANE_OPERATION_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".membrane/tool-operations")),
        )
    }
    pub fn reserve(&self, caller: &str, req: &ToolInvokeRequest) -> Result<PathBuf, GateError> {
        let id = req
            .operation_id
            .as_deref()
            .filter(|id| !id.trim().is_empty() && id.len() <= 128)
            .ok_or_else(|| {
                GateError::Connector("mutating tools require operation_id (1-128 bytes)".into())
            })?;
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&(caller, id)).map_err(|e| GateError::Registry(e.to_string()))?,
        ));
        fs::create_dir_all(&self.path).map_err(journal_error)?;
        let path = self.path.join(format!("{key}.jsonl"));
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|e| GateError::Connector(format!("operation unavailable or already reserved; reconcile upstream before retry: {e}")))?;
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(req).map_err(|e| GateError::Registry(e.to_string()))?,
        ));
        let intent = serde_json::json!({"state":"reserved", "request_sha256":digest});
        file.write_all(intent.to_string().as_bytes())
            .map_err(journal_error)?;
        file.sync_all().map_err(journal_error)?;
        // Persist the directory entry before dispatch, too.
        fs::File::open(&self.path)
            .and_then(|dir| dir.sync_all())
            .map_err(journal_error)?;
        Ok(path)
    }
    pub fn finish(&self, path: &std::path::Path, success: bool) -> Result<(), GateError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(journal_error)?;
        writeln!(
            file,
            "\n{}",
            serde_json::json!({"state": if success {"completed"} else {"uncertain"}})
        )
        .map_err(journal_error)?;
        file.sync_all().map_err(journal_error)
    }
}
fn journal_error(e: std::io::Error) -> GateError {
    GateError::Connector(format!(
        "operation journal failed; do not retry without reconciliation: {e}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_survives_restart_and_rejects_changed_body() {
        let path = std::env::temp_dir().join(nostr::Keys::generate().public_key().to_hex());
        let mut req: ToolInvokeRequest = serde_json::from_value(serde_json::json!({
            "tool":"github.comment", "model":"demo", "owner":"acme", "repo":"pilot", "issue_number":1,
            "body":"first", "operation_id":"stable-id"
        })).unwrap();
        let journal = OperationJournal::new(path.clone());
        let record = journal.reserve("caller", &req).unwrap();
        journal.finish(&record, false).unwrap();
        let restarted = OperationJournal::new(path.clone());
        assert!(restarted.reserve("caller", &req).is_err());
        req.body = Some("different".into());
        assert!(restarted.reserve("caller", &req).is_err());
        assert!(restarted.reserve("other-caller", &req).is_ok());
        fs::remove_dir_all(path).unwrap();
    }
}
