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
    /// GitHub connector (operator installs). Demo dashboard does not use this.
    pub github: Arc<GitHubConnector>,
    pub audit: Arc<crate::audit::AuditLog>,
}

pub async fn run_gate_server(state: GateServerState, listen: &str) -> anyhow::Result<()> {
    // Dedicated loopback-only telemetry listener, never the production write router.
    let audit_listen =
        std::env::var("MEMBRANE_AUDIT_LISTEN").unwrap_or_else(|_| "127.0.0.1:8788".into());
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let audit_task = crate::audit::bind(state.clone(), &audit_listen).await?;
    spawn_delta_t_watchdog(state.gate.clone(), state.session_chain.clone());
    // Invalid alarm settings stop startup; they never silently disable alarms.
    let alarm_config = crate::alarm::AlarmConfig::from_env().map_err(anyhow::Error::msg)?;
    crate::alarm::spawn_alarm_task(state.audit.clone(), alarm_config);

    let app = axum::Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/tools/invoke", post(tools_invoke))
        .with_state(state);

    info!(listen = %listen, "membrane gate listening");
    let result = axum::serve(listener, app).await;
    audit_task.abort();
    result?;
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
        "caller_audience": state.gate.caller_audience(),
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
    request: Result<Json<ChatRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let mut req = match request {
        Ok(Json(req)) => req,
        Err(rejection) => {
            record_denied(
                &state,
                &GateError::Registry("invalid JSON request".into()),
                "chat",
            );
            return rejection.into_response();
        }
    };
    let mut identity = None;
    let authorization = (|| {
        let iac = load_iac(&headers, state.default_iac.as_ref())?;
        let proof = load_caller_proof(&headers)?;
        identity = Some(state.gate.authenticate_caller(
            &iac,
            &proof,
            "/v1/chat/completions",
            &req,
            now_secs(),
        )?);
        state
            .gate
            .authorize_identity(identity.as_deref().unwrap(), &iac, &req.model, None)
    })();
    if let Err(err) = authorization {
        record_denied_identity(&state, &err, "chat", identity);
        return gate_error_response(err);
    }
    let mut authorized = false;
    match handle_chat(&state, &headers, &mut req, &mut authorized).await {
        Ok((resp, receipt)) => chat_success_response(resp, &receipt),
        Err(err) => {
            if !authorized {
                record_denied_identity(&state, &err, "chat", identity.clone());
            }
            publish_blocked_receipt(&state, &headers, Some(&req.model), None, &err).await;
            gate_error_response(err)
        }
    }
}

async fn tools_invoke(
    State(state): State<GateServerState>,
    headers: HeaderMap,
    request: Result<Json<ToolInvokeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let req = match request {
        Ok(Json(req)) => req,
        Err(rejection) => {
            record_denied(
                &state,
                &GateError::Registry("invalid JSON request".into()),
                "tool",
            );
            return rejection.into_response();
        }
    };
    let mut identity = None;
    let authorization = (|| {
        let iac = load_iac(&headers, state.default_iac.as_ref())?;
        let proof = load_caller_proof(&headers)?;
        identity = Some(state.gate.authenticate_caller(
            &iac,
            &proof,
            "/v1/tools/invoke",
            &req,
            now_secs(),
        )?);
        state.gate.authorize_identity(
            identity.as_deref().unwrap(),
            &iac,
            &req.model,
            Some((
                req.tool.as_str(),
                format!("{}/{}", req.owner, req.repo).as_str(),
            )),
        )
    })();
    if let Err(err) = authorization {
        record_denied_identity(&state, &err, "tool", identity);
        return gate_error_response(err);
    }
    let mut authorized = false;
    match handle_tool_invoke(&state, &headers, &req, &mut authorized).await {
        Ok((body, receipt)) => tool_success_response(body, &receipt),
        Err(err) => {
            if !authorized {
                record_denied_identity(&state, &err, "tool", identity.clone());
            }
            publish_blocked_receipt(&state, &headers, Some(&req.model), Some(&req.tool), &err)
                .await;
            gate_error_response(err)
        }
    }
}

