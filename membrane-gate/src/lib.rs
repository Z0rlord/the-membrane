use membrane_core::event::{EventType, MembraneEvent, MembranePayload, RouterSessionPayload};
use membrane_core::iac::IntentAuthorizationCredential;
use membrane_core::merkle::{Domain, MerkleTree};
use membrane_core::nostr_bus::BusPublisher;
use membrane_core::rollup::cp_hash_hex;
use membrane_core::{alert_degraded_payload, SessionChainState};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GateError {
    #[error("no valid IAC: {0}")]
    NoValidIac(String),
    #[error("invalid IAC signature: {0}")]
    InvalidIacSignature(String),
    #[error("channel not permitted: {0}")]
    ChannelDenied(String),
    #[error("model not in allowlist: {0}")]
    ModelDenied(String),
    #[error("tool not in allowlist: {0}")]
    ToolDenied(String),
    #[error("export forbidden: {0}")]
    ExportForbidden(String),
    #[error("context merkle root exceeds IAC bound")]
    ContextBoundExceeded,
    #[error("session degraded ({0}): {1}")]
    SessionDegraded(String, String),
    #[error("router CP stale: last CP {0}s ago exceeds Δt={1}s")]
    SessionStale(i64, u64),
    #[error("registry error: {0}")]
    Registry(String),
    #[error("bus error: {0}")]
    Bus(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRegistry {
    pub permitted_channels: Vec<String>,
    pub forbidden_exports: Vec<String>,
    pub model_allowlist: Vec<String>,
    #[serde(default = "default_delta_t_secs")]
    pub delta_t_secs: u64,
    #[serde(default, alias = "llama_cpp_url", alias = "ollama_url")]
    pub model_api_url: Option<String>,
}

fn default_delta_t_secs() -> u64 {
    300
}

impl ChannelRegistry {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())?;
        Ok(serde_yaml::from_str(&text)?)
    }
}

pub struct Gate {
    registry: ChannelRegistry,
    publisher: BusPublisher,
    iac_signer_pubkey: String,
}

#[derive(Debug, Clone)]
pub struct RouterSessionRequest {
    pub model_id: String,
    pub context_chunks: Vec<Vec<u8>>,
    pub session_nonce: u64,
    pub parent_cp_hash: String,
}

#[derive(Debug, Clone)]
pub struct RouterSessionOutcome {
    pub event: MembraneEvent,
    pub context_merkle_root: String,
    pub cp_hash: String,
    pub bus_event_id: Option<String>,
}

impl Gate {
    pub fn new(registry: ChannelRegistry, publisher: BusPublisher) -> Self {
        let iac_signer_pubkey = publisher.keys().public_key().to_hex();
        Self {
            registry,
            publisher,
            iac_signer_pubkey,
        }
    }

    pub fn registry(&self) -> &ChannelRegistry {
        &self.registry
    }

    pub fn validate_iac(
        &self,
        iac: Option<&IntentAuthorizationCredential>,
        now: i64,
    ) -> Result<(), GateError> {
        let iac = iac.ok_or_else(|| GateError::NoValidIac("missing IAC".into()))?;
        iac.verify_signature(&self.iac_signer_pubkey)
            .map_err(|e| GateError::InvalidIacSignature(e.to_string()))?;
        if !iac.is_valid_at(now) {
            return Err(GateError::NoValidIac("IAC expired".into()));
        }
        if !iac.permits_channel("local-llm") {
            return Err(GateError::ChannelDenied(
                "IAC does not permit local-llm".into(),
            ));
        }
        for channel in &self.registry.permitted_channels {
            if !iac.permits_channel(channel) {
                return Err(GateError::ChannelDenied(channel.clone()));
            }
        }
        Ok(())
    }

    pub fn check_session_liveness(
        &self,
        chain: &SessionChainState,
        scope_id: &str,
        now: i64,
    ) -> Result<(), GateError> {
        if chain.is_scope_degraded(scope_id) {
            let reason = chain
                .degraded_reason
                .clone()
                .unwrap_or_else(|| "degraded".into());
            return Err(GateError::SessionDegraded(scope_id.to_string(), reason));
        }
        if chain.is_router_stale(now, self.registry.delta_t_secs) {
            let age = chain
                .last_router_cp_age_secs(now)
                .unwrap_or(self.registry.delta_t_secs as i64);
            return Err(GateError::SessionStale(age, self.registry.delta_t_secs));
        }
        Ok(())
    }

