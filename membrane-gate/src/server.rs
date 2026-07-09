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
use membrane_core::SessionChainState;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::proxy::{ChatRequest, ChatResponse, LlmProxy};
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
}

pub async fn run_gate_server(
    state: GateServerState,
    listen: &str,
) -> anyhow::Result<()> {
    let app = axum::Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(listen = %listen, "membrane gate listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "gate": "membrane-phase-0" }))
}

async fn chat_completions(
    State(state): State<GateServerState>,
    headers: HeaderMap,
    Json(mut req): Json<ChatRequest>,
) -> Response {
    match handle_chat(&state, &headers, &mut req).await {
        Ok((resp, receipt)) => chat_success_response(resp, &receipt),
        Err(err) => gate_error_response(err),
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

    let context_chunks: Vec<Vec<u8>> = req
        .messages
        .iter()
        .map(|m| serde_json::to_vec(m).map_err(|e| GateError::Registry(e.to_string())))
        .collect::<Result<_, _>>()?;

    let mut chain = state.session_chain.lock().await;
    let new_scope = chain.begin_scope(&iac.scope_id);
    chain
        .validate_iac_anchor(&iac.parent_cp_hash, new_scope)
        .map_err(GateError::NoValidIac)?;

    let parent_cp_hash = chain.next_parent_cp_hash(&iac.parent_cp_hash);
    let session_nonce = chain.next_session_nonce();
    let prev_event_id = chain.last_event_id.clone();

    let session_req = RouterSessionRequest {
        model_id: req.model.clone(),
        context_chunks,
        session_nonce,
        parent_cp_hash: parent_cp_hash.clone(),
    };

    let outcome = state
        .gate
        .open_router_session(Some(&iac), session_req, now, prev_event_id.as_deref())
        .await?;

    let cp_hash = cp_hash_hex(&outcome.event).map_err(|e| GateError::Bus(e.into()))?;
    chain.record_cp(cp_hash.clone(), outcome.bus_event_id.clone());

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

    let response = state.proxy.chat(req).await.map_err(|e| GateError::Bus(e))?;
    Ok((response, receipt))
}

fn chat_success_response(resp: ChatResponse, receipt: &SessionReceipt) -> Response {
    let mut headers = HeaderMap::new();
    set_header(&mut headers, "x-membrane-scope-id", &receipt.scope_id);
    set_header(
        &mut headers,
        "x-membrane-session-nonce",
        &receipt.session_nonce.to_string(),
    );
    set_header(&mut headers, "x-membrane-cp-hash", &receipt.cp_hash);
    set_header(
        &mut headers,
        "x-membrane-context-root",
        &receipt.context_merkle_root,
    );
    set_header(
        &mut headers,
        "x-membrane-parent-cp-hash",
        &receipt.parent_cp_hash,
    );
    if let Some(id) = &receipt.bus_event_id {
        set_header(&mut headers, "x-membrane-bus-event-id", id);
    }

    (StatusCode::OK, headers, Json(resp)).into_response()
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
    serde_json::from_str(&json)
        .map_err(|e| GateError::NoValidIac(format!("invalid IAC JSON: {e}")))
}

fn gate_error_response(err: GateError) -> Response {
    warn!(error = %err, "gate fail-closed");
    let status = match &err {
        GateError::NoValidIac(_) | GateError::InvalidIacSignature(_)
        | GateError::ChannelDenied(_) | GateError::ModelDenied(_)
        | GateError::ExportForbidden(_) | GateError::ContextBoundExceeded => StatusCode::FORBIDDEN,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(json!({
            "error": {
                "message": err.to_string(),
                "type": "membrane_gate_error"
            }
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
