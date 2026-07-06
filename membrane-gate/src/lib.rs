use membrane_core::event::{EventType, MembraneEvent, MembranePayload, RouterSessionPayload};
use membrane_core::iac::IntentAuthorizationCredential;
use membrane_core::merkle::{Domain, MerkleTree};
use membrane_core::nostr_bus::BusPublisher;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GateError {
    #[error("no valid IAC: {0}")]
    NoValidIac(String),
    #[error("channel not permitted: {0}")]
    ChannelDenied(String),
    #[error("model not in allowlist: {0}")]
    ModelDenied(String),
    #[error("export forbidden: {0}")]
    ExportForbidden(String),
    #[error("context merkle root exceeds IAC bound")]
    ContextBoundExceeded,
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
    #[serde(default, alias = "ollama_url")]
    pub llama_cpp_url: Option<String>,
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
    pub bus_event_id: Option<String>,
}

impl Gate {
    pub fn new(registry: ChannelRegistry, publisher: BusPublisher) -> Self {
        Self {
            registry,
            publisher,
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

        Ok(RouterSessionOutcome {
            event,
            context_merkle_root,
            bus_event_id: Some(bus_event_id),
        })
    }
}

pub fn context_root_hex(chunks: &[Vec<u8>]) -> Result<String, GateError> {
    let tree = MerkleTree::from_domain_leaves(Domain::FeatureChunk, chunks);
    tree.root_hex()
        .ok_or_else(|| GateError::Registry("empty context".into()))
}

pub mod proxy;
pub mod server;

pub use proxy::LlmProxy;
pub use server::{GateServerState, run_gate_server};
