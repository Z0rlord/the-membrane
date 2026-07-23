use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
};
use membrane_core::iac::IntentAuthorizationCredential;
use membrane_core::rollup::cp_hash_hex;
use membrane_core::{SessionChainState, ALERT_REASON_DELTA_T_EXCEEDED};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::github::{is_github_tool, GitHubConnector, ToolInvokeRequest};
use crate::proxy::{ChatRequest, ChatResponse, LlmProxy};
use crate::watchdog::spawn_delta_t_watchdog;
use crate::{Gate, GateError, RouterSessionRequest};

/// Per-turn attestation receipt returned to sovereign clients (§4.2.2).
#[derive(Debug, Clone)]
pub struct SessionReceipt {
    pub scope_id: String,
    pub session_nonce: u64,
    pub cp_hash: String,
    pub context_merkle_root: String,
    pub parent_cp_hash: String,
    pub bus_event_id: Option<String>,
}

#[derive(Clone)]
pub struct GateServerState {
    pub gate: Arc<Gate>,
    pub proxy: Arc<LlmProxy>,
    pub default_iac: Option<IntentAuthorizationCredential>,
    pub session_chain: Arc<Mutex<SessionChainState>>,
    /// Real GitHub connector (operator installs). Demo dashboard does not use this.
    pub github: Arc<GitHubConnector>,
}

pub async fn run_gate_server(state: GateServerState, listen: &str) -> anyhow::Result<()> {
    spawn_delta_t_watchdog(state.gate.clone(), state.session_chain.clone());

    let app = axum::Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/tools/invoke", post(tools_invoke))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(listen = %listen, "membrane gate listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<GateServerState>) -> impl IntoResponse {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    let delta_t_secs = state.gate.registry().delta_t_secs;
    let last_cp_age_secs = chain.last_router_cp_age_secs(now);
    let router_stale = chain.is_router_stale(now, delta_t_secs);
    let active_scope = chain.active_scope_id.clone();
    let degraded_scope = chain.degraded_scope_id.clone();

    Json(json!({
        "status": if router_stale || degraded_scope.is_some() { "degraded" } else { "ok" },
        "gate": "membrane-phase-0",
        "delta_t_secs": delta_t_secs,
        "last_cp_age_secs": last_cp_age_secs,
        "router_stale": router_stale,
        "active_scope_id": active_scope,
        "degraded_scope_id": degraded_scope,
        "degraded_reason": chain.degraded_reason,
        "github_connector": {
            "repo_allowlist": state.gate.registry().github_repo_allowlist,
            "token_configured": state.github.config().has_token(),
        },
    }))
}

async fn chat_completions(
    State(state): State<GateServerState>,
    headers: HeaderMap,
    Json(mut req): Json<ChatRequest>,
) -> Response {
    match handle_chat(&state, &headers, &mut req).await {
        Ok((resp, receipt)) => chat_success_response(resp, &receipt),
        Err(err) => {
            publish_blocked_receipt(&state, &headers, Some(&req.model), None, &err).await;
            gate_error_response(err)
        }
    }
}

async fn tools_invoke(
    State(state): State<GateServerState>,
    headers: HeaderMap,
    Json(req): Json<ToolInvokeRequest>,
) -> Response {
    match handle_tool_invoke(&state, &headers, &req).await {
        Ok((body, receipt)) => tool_success_response(body, &receipt),
        Err(err) => {
            publish_blocked_receipt(
                &state,
                &headers,
                Some(&req.model),
                Some(&req.tool),
                &err,
            )
            .await;
            gate_error_response(err)
        }
    }
}