    pub async fn publish_alert_degraded(
        &self,
        scope_id: &str,
        reason: &str,
        now: i64,
        last_cp_hash: &str,
        last_cp_age_secs: Option<i64>,
        prev_event_id: Option<&str>,
    ) -> Result<String, GateError> {
        let payload = alert_degraded_payload(
            reason,
            scope_id,
            last_cp_age_secs,
            self.registry.delta_t_secs,
        );
        let mut event =
            MembraneEvent::new(EventType::AlertDegraded, "", last_cp_hash, now, payload);
        let id = self
            .publisher
            .publish(&mut event, prev_event_id)
            .await
            .map_err(GateError::Bus)?
            .to_hex();
        Ok(id)
    }

    pub async fn publish_action_blocked(
        &self,
        scope_id: Option<&str>,
        model_id: Option<&str>,
        iac_hash: Option<&str>,
        reason: &str,
        now: i64,
        last_cp_hash: &str,
        prev_event_id: Option<&str>,
    ) -> Result<String, GateError> {
        let payload = MembranePayload::Generic(serde_json::json!({
            "scope_id": scope_id,
            "model_allowlist": model_id.into_iter().collect::<Vec<_>>(),
            "tool_allowlist": Vec::<String>::new(),
            "iac_hash": iac_hash,
            "reason": reason,
        }));
        let mut event =
            MembraneEvent::new(EventType::ActionBlocked, "", last_cp_hash, now, payload);
        let id = self
            .publisher
            .publish(&mut event, prev_event_id)
            .await
            .map_err(GateError::Bus)?
            .to_hex();
        Ok(id)
    }

    pub async fn open_router_session(
        &self,
        iac: Option<&IntentAuthorizationCredential>,
        req: RouterSessionRequest,
        now: i64,
        prev_event_id: Option<&str>,
    ) -> Result<RouterSessionOutcome, GateError> {
        self.validate_iac(iac, now)?;
        let iac = iac.expect("validated");

        if !iac.model_allowed(&req.model_id) {
            return Err(GateError::ModelDenied(req.model_id.clone()));
        }
        if !self.registry.model_allowlist.contains(&req.model_id) {
            return Err(GateError::ModelDenied(req.model_id.clone()));
        }
        for export in &self.registry.forbidden_exports {
            if !iac.forbidden_exports.iter().any(|f| f == export) {
                return Err(GateError::ExportForbidden(format!(
                    "IAC missing forbidden export: {export}"
                )));
            }
        }

        let context_merkle_root = context_root_hex(&req.context_chunks)?;
        if context_merkle_root > iac.context_merkle_bound {
            return Err(GateError::ContextBoundExceeded);
        }

        let iac_hash = iac
            .hash_hex()
            .map_err(|e| GateError::Bus(anyhow::anyhow!(e)))?;

        let mut event = MembraneEvent::new(
            EventType::CpRouter,
            "",
            req.parent_cp_hash.clone(),
            now,
            MembranePayload::Router(RouterSessionPayload {
                model_id: req.model_id,
                context_merkle_root: context_merkle_root.clone(),
                session_nonce: req.session_nonce,
                parent_cp_hash: req.parent_cp_hash,
                iac_hash,
            }),
        );

        let bus_event_id = self
            .publisher
            .publish(&mut event, prev_event_id)
            .await
            .map_err(GateError::Bus)?
            .to_hex();

        let cp_hash = cp_hash_hex(&event).map_err(|e| GateError::Bus(e.into()))?;

        Ok(RouterSessionOutcome {
            event,
            context_merkle_root,
            cp_hash,
            bus_event_id: Some(bus_event_id),
        })
    }

    /// Fail-closed tool check against the live IAC (product agent scopes).
    pub fn authorize_tool(
        &self,
        iac: &IntentAuthorizationCredential,
        tool_id: &str,
        now: i64,
    ) -> Result<(), GateError> {
        self.validate_iac(Some(iac), now)?;
        if !iac.tool_allowed(tool_id) {
            return Err(GateError::ToolDenied(tool_id.to_string()));
        }
        Ok(())
    }

