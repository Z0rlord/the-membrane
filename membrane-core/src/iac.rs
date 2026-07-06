use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAuthorizationCredential {
    pub version: String,
    pub scope_id: String,
    pub permitted_channels: Vec<String>,
    pub model_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stimulation_policy: Option<String>,
    pub context_merkle_bound: String,
    pub forbidden_exports: Vec<String>,
    pub valid_until: i64,
    pub parent_cp_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignableIac {
    pub version: String,
    pub scope_id: String,
    pub permitted_channels: Vec<String>,
    pub model_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stimulation_policy: Option<String>,
    pub context_merkle_bound: String,
    pub forbidden_exports: Vec<String>,
    pub valid_until: i64,
    pub parent_cp_hash: String,
}

impl IntentAuthorizationCredential {
    pub const SCHEMA_VERSION: &str = "0.9.14";

    pub fn signable_view(&self) -> SignableIac {
        SignableIac {
            version: self.version.clone(),
            scope_id: self.scope_id.clone(),
            permitted_channels: self.permitted_channels.clone(),
            model_allowlist: self.model_allowlist.clone(),
            decoder_version: self.decoder_version.clone(),
            stimulation_policy: self.stimulation_policy.clone(),
            context_merkle_bound: self.context_merkle_bound.clone(),
            forbidden_exports: self.forbidden_exports.clone(),
            valid_until: self.valid_until,
            parent_cp_hash: self.parent_cp_hash.clone(),
        }
    }

    pub fn hash_hex(&self) -> Result<String, crate::canonical::CanonicalError> {
        let bytes = crate::canonical::canonical_json_bytes(&self.signable_view())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    pub fn is_valid_at(&self, now: i64) -> bool {
        self.valid_until >= now
    }

    pub fn permits_channel(&self, channel: &str) -> bool {
        self.permitted_channels.iter().any(|c| c == channel)
    }

    pub fn model_allowed(&self, model_id: &str) -> bool {
        self.model_allowlist.iter().any(|m| m == model_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupBundle {
    pub version: String,
    pub subject_pubkey: String,
    pub period_start: i64,
    pub period_end: i64,
    pub cp_chain_root: String,
    pub last_bus_root: String,
    pub last_cp_hash: String,
}

impl RollupBundle {
    pub const SCHEMA_VERSION: &str = "0.9.14";
}