async fn publish_blocked_receipt(
    state: &GateServerState,
    headers: &HeaderMap,
    model: Option<&str>,
    tool_id: Option<&str>,
    err: &GateError,
) {
    let iac = load_iac(headers, state.default_iac.as_ref()).ok();
    let scope_id = iac.as_ref().map(|iac| iac.scope_id.as_str());
    let iac_hash = iac.as_ref().and_then(|iac| iac.hash_hex().ok());
    let tools = iac
        .as_ref()
        .map(|iac| iac.tool_allowlist.clone())
        .unwrap_or_default();
    let model_id = model.and_then(|m| {
        state
            .gate
            .registry()
            .model_allowlist
            .contains(&m.to_string())
            .then_some(m)
    });
    let (last_cp_hash, prev_event_id) = {
        let chain = state.session_chain.lock().await;
        (chain.last_cp_hash.clone(), chain.last_event_id.clone())
    };

    if let Err(publish_err) = state
        .gate
        .publish_action_blocked_detailed(
            scope_id,
            model_id,
            tool_id,
            &tools,
            iac_hash.as_deref(),
            &err.to_string(),
            now_secs(),
            &last_cp_hash,
            prev_event_id.as_deref(),
        )
        .await
    {
        warn!(error = %publish_err, "failed to publish blocked-action receipt");
    }
}

async fn handle_chat(
    state: &GateServerState,
    headers: &HeaderMap,
    req: &ChatRequest,
) -> Result<(ChatResponse, SessionReceipt), GateError> {
    let iac = load_iac(headers, state.default_iac.as_ref())?;
    let now = now_secs();

    state.gate.validate_iac(Some(&iac), now)?;

    let mut chain = state.session_chain.lock().await;
    ensure_live_session(state, &iac, &mut chain, now).await?;

    let parent_cp_hash = chain.next_parent_cp_hash(&iac.parent_cp_hash);
    let session_nonce = chain.next_session_nonce();
    let prev_event_id = chain.last_event_id.clone();

    let session_req = RouterSessionRequest {
        model_id: req.model.clone(),
        context_chunks: req
            .messages
            .iter()
            .map(|m| serde_json::to_vec(m).map_err(|e| GateError::Registry(e.to_string())))
            .collect::<Result<_, _>>()?,
        session_nonce,
        parent_cp_hash: parent_cp_hash.clone(),
    };

    let outcome = state
        .gate
        .open_router_session(Some(&iac), session_req, now, prev_event_id.as_deref())
        .await?;

    let cp_hash = cp_hash_hex(&outcome.event).map_err(|e| GateError::Bus(e.into()))?;
    chain.record_cp(cp_hash.clone(), outcome.bus_event_id.clone(), now);

    let receipt = SessionReceipt {
        scope_id: iac.scope_id.clone(),
        session_nonce,
        cp_hash: cp_hash.clone(),
        context_merkle_root: outcome.context_merkle_root.clone(),
        parent_cp_hash,
        bus_event_id: outcome.bus_event_id.clone(),
    };

    info!(
        scope_id = %iac.scope_id,
        session_nonce,
        cp_hash = %cp_hash,
        "membrane.cp.router published"
    );

    drop(chain);

    let response = state.proxy.chat(req).await.map_err(|e| GateError::Bus(e))?;
    Ok((response, receipt))
}