    pub fn publisher_pubkey_hex(&self) -> String {
        self.iac_signer_pubkey.clone()
    }

    pub fn publisher(&self) -> &BusPublisher {
        &self.publisher
    }
}

pub fn context_root_hex(chunks: &[Vec<u8>]) -> Result<String, GateError> {
    let tree = MerkleTree::from_domain_leaves(Domain::FeatureChunk, chunks);
    tree.root_hex()
        .ok_or_else(|| GateError::Registry("empty context".into()))
}

pub mod demo;
pub mod proxy;
pub mod server;
pub mod watchdog;

pub use demo::{
    demo_registry, run_demo_dashboard, verify_evidence_pack, DemoRuntime, DemoServerState,
    EvidencePack, DEMO_ALLOWED_TOOLS, DEMO_BLOCKED_TOOL, DEMO_MODEL, DEMO_SWAP_MODEL,
    DEMO_TTL_SECS,
};
pub use proxy::{ChatMessage, ChatRequest, ChatResponse, LlmProxy};
pub use server::{run_gate_server, GateServerState, SessionReceipt};

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_core::{
        alert_degraded_payload, BusPublisher, BusPublisherConfig, ALERT_REASON_SUBJECT_SEVER,
    };
    use nostr::Keys;

    fn test_gate(delta_t: u64) -> Gate {
        let keys = Keys::generate();
        let registry = ChannelRegistry {
            permitted_channels: vec!["local-llm".into()],
            forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
            model_allowlist: vec!["demo".into()],
            delta_t_secs: delta_t,
            model_api_url: None,
        };
        let publisher = BusPublisher::new(BusPublisherConfig {
            relay_url: "ws://localhost:7777".into(),
            keys,
        });
        Gate::new(registry, publisher)
    }

    #[test]
    fn rejects_degraded_scope() {
        let gate = test_gate(300);
        let mut chain = SessionChainState::genesis();
        chain.mark_degraded("scope-a", ALERT_REASON_SUBJECT_SEVER, 100);
        let err = gate
            .check_session_liveness(&chain, "scope-a", 200)
            .unwrap_err();
        assert!(matches!(err, GateError::SessionDegraded(_, _)));
    }

    #[test]
    fn rejects_stale_router_cp() {
        let gate = test_gate(300);
        let mut chain = SessionChainState::genesis();
        chain.last_router_cp_at = Some(1_000);
        let err = gate
            .check_session_liveness(&chain, "scope-a", 1_400)
            .unwrap_err();
        assert!(matches!(err, GateError::SessionStale(400, 300)));
    }

    #[test]
    fn fresh_router_cp_passes() {
        let gate = test_gate(300);
        let mut chain = SessionChainState::genesis();
        chain.last_router_cp_at = Some(1_000);
        gate.check_session_liveness(&chain, "scope-a", 1_200)
            .unwrap();
    }

    #[test]
    fn alert_payload_includes_delta_t() {
        let payload = alert_degraded_payload("subject_sever", "scope-x", Some(10), 300);
        let MembranePayload::Generic(v) = payload else {
            panic!("expected generic");
        };
        assert_eq!(v["reason"], "subject_sever");
        assert_eq!(v["delta_t_secs"], 300);
    }

    #[test]
    fn rejects_unknown_tool() {
        let keys = Keys::generate();
        let registry = ChannelRegistry {
            permitted_channels: vec!["local-llm".into()],
            forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
            model_allowlist: vec!["demo".into()],
            delta_t_secs: 300,
            model_api_url: None,
        };
        let publisher = BusPublisher::new(BusPublisherConfig {
            relay_url: "memory://test".into(),
            keys: keys.clone(),
        });
        let gate = Gate::new(registry, publisher);
        let mut iac = IntentAuthorizationCredential::new_session_with_tools(
            "scope",
            "demo",
            "0".repeat(64),
            4_102_444_800,
            vec!["local-llm".into()],
            vec!["cloud-telemetry".into(), "training-retention".into()],
            vec!["jira.comment".into()],
        );
        iac.sign(&keys).unwrap();
        let err = gate
            .authorize_tool(&iac, "github.merge", 1_000)
            .unwrap_err();
        assert!(matches!(err, GateError::ToolDenied(_)));
        gate.authorize_tool(&iac, "jira.comment", 1_000).unwrap();
    }
}