fn record_allowed(state: &GateServerState, iac: &IntentAuthorizationCredential, action: &str) {
    state.audit.record_authenticated(
        "allow",
        "all_authorization_checks_passed",
        state.gate.publisher_pubkey_hex(),
        Some(iac.scope_id.clone()),
        action,
        None,
        iac.caller_pubkey.clone(),
    );
}
fn record_denied(state: &GateServerState, err: &GateError, action: &str) {
    record_denied_identity(state, err, action, None);
}
fn record_denied_identity(
    state: &GateServerState,
    err: &GateError,
    action: &str,
    identity: Option<String>,
) {
    state.audit.record_authenticated(
        "deny",
        crate::audit::rule(err),
        state.gate.publisher_pubkey_hex(),
        None,
        action,
        crate::audit::subject(err),
        identity,
    );
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
    authorized: &mut bool,
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

    state.gate.authorize_identity(
        iac.caller_pubkey
            .as_deref()
            .ok_or_else(|| GateError::IdentityAuthentication("missing caller binding".into()))?,
        &iac,
        &req.model,
        None,
    )?;
    *authorized = true;
    record_allowed(state, &iac, "chat");
    let response = state.proxy.chat(req).await.map_err(|e| GateError::Bus(e))?;
    Ok((response, receipt))
}