/// Production tool path: IAC + allowlist + liveness first; GitHub only after allow.
async fn handle_tool_invoke(
    state: &GateServerState,
    headers: &HeaderMap,
    req: &ToolInvokeRequest,
) -> Result<(serde_json::Value, SessionReceipt), GateError> {
    let iac = load_iac(headers, state.default_iac.as_ref())?;
    let now = now_secs();

    state.gate.validate_iac(Some(&iac), now)?;

    if !iac.model_allowed(&req.model) || !state.gate.registry().model_allowlist.contains(&req.model)
    {
        return Err(GateError::ModelDenied(req.model.clone()));
    }

    // Hard block out-of-scope tools before any upstream connector call.
    state.gate.authorize_tool(&iac, &req.tool, now)?;

    if !is_github_tool(&req.tool) {
        return Err(GateError::Connector(format!(
            "no real connector for tool '{}'; supported: github.comment, github.merge, github.issue.read",
            req.tool
        )));
    }

    // Repo allowlist + token presence — still before GitHub HTTP.
    state.github.preflight(req).map_err(map_github_err)?;

    // Fail closed if severed / stale before the mutating call.
    {
        let mut chain = state.session_chain.lock().await;
        ensure_live_session(state, &iac, &mut chain, now).await?;
    }

    let tool_ctx = state.github.execute(req).await.map_err(map_github_err)?;

    let mut chain = state.session_chain.lock().await;
    ensure_live_session(state, &iac, &mut chain, now).await?;

    let parent_cp_hash = chain.next_parent_cp_hash(&iac.parent_cp_hash);
    let session_nonce = chain.next_session_nonce();
    let prev_event_id = chain.last_event_id.clone();

    let context_chunks = vec![serde_json::to_vec(&tool_ctx)
        .map_err(|e| GateError::Registry(e.to_string()))?];

    let outcome = state
        .gate
        .open_router_session(
            Some(&iac),
            RouterSessionRequest {
                model_id: req.model.clone(),
                context_chunks,
                session_nonce,
                parent_cp_hash: parent_cp_hash.clone(),
            },
            now,
            prev_event_id.as_deref(),
        )
        .await?;

    let cp_hash = cp_hash_hex(&outcome.event).map_err(|e| GateError::Bus(e.into()))?;
    chain.record_cp(cp_hash.clone(), outcome.bus_event_id.clone(), now);

    let receipt = SessionReceipt {
        scope_id: iac.scope_id.clone(),
        session_nonce,
        cp_hash: cp_hash.clone(),
        context_merkle_root: outcome.context_merkle_root.clone(),
        parent_cp_hash,
        bus_event_id: outcome.bus_event_id.clone(),
    };

    info!(
        scope_id = %iac.scope_id,
        tool = %req.tool,
        cp_hash = %cp_hash,
        "membrane tool invoke allowed"
    );

    let body = json!({
        "ok": true,
        "status": "allowed",
        "simulation": false,
        "tool": req.tool,
        "model": req.model,
        "owner": req.owner,
        "repo": req.repo,
        "body_sha256": tool_ctx.body_sha256,
        "result": tool_ctx.result,
        "receipt": {
            "scope_id": receipt.scope_id,
            "session_nonce": receipt.session_nonce,
            "cp_hash": receipt.cp_hash,
            "context_merkle_root": receipt.context_merkle_root,
            "parent_cp_hash": receipt.parent_cp_hash,
            "bus_event_id": receipt.bus_event_id,
        }
    });

    Ok((body, receipt))
}

async fn ensure_live_session(
    state: &GateServerState,
    iac: &IntentAuthorizationCredential,
    chain: &mut SessionChainState,
    now: i64,
) -> Result<(), GateError> {
    // Check degraded before begin_scope — switching onto a severed scope must not clear it.
    if let Err(err) = state
        .gate
        .check_session_liveness(chain, &iac.scope_id, now)
    {
        if matches!(err, GateError::SessionDegraded(_, _)) {
            return Err(err);
        }
        if matches!(err, GateError::SessionStale(_, _)) {
            let age = chain.last_router_cp_age_secs(now);
            let prev = chain.last_event_id.clone();
            let cp_hash = chain.last_cp_hash.clone();
            let scope_id = iac.scope_id.clone();
            state
                .gate
                .publish_alert_degraded(
                    &scope_id,
                    ALERT_REASON_DELTA_T_EXCEEDED,
                    now,
                    &cp_hash,
                    age,
                    prev.as_deref(),
                )
                .await?;
            chain.mark_degraded(&iac.scope_id, ALERT_REASON_DELTA_T_EXCEEDED, now);
            return Err(err);
        }
        return Err(err);
    }

    let new_scope = chain.begin_scope(&iac.scope_id);
    if new_scope {
        // Fresh scope id only — does not revive a still-degraded scope (checked above).
        chain.clear_degraded_for_scope(&iac.scope_id);
    }

    chain
        .validate_iac_anchor(&iac.parent_cp_hash, new_scope)
        .map_err(GateError::NoValidIac)?;
    Ok(())
}

