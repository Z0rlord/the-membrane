//! Local UI verification fixture. Synthetic rows; no real tool calls or credentials.
use membrane_core::{BusPublisher, BusPublisherConfig, SessionChainState};
use membrane_gate::{
    audit::{bind, AuditLog},
    ChannelRegistry, Gate, GateServerState, GitHubConnector, GitHubConnectorConfig, LlmProxy,
};
use std::sync::Arc;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let registry = ChannelRegistry {
        permitted_channels: vec!["local-llm".into()],
        forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
        model_allowlist: vec!["local-model".into()],
        delta_t_secs: 300,
        model_api_url: None,
        github_repo_allowlist: vec!["acme/pilot".into()],
    };
    let keys = nostr::Keys::generate();
    let gate = Arc::new(Gate::new(
        registry,
        BusPublisher::new(BusPublisherConfig {
            relay_url: "memory://audit-preview".into(),
            keys,
        }),
    ));
    let audit = Arc::new(AuditLog::default());
    for (outcome, rule, scope, action) in [
        (
            "allow",
            "all_authorization_checks_passed",
            Some("preview-scope".into()),
            "chat",
        ),
        ("deny", "tool_allowlist", None, "tool"),
        ("deny", "iac_signature", None, "chat"),
    ] {
        audit.record(outcome, rule, gate.publisher_pubkey_hex(), scope, action);
    }
    let state = GateServerState {
        gate,
        proxy: Arc::new(LlmProxy::new(None)),
        default_iac: None,
        session_chain: Arc::new(tokio::sync::Mutex::new(SessionChainState::genesis())),
        github: Arc::new(GitHubConnector::new(GitHubConnectorConfig {
            repo_allowlist: vec![],
            api_base: "http://127.0.0.1:1".into(),
            token: None,
        })),
        audit,
    };
    let task = bind(state, "127.0.0.1:8788").await?;
    println!("Synthetic audit preview on 127.0.0.1:8788; not a running production gate");
    tokio::signal::ctrl_c().await?;
    task.abort();
    Ok(())
}