/// Production tool path: IAC + allowlist + liveness first; GitHub only after allow.
async fn handle_tool_invoke(
    state: &GateServerState,
    headers: &HeaderMap,
    req: &ToolInvokeRequest,
    authorized: &mut bool,
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
            "no connector configured for tool '{}'; supported: github.comment, github.merge, github.issue.read",
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

    state.gate.authorize_identity(
        iac.caller_pubkey
            .as_deref()
            .ok_or_else(|| GateError::IdentityAuthentication("missing caller binding".into()))?,
        &iac,
        &req.model,
        Some((&req.tool, &format!("{}/{}", req.owner, req.repo))),
    )?;
    *authorized = true;
    record_allowed(state, &iac, "tool");
    let tool_ctx = state.github.execute(req).await.map_err(map_github_err)?;

    let mut chain = state.session_chain.lock().await;
    ensure_live_session(state, &iac, &mut chain, now).await?;

    let parent_cp_hash = chain.next_parent_cp_hash(&iac.parent_cp_hash);
    let session_nonce = chain.next_session_nonce();
    let prev_event_id = chain.last_event_id.clone();

    let context_chunks =
        vec![serde_json::to_vec(&tool_ctx).map_err(|e| GateError::Registry(e.to_string()))?];

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
    if let Err(err) = state.gate.check_session_liveness(chain, &iac.scope_id, now) {
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
        GitHubConnectorError::UnsupportedTool(t) => {
            GateError::Connector(format!("unsupported tool: {t}"))
        }
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

fn load_caller_proof(headers: &HeaderMap) -> Result<membrane_core::caller::CallerProof, GateError> {
    let raw = headers
        .get("x-membrane-caller-proof")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| GateError::IdentityAuthentication("missing caller proof".into()))?;
    serde_json::from_str(raw)
        .map_err(|_| GateError::IdentityAuthentication("invalid caller proof".into()))
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
        GateError::IdentityAuthentication(_)
        | GateError::IdentityGrant(_)
        | GateError::GrantWindow(_)
        | GateError::NoValidIac(_)
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

    fn test_state(
        tools: Vec<String>,
        repos: Vec<String>,
    ) -> (GateServerState, IntentAuthorizationCredential) {
        let keys = Keys::generate();
        let registry = ChannelRegistry {
            identities: [(
                keys.public_key().to_hex(),
                crate::identity::IdentityGrant {
                    revoked: false,
                    scopes: vec!["pilot-scope".into()],
                    permitted_channels: vec!["local-llm".into()],
                    model_allowlist: vec!["demo".into()],
                    tool_allowlist: tools.clone(),
                    github_repo_allowlist: repos.clone(),
                    not_before: None,
                    expires_at: None,
                },
            )]
            .into(),
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
        iac.caller_pubkey = Some(keys.public_key().to_hex());
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
            audit: Arc::new(crate::audit::AuditLog::default()),
        };
        (state, iac)
    }

    #[tokio::test]
    async fn handler_records_denial_without_trusting_agent_headers() {
        let (state, _) = test_state(vec![], vec!["acme/pilot".into()]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_MERGE.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "pilot".into(),
            issue_number: None,
            pull_number: None,
            body: None,
            commit_title: None,
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-agent-id", "spoofed-agent".parse().unwrap());
        let proof = membrane_core::caller::CallerProof::sign(
            state.gate.publisher().keys(),
            &state.gate.caller_audience(),
            "/v1/tools/invoke",
            &req,
            state.default_iac.as_ref().unwrap(),
            now_secs(),
            nostr::Keys::generate().public_key().to_hex(),
        )
        .unwrap();
        headers.insert(
            "x-membrane-caller-proof",
            serde_json::to_string(&proof).unwrap().parse().unwrap(),
        );
        let response = tools_invoke(State(state.clone()), headers, Ok(Json(req))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let view = crate::audit::snapshot(&state).await;
        assert_eq!(view.denied, 1);
        assert_eq!(view.allowed, 0);
        assert_eq!(view.decisions[0].rule, "tool_allowlist");
        assert_eq!(view.decisions[0].agent, state.gate.publisher_pubkey_hex());
        assert_eq!(view.deny_rate, Some(1.0));
        assert_eq!(view.status, "idle");
    }
    #[tokio::test]
    async fn upstream_failure_does_not_rewrite_allow_as_deny() {
        let (state, _) = test_state(vec![TOOL_GITHUB_COMMENT.into()], vec!["acme/pilot".into()]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "pilot".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("local test".into()),
            commit_title: None,
        };
        let mut headers = HeaderMap::new();
        let proof = membrane_core::caller::CallerProof::sign(
            state.gate.publisher().keys(),
            &state.gate.caller_audience(),
            "/v1/tools/invoke",
            &req,
            state.default_iac.as_ref().unwrap(),
            now_secs(),
            nostr::Keys::generate().public_key().to_hex(),
        )
        .unwrap();
        headers.insert(
            "x-membrane-caller-proof",
            serde_json::to_string(&proof).unwrap().parse().unwrap(),
        );
        let _ = tools_invoke(State(state.clone()), headers, Ok(Json(req))).await;
        let view = crate::audit::snapshot(&state).await;
        assert_eq!(view.allowed, 1);
        assert_eq!(view.denied, 0);
        assert_eq!(view.decisions[0].rule, "all_authorization_checks_passed");
    }
    #[tokio::test]
    async fn metrics_empty_and_degraded_are_honest() {
        let (state, iac) = test_state(vec![], vec![]);
        let view = crate::audit::snapshot(&state).await;
        assert_eq!(view.deny_rate, None);
        state
            .session_chain
            .lock()
            .await
            .mark_degraded(&iac.scope_id, "subject_sever", now_secs());
        let view = crate::audit::snapshot(&state).await;
        assert_eq!(view.status, "degraded");
        assert!(view.degraded);
    }

    #[tokio::test]
    async fn blocks_merge_before_github_http() {
        let (state, _) = test_state(vec![TOOL_GITHUB_COMMENT.into()], vec!["acme/pilot".into()]);
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
        let err = handle_tool_invoke(&state, &headers, &req, &mut false)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::ToolDenied(_)));
    }

    #[tokio::test]
    async fn blocks_unlisted_repo_before_github_http() {
        let (state, _) = test_state(vec![TOOL_GITHUB_COMMENT.into()], vec!["acme/pilot".into()]);
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
        let err = handle_tool_invoke(&state, &headers, &req, &mut false)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::RepoDenied(_)));
        // The audit log names the denied repo so a read-only advisor can act on it.
        record_denied(&state, &err, "tool");
        let (_, rows) = state.audit.snapshot().unwrap();
        assert_eq!(rows[0].rule, "repository_allowlist");
        assert_eq!(rows[0].subject.as_deref(), Some("acme/other"));
    }

    #[tokio::test]
    async fn fails_closed_after_sever() {
        use membrane_core::ALERT_REASON_SUBJECT_SEVER;
        let (state, _) = test_state(vec![TOOL_GITHUB_COMMENT.into()], vec!["acme/pilot".into()]);
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
        let err = handle_tool_invoke(&state, &headers, &req, &mut false)
            .await
            .unwrap_err();
        assert!(matches!(err, GateError::SessionDegraded(_, _)));
    }
    #[tokio::test]
    async fn unauthenticated_handler_has_no_identity_and_never_allows() {
        let (state, _) = test_state(vec![TOOL_GITHUB_COMMENT.into()], vec!["acme/pilot".into()]);
        let req = ToolInvokeRequest {
            tool: TOOL_GITHUB_COMMENT.into(),
            model: "demo".into(),
            owner: "acme".into(),
            repo: "pilot".into(),
            issue_number: Some(1),
            pull_number: None,
            body: Some("test".into()),
            commit_title: None,
        };
        let mut h = HeaderMap::new();
        h.insert("x-agent-id", "forged".parse().unwrap());
        let response = tools_invoke(State(state.clone()), h, Ok(Json(req))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let v = crate::audit::snapshot(&state).await;
        assert_eq!(v.allowed, 0);
        assert_eq!(v.decisions[0].rule, "identity_authentication");
        assert_eq!(v.decisions[0].authenticated_identity, None);
    }
    #[tokio::test]
    async fn distinct_caller_is_recorded_on_chat_allow_and_identity_denial() {
        let (mut state, mut iac) = test_state(vec![], vec![]);
        let caller = Keys::generate();
        let id = caller.public_key().to_hex();
        iac.caller_pubkey = Some(id.clone());
        iac.sign(state.gate.publisher().keys()).unwrap();
        let mut registry = state.gate.registry().clone();
        let mut grant = registry.identities.values().next().unwrap().clone();
        registry.identities = [(id.clone(), grant.clone())].into();
        state.gate = Arc::new(Gate::new(
            registry.clone(),
            BusPublisher::new(BusPublisherConfig {
                relay_url: "memory://identity-http".into(),
                keys: state.gate.publisher().keys().clone(),
            }),
        ));
        state.default_iac = Some(iac.clone());
        let req = ChatRequest {
            model: "demo".into(),
            messages: vec![crate::ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            stream: false,
        };
        let proof = membrane_core::caller::CallerProof::sign(
            &caller,
            &state.gate.caller_audience(),
            "/v1/chat/completions",
            &req,
            &iac,
            now_secs(),
            Keys::generate().public_key().to_hex(),
        )
        .unwrap();
        let mut h = HeaderMap::new();
        h.insert(
            "x-membrane-caller-proof",
            serde_json::to_string(&proof).unwrap().parse().unwrap(),
        );
        assert_eq!(
            chat_completions(State(state.clone()), h, Ok(Json(req.clone())))
                .await
                .status(),
            StatusCode::OK
        );
        let v = crate::audit::snapshot(&state).await;
        assert_eq!(v.allowed, 1);
        assert_eq!(
            v.decisions[0].authenticated_identity.as_deref(),
            Some(id.as_str())
        );
        assert_ne!(v.decisions[0].agent, id);
        grant.revoked = true;
        registry.identities.insert(id.clone(), grant);
        state.gate = Arc::new(Gate::new(
            registry,
            BusPublisher::new(BusPublisherConfig {
                relay_url: "memory://identity-http".into(),
                keys: state.gate.publisher().keys().clone(),
            }),
        ));
        let proof = membrane_core::caller::CallerProof::sign(
            &caller,
            &state.gate.caller_audience(),
            "/v1/chat/completions",
            &req,
            &iac,
            now_secs(),
            Keys::generate().public_key().to_hex(),
        )
        .unwrap();
        let mut h = HeaderMap::new();
        h.insert(
            "x-membrane-caller-proof",
            serde_json::to_string(&proof).unwrap().parse().unwrap(),
        );
        assert_eq!(
            chat_completions(State(state.clone()), h, Ok(Json(req)))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let v = crate::audit::snapshot(&state).await;
        assert_eq!(v.decisions[0].rule, "identity_grant");
        assert_eq!(
            v.decisions[0].authenticated_identity.as_deref(),
            Some(id.as_str())
        );
    }
}