fn map_github_err(err: crate::github::GitHubConnectorError) -> GateError {
    use crate::github::GitHubConnectorError;
    match err {
        GitHubConnectorError::TokenMissing => GateError::Connector(err.to_string()),
        GitHubConnectorError::RepoDenied(r) => GateError::RepoDenied(r),
        GitHubConnectorError::UnsupportedTool(t) => GateError::Connector(format!(
            "unsupported tool: {t}"
        )),
        GitHubConnectorError::InvalidArgs(m) => GateError::Registry(m),
        GitHubConnectorError::Api { status, message } => {
            GateError::Connector(format!("GitHub API {status}: {message}"))
        }
        GitHubConnectorError::Http(m) => GateError::Connector(m),
    }
}

fn chat_success_response(resp: ChatResponse, receipt: &SessionReceipt) -> Response {
    let mut headers = HeaderMap::new();
    attach_receipt_headers(&mut headers, receipt);
    (StatusCode::OK, headers, Json(resp)).into_response()
}

fn tool_success_response(body: serde_json::Value, receipt: &SessionReceipt) -> Response {
    let mut headers = HeaderMap::new();
    attach_receipt_headers(&mut headers, receipt);
    (StatusCode::OK, headers, Json(body)).into_response()
}

fn attach_receipt_headers(headers: &mut HeaderMap, receipt: &SessionReceipt) {
    set_header(headers, "x-membrane-scope-id", &receipt.scope_id);
    set_header(
        headers,
        "x-membrane-session-nonce",
        &receipt.session_nonce.to_string(),
    );
    set_header(headers, "x-membrane-cp-hash", &receipt.cp_hash);
    set_header(
        headers,
        "x-membrane-context-root",
        &receipt.context_merkle_root,
    );
    set_header(
        headers,
        "x-membrane-parent-cp-hash",
        &receipt.parent_cp_hash,
    );
    if let Some(id) = &receipt.bus_event_id {
        set_header(headers, "x-membrane-bus-event-id", id);
    }
}

fn set_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        headers.insert(name, v);
    }
}

fn load_iac(
    headers: &HeaderMap,
    default: Option<&IntentAuthorizationCredential>,
) -> Result<IntentAuthorizationCredential, GateError> {
    if let Some(raw) = headers.get("x-membrane-iac").and_then(|v| v.to_str().ok()) {
        return parse_iac_header(raw);
    }
    if let Some(iac) = default {
        return Ok(iac.clone());
    }
    Err(GateError::NoValidIac(
        "missing X-Membrane-IAC header and no default IAC configured".into(),
    ))
}

fn parse_iac_header(raw: &str) -> Result<IntentAuthorizationCredential, GateError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let trimmed = raw.trim();
    let json = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        let bytes = STANDARD
            .decode(trimmed)
            .map_err(|e| GateError::NoValidIac(format!("invalid base64 IAC: {e}")))?;
        String::from_utf8(bytes)
            .map_err(|e| GateError::NoValidIac(format!("invalid utf8 IAC: {e}")))?
    };
    serde_json::from_str(&json).map_err(|e| GateError::NoValidIac(format!("invalid IAC JSON: {e}")))
}

fn gate_error_response(err: GateError) -> Response {
    warn!(error = %err, "gate fail-closed");
    let status = match &err {
        GateError::NoValidIac(_)
        | GateError::InvalidIacSignature(_)
        | GateError::ChannelDenied(_)
        | GateError::ModelDenied(_)
        | GateError::ToolDenied(_)
        | GateError::RepoDenied(_)
        | GateError::ExportForbidden(_)
        | GateError::ContextBoundExceeded
        | GateError::SessionDegraded(_, _)
        | GateError::SessionStale(_, _)
        | GateError::Connector(_) => StatusCode::FORBIDDEN,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(json!({
            "error": {
                "message": err.to_string(),
                "type": "membrane_gate_error"
            },
            "ok": false,
            "status": "blocked",
            "simulation": false,
        })),
    )
        .into_response()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

#[cfg(test)]
mod tool_invoke_policy_tests {
    use super::*;
    use crate::github::{GitHubConnectorConfig, TOOL_GITHUB_COMMENT, TOOL_GITHUB_MERGE};
    use crate::ChannelRegistry;
    use membrane_core::{BusPublisher, BusPublisherConfig};
    use nostr::Keys;

    fn test_state(tools: Vec<String>, repos: Vec<String>) -> (GateServerState, IntentAuthorizationCredential) {
        let keys = Keys::generate();
        let registry = ChannelRegistry {
            permitted_channels: vec!["local-llm".into()],
            forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
            model_allowlist: vec!["demo".into()],
            delta_t_secs: 300,
            model_api_url: None,
            github_repo_allowlist: repos.clone(),
        };
        let publisher = BusPublisher::new(BusPublisherConfig {
            relay_url: "memory://tool-policy".into(),
            keys: keys.clone(),
        });
        let gate = Gate::new(registry, publisher);
        let mut iac = IntentAuthorizationCredential::new_session_with_tools(
            "pilot-scope",
            "demo",
            "0".repeat(64),
            4_102_444_800,
            vec!["local-llm".into()],
            vec!["cloud-telemetry".into(), "training-retention".into()],
            tools,
        );
        iac.sign(&keys).unwrap();
        let github = GitHubConnector::new(GitHubConnectorConfig {
            repo_allowlist: repos,
            api_base: "http://127.0.0.1:9".into(),
            token: Some("test-token".into()),
        });
        let state = GateServerState {
            gate: Arc::new(gate),
            proxy: Arc::new(LlmProxy::new(None)),
            default_iac: Some(iac.clone()),
            session_chain: Arc::new(Mutex::new(SessionChainState::genesis())),
            github: Arc::new(github),
        };
        (state, iac)
    }

    #[tokio::test]
    async fn blocks_merge_before_github_http() {
        let (state, _) = test_state(
            vec![TOOL_GITHUB_COMMENT.into()],
            vec!["acme/pilot".into()],
        );
        let headers = HeaderMap::new();
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_MERGE.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "pilot".into(),
            issue_number: None,
            pull_number: Some(1),
            body: None,
            commit_title: None,
        };
        let err = handle_tool_invoke(&state, &headers, &req)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::ToolDenied(_)));
    }

    #[tokio::test]
    async fn blocks_unlisted_repo_before_github_http() {
        let (state, _) = test_state(
            vec![TOOL_GITHUB_COMMENT.into()],
            vec!["acme/pilot".into()],
        );
        let headers = HeaderMap::new();
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "other".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("nope".into()),
            commit_title: None,
        };
        let err = handle_tool_invoke(&state, &headers, &req)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::RepoDenied(_)));
    }

    #[tokio::test]
    async fn fails_closed_after_sever() {
        use membrane_core::ALERT_REASON_SUBJECT_SEVER;
        let (state, _) = test_state(
            vec![TOOL_GITHUB_COMMENT.into()],
            vec!["acme/pilot".into()],
        );
        {
            let mut chain = state.session_chain.lock().await;
            chain.mark_degraded("pilot-scope", ALERT_REASON_SUBJECT_SEVER, now_secs());
        }
        let headers = HeaderMap::new();
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "pilot".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("after sever".into()),
            commit_title: None,
        };
        let err = handle_tool_invoke(&state, &headers, &req)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::SessionDegraded(_, _)));
    }
}
