//! Membrane local demo: signed authorizations, simulated tools, evidence packs.
//!
//! All `/demo/*` routes are demo-only. Production gate starts omit `DemoServerState`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Extension, Path, Query, Request, State},
    http::{
        header::{self, HeaderName},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use membrane_core::event::MembraneEvent;
use membrane_core::iac::IntentAuthorizationCredential;
use membrane_core::rollup::{cp_hash_hex, GENESIS_CP_HASH};
use membrane_core::{
    build_ocsf_inspired_pack, render_jsonl, SessionChainState, SiemEvent,
    ALERT_REASON_SUBJECT_SEVER, SIEM_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::info;

use crate::{Gate, GateError, RouterSessionRequest};

pub const DEMO_MODEL: &str = "support-agent-v1";
pub const DEMO_AGENT_ID: &str = "support-agent";
pub const DEMO_TTL_SECS: i64 = 900;
pub const DEMO_ALLOWED_TOOLS: &[&str] = &["jira.comment", "slack.post"];
pub const DEMO_BLOCKED_TOOL: &str = "github.merge";
pub const DEMO_SWAP_MODEL: &str = "unrestricted-agent-v9";

const DASHBOARD_HTML: &str = include_str!("demo_dashboard.html");
const SESSION_COOKIE: &str = "membrane_demo_session";
const SESSION_TTL_SECS: i64 = 30 * 60;
const MAX_SESSIONS: usize = 512;
const MAX_TIMELINE_ENTRIES: usize = 128;
const MAX_RECEIPTS: usize = 64;
const MAX_BODY_BYTES: usize = 64 * 1024;
const READS_PER_MINUTE: u32 = 120;
const WRITES_PER_MINUTE: u32 = 30;

#[derive(Clone)]
pub struct DemoServerState {
    pub gate: Arc<Gate>,
    pub session_chain: Arc<Mutex<SessionChainState>>,
    pub runtime: Arc<Mutex<DemoRuntime>>,
}

#[derive(Clone)]
struct DemoAppState {
    gate: Arc<Gate>,
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    rate_limits: Arc<Mutex<HashMap<String, RateWindow>>>,
    public: bool,
}

#[derive(Clone)]
struct SessionEntry {
    state: DemoServerState,
    last_seen: i64,
}

#[derive(Debug, Clone)]
struct RateWindow {
    started_at: i64,
    reads: u32,
    writes: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    Issued,
    Allowed,
    Blocked,
    Severed,
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: String,
    pub kind: TimelineKind,
    pub timestamp: i64,
    pub agent_id: String,
    pub scope_id: Option<String>,
    pub model: Option<String>,
    pub tool: Option<String>,
    pub reason: Option<String>,
    pub cp_hash: Option<String>,
    pub parent_cp_hash: Option<String>,
    pub iac_hash: Option<String>,
    pub bus_event_id: Option<String>,
    pub simulation: bool,
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceReceipt {
    pub action_id: String,
    pub timestamp: i64,
    pub model: String,
    pub tool: String,
    pub scope_id: String,
    pub iac_hash: String,
    pub cp_hash: String,
    pub parent_cp_hash: String,
    pub bus_event_id: Option<String>,
    pub event: MembraneEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePack {
    pub version: String,
    pub product: String,
    pub exported_at: i64,
    pub issuer_pubkey: String,
    pub agent_id: String,
    pub scope_id: Option<String>,
    pub simulation: bool,
    pub disclaimer: String,
    pub authorization: Option<Value>,
    pub receipts: Vec<EvidenceReceipt>,
    pub timeline: Vec<TimelineEntry>,
    pub chain_head: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerifyResult {
    pub ok: bool,
    pub receipts_checked: usize,
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct DemoRuntime {
    pub agent_id: String,
    pub active_iac: Option<IntentAuthorizationCredential>,
    pub timeline: Vec<TimelineEntry>,
    pub receipts: Vec<EvidenceReceipt>,
    pub next_id: u64,
}

impl DemoRuntime {
    pub fn new() -> Self {
        Self {
            agent_id: DEMO_AGENT_ID.into(),
            active_iac: None,
            timeline: Vec::new(),
            receipts: Vec::new(),
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> String {
        let id = format!("act-{:04}", self.next_id);
        self.next_id += 1;
        id
    }

    fn push(&mut self, entry: TimelineEntry) {
        self.timeline.push(entry);
        if self.timeline.len() > MAX_TIMELINE_ENTRIES {
            let overflow = self.timeline.len() - MAX_TIMELINE_ENTRIES;
            self.timeline.drain(0..overflow);
        }
    }
}

impl Default for DemoRuntime {
    fn default() -> Self {
        Self::new()
    }
}

pub fn demo_registry() -> crate::ChannelRegistry {
    crate::ChannelRegistry {
        permitted_channels: vec!["local-llm".into()],
        forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
        model_allowlist: vec![DEMO_MODEL.into()],
        // Generous Δt so a live demo is not interrupted mid-narrative.
        delta_t_secs: 86_400,
        model_api_url: None,
    }
}

pub fn demo_router(state: DemoServerState) -> Router {
    let public = std::env::var("MEMBRANE_DEMO_PUBLIC")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let app_state = DemoAppState {
        gate: state.gate,
        sessions: Arc::new(Mutex::new(HashMap::new())),
        rate_limits: Arc::new(Mutex::new(HashMap::new())),
        public,
    };

    Router::new()
        .route("/", get(dashboard))
        .route("/demo", get(dashboard))
        .route("/demo/", get(dashboard))
        .route("/health", get(demo_health))
        .route("/demo/api/overview", get(api_overview))
        .route("/demo/api/timeline", get(api_timeline))
        .route("/demo/api/actions/{id}", get(api_action_detail))
        .route("/demo/api/issue", post(api_issue))
        .route("/demo/api/action", post(api_action))
        .route("/demo/api/sever", post(api_sever))
        .route("/demo/api/reset", post(api_reset))
        .route("/demo/api/evidence", get(api_evidence_export))
        .route("/demo/api/evidence/verify", post(api_evidence_verify))
        .route("/demo/api/siem", get(api_siem_export))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            session_middleware,
        ))
        .with_state(app_state)
}

pub async fn run_demo_dashboard(state: DemoServerState, listen: &str) -> anyhow::Result<()> {
    let app = demo_router(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(listen = %listen, "local demo listening (demo-only endpoints enabled)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn session_middleware(
    State(app): State<DemoAppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == "/health" {
        return secured_response(next.run(request).await, app.public);
    }

    if !origin_is_same_host(request.headers(), request.method()) {
        return secured_response(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "cross-origin request denied" })),
            )
                .into_response(),
            app.public,
        );
    }

    let now = now_secs();
    let requested_id = cookie_value(request.headers(), SESSION_COOKIE);
    let (session_id, session_state, created) = {
        let mut sessions = app.sessions.lock().await;
        sessions.retain(|_, entry| now - entry.last_seen <= SESSION_TTL_SECS);

        if let Some(id) = requested_id.filter(|id| valid_session_id(id)) {
            if let Some(entry) = sessions.get_mut(&id) {
                entry.last_seen = now;
                (id, entry.state.clone(), false)
            } else {
                create_session(&app, &mut sessions, now)
            }
        } else {
            create_session(&app, &mut sessions, now)
        }
    };

    let rate_key = if app.public {
        request
            .headers()
            .get("cf-connecting-ip")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 64)
            .map(str::to_owned)
            .unwrap_or_else(|| session_id.clone())
    } else {
        session_id.clone()
    };
    if let Some(retry_after) = rate_limited(&app, &rate_key, request.method(), now).await {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limit exceeded", "retry_after_secs": retry_after })),
        )
            .into_response();
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_after.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("60")),
        );
        return secured_response(response, app.public);
    }

    request.extensions_mut().insert(session_state);
    let mut response = next.run(request).await;
    if created {
        let secure = if app.public { "; Secure" } else { "" };
        let cookie = format!(
            "{SESSION_COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECS}{secure}"
        );
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    secured_response(response, app.public)
}

fn create_session(
    app: &DemoAppState,
    sessions: &mut HashMap<String, SessionEntry>,
    now: i64,
) -> (String, DemoServerState, bool) {
    if sessions.len() >= MAX_SESSIONS {
        if let Some(oldest) = sessions
            .iter()
            .min_by_key(|(_, entry)| entry.last_seen)
            .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest);
        }
    }

    // A generated public key is a random 256-bit, non-secret session identifier.
    let session_id = nostr::Keys::generate().public_key().to_hex();
    let state = DemoServerState {
        gate: app.gate.clone(),
        session_chain: Arc::new(Mutex::new(SessionChainState::genesis())),
        runtime: Arc::new(Mutex::new(DemoRuntime::new())),
    };
    sessions.insert(
        session_id.clone(),
        SessionEntry {
            state: state.clone(),
            last_seen: now,
        },
    );
    (session_id, state, true)
}

async fn rate_limited(app: &DemoAppState, key: &str, method: &Method, now: i64) -> Option<i64> {
    let mut limits = app.rate_limits.lock().await;
    limits.retain(|_, window| now - window.started_at <= 120);
    let window = limits.entry(key.to_owned()).or_insert(RateWindow {
        started_at: now,
        reads: 0,
        writes: 0,
    });
    if now - window.started_at >= 60 {
        *window = RateWindow {
            started_at: now,
            reads: 0,
            writes: 0,
        };
    }

    let is_read = matches!(*method, Method::GET | Method::HEAD);
    let (count, limit) = if is_read {
        (&mut window.reads, READS_PER_MINUTE)
    } else {
        (&mut window.writes, WRITES_PER_MINUTE)
    };
    *count += 1;
    (*count > limit).then(|| (60 - (now - window.started_at)).max(1))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn valid_session_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn origin_is_same_host(headers: &HeaderMap, method: &Method) -> bool {
    if matches!(*method, Method::GET | Method::HEAD) {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .map(|origin_host| origin_host == host)
        .unwrap_or(false)
}

fn secured_response(mut response: Response, public: bool) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'none'; connect-src 'self'; form-action 'none'; frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if public {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    response
}

async fn demo_health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "gate": "demo",
        "demo": true,
        "simulation": true,
        "state": "ephemeral_per_session",
    }))
}

async fn api_overview(Extension(state): Extension<DemoServerState>) -> impl IntoResponse {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    let runtime = state.runtime.lock().await;
    let iac = runtime.active_iac.clone();
    let ttl_remaining = iac.as_ref().map(|i| (i.valid_until - now).max(0));
    let chain_fresh = !chain.is_router_stale(now, state.gate.registry().delta_t_secs);

    Json(json!({
        "product": "The Membrane",
        "demo": true,
        "simulation": true,
        "disclaimer": "Only gateway-routed traffic is enforced and attested. Tool calls are simulated locally.",
        "gateway": {
            "status": if chain.degraded_scope_id.is_some() { "severed" }
                else if iac.is_some() { "armed" }
                else { "idle" },
            "issuer_pubkey": state.gate.publisher_pubkey_hex(),
            "delta_t_secs": state.gate.registry().delta_t_secs,
            "last_cp_hash": chain.last_cp_hash,
            "last_cp_age_secs": chain.last_router_cp_age_secs(now),
            "chain_fresh": chain_fresh,
            "active_scope_id": chain.active_scope_id,
            "degraded_scope_id": chain.degraded_scope_id,
            "degraded_reason": chain.degraded_reason,
        },
        "agent": {
            "id": runtime.agent_id,
            "model": iac.as_ref().and_then(|i| i.model_allowlist.first().cloned()),
            "tools": iac.as_ref().map(|i| i.tool_allowlist.clone()).unwrap_or_default(),
            "authorization_ttl_secs": ttl_remaining,
            "valid_until": iac.as_ref().map(|i| i.valid_until),
            "scope_id": iac.as_ref().map(|i| i.scope_id.clone()),
            "iac_hash": iac.as_ref().and_then(|i| i.hash_hex().ok()),
        },
        "controls": {
            "allowed_tools": DEMO_ALLOWED_TOOLS,
            "blocked_tool": DEMO_BLOCKED_TOOL,
            "allowed_model": DEMO_MODEL,
            "swap_model": DEMO_SWAP_MODEL,
            "default_ttl_secs": DEMO_TTL_SECS,
        },
        "timeline_count": runtime.timeline.len(),
        "receipt_count": runtime.receipts.len(),
    }))
}

async fn api_timeline(Extension(state): Extension<DemoServerState>) -> impl IntoResponse {
    let runtime = state.runtime.lock().await;
    Json(json!({ "events": runtime.timeline }))
}

async fn api_action_detail(
    Extension(state): Extension<DemoServerState>,
    Path(id): Path<String>,
) -> Response {
    let runtime = state.runtime.lock().await;
    match runtime.timeline.iter().find(|e| e.id == id) {
        Some(entry) => Json(entry).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "action not found" })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct IssueRequest {
    #[serde(default = "default_ttl")]
    ttl_secs: i64,
}

fn default_ttl() -> i64 {
    DEMO_TTL_SECS
}

async fn api_issue(
    Extension(state): Extension<DemoServerState>,
    Json(req): Json<IssueRequest>,
) -> Response {
    match issue_authorization(&state, req.ttl_secs.clamp(60, DEMO_TTL_SECS)).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => demo_err(err),
    }
}

#[derive(Debug, Deserialize)]
struct ActionRequest {
    tool: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    ticket: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

async fn api_action(
    Extension(state): Extension<DemoServerState>,
    Json(req): Json<ActionRequest>,
) -> Response {
    if let Err(message) = validate_action_request(&req) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": message, "type": "invalid_demo_input" })),
        )
            .into_response();
    }
    match run_tool_action(&state, req).await {
        Ok((status, body)) => (status, Json(body)).into_response(),
        Err(err) => demo_err(err),
    }
}

fn validate_action_request(req: &ActionRequest) -> Result<(), &'static str> {
    if req.tool.len() > 64 || req.model.as_ref().is_some_and(|value| value.len() > 128) {
        return Err("tool or model identifier is too long");
    }
    if req.ticket.as_ref().is_some_and(|value| value.len() > 128)
        || req.channel.as_ref().is_some_and(|value| value.len() > 128)
        || req.body.as_ref().is_some_and(|value| value.len() > 2_000)
    {
        return Err("demo text exceeds the allowed length");
    }
    Ok(())
}

async fn api_sever(Extension(state): Extension<DemoServerState>) -> Response {
    match sever_session(&state).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => demo_err(err),
    }
}

async fn api_reset(Extension(state): Extension<DemoServerState>) -> Response {
    {
        let mut chain = state.session_chain.lock().await;
        *chain = SessionChainState::genesis();
    }
    {
        let mut runtime = state.runtime.lock().await;
        let id = runtime.alloc_id();
        *runtime = DemoRuntime::new();
        runtime.next_id = id.trim_start_matches("act-").parse::<u64>().unwrap_or(1) + 1;
        runtime.push(TimelineEntry {
            id,
            kind: TimelineKind::Reset,
            timestamp: now_secs(),
            agent_id: DEMO_AGENT_ID.into(),
            scope_id: None,
            model: None,
            tool: None,
            reason: Some("demo reset".into()),
            cp_hash: Some(GENESIS_CP_HASH.into()),
            parent_cp_hash: None,
            iac_hash: None,
            bus_event_id: None,
            simulation: true,
            detail: json!({ "note": "Demo state cleared; chain returned to genesis." }),
        });
    }
    (StatusCode::OK, Json(json!({ "ok": true, "reset": true }))).into_response()
}

async fn api_evidence_export(Extension(state): Extension<DemoServerState>) -> impl IntoResponse {
    let pack = build_evidence_pack(&state).await;
    Json(pack)
}

async fn api_evidence_verify(
    Extension(_state): Extension<DemoServerState>,
    Json(pack): Json<EvidencePack>,
) -> impl IntoResponse {
    Json(verify_evidence_pack(&pack))
}

#[derive(Debug, Deserialize)]
struct SiemExportQuery {
    #[serde(default = "default_siem_format")]
    format: String,
}

fn default_siem_format() -> String {
    "ocsf".into()
}

async fn api_siem_export(
    Extension(state): Extension<DemoServerState>,
    Query(query): Query<SiemExportQuery>,
) -> Response {
    let runtime = state.runtime.lock().await;
    let events: Vec<_> = runtime.timeline.iter().map(timeline_to_siem).collect();
    drop(runtime);

    match query.format.as_str() {
        "ocsf" => Json(build_ocsf_inspired_pack(&events, now_secs())).into_response(),
        "jsonl" => match render_jsonl(&events) {
            Ok(body) => (
                [
                    (header::CONTENT_TYPE, "application/x-ndjson"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"membrane-siem.jsonl\"",
                    ),
                ],
                body,
            )
                .into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("SIEM export failed: {err}") })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "format must be ocsf or jsonl" })),
        )
            .into_response(),
    }
}

async fn issue_authorization(state: &DemoServerState, ttl_secs: i64) -> Result<Value, GateError> {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    // Fresh demo scope after sever: clear degraded for new scope issuance.
    let scope_id = format!("demo-{now}");
    let parent = chain.last_cp_hash.clone();
    drop(chain);

    let tools: Vec<String> = DEMO_ALLOWED_TOOLS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut iac = IntentAuthorizationCredential::new_session_with_tools(
        scope_id.clone(),
        DEMO_MODEL,
        parent.clone(),
        now + ttl_secs,
        vec!["local-llm".into()],
        vec!["cloud-telemetry".into(), "training-retention".into()],
        tools.clone(),
    );
    // Sign via gate publisher keys (same identity that must verify IACs).
    iac.sign(state.gate.publisher().keys())
        .map_err(|e| GateError::InvalidIacSignature(e.to_string()))?;

    state.gate.validate_iac(Some(&iac), now)?;
    let iac_hash = iac
        .hash_hex()
        .map_err(|e| GateError::Bus(anyhow::anyhow!(e)))?;

    let mut runtime = state.runtime.lock().await;
    let id = runtime.alloc_id();
    let agent_id = runtime.agent_id.clone();
    runtime.active_iac = Some(iac.clone());
    runtime.push(TimelineEntry {
        id: id.clone(),
        kind: TimelineKind::Issued,
        timestamp: now,
        agent_id,
        scope_id: Some(scope_id.clone()),
        model: Some(DEMO_MODEL.into()),
        tool: None,
        reason: None,
        cp_hash: None,
        parent_cp_hash: Some(parent.clone()),
        iac_hash: Some(iac_hash.clone()),
        bus_event_id: None,
        simulation: true,
        detail: json!({
            "ttl_secs": ttl_secs,
            "valid_until": iac.valid_until,
            "tools": tools,
            "issuer_pubkey": state.gate.publisher_pubkey_hex(),
            "note": "Short-lived signed authorization issued for the support agent."
        }),
    });

    Ok(json!({
        "ok": true,
        "action_id": id,
        "scope_id": scope_id,
        "model": DEMO_MODEL,
        "tools": tools,
        "ttl_secs": ttl_secs,
        "valid_until": iac.valid_until,
        "parent_cp_hash": parent,
        "iac_hash": iac_hash,
        "issuer_pubkey": state.gate.publisher_pubkey_hex(),
        // Public credential fields only — never includes nsec.
        "authorization": public_iac_view(&iac),
    }))
}

async fn run_tool_action(
    state: &DemoServerState,
    req: ActionRequest,
) -> Result<(StatusCode, Value), GateError> {
    let now = now_secs();
    let model = req.model.unwrap_or_else(|| DEMO_MODEL.to_string());
    let tool = req.tool;

    let iac = {
        let runtime = state.runtime.lock().await;
        runtime.active_iac.clone()
    };

    let Some(iac) = iac else {
        let mut runtime = state.runtime.lock().await;
        let id = runtime.alloc_id();
        let agent_id = runtime.agent_id.clone();
        let reason = "no live authorization; session idle or severed".to_string();
        runtime.push(TimelineEntry {
            id: id.clone(),
            kind: TimelineKind::Blocked,
            timestamp: now,
            agent_id,
            scope_id: None,
            model: Some(model.clone()),
            tool: Some(tool.clone()),
            reason: Some(reason.clone()),
            cp_hash: None,
            parent_cp_hash: None,
            iac_hash: None,
            bus_event_id: None,
            simulation: true,
            detail: json!({
                "blocked": true,
                "reason": reason,
                "note": "Fail-closed: no signed authorization is live."
            }),
        });
        return Ok((
            StatusCode::FORBIDDEN,
            json!({
                "ok": false,
                "status": "blocked",
                "action_id": id,
                "model": model,
                "tool": tool,
                "reason": reason,
                "simulation": true,
            }),
        ));
    };

    // Record blocked outcomes on the timeline without extending the CP chain.
    if let Err(err) = state.gate.validate_iac(Some(&iac), now) {
        return Ok(record_blocked(state, &iac, &model, &tool, now, err.to_string()).await);
    }

    {
        let chain = state.session_chain.lock().await;
        if let Err(err) = state
            .gate
            .check_session_liveness(&chain, &iac.scope_id, now)
        {
            return Ok(record_blocked(state, &iac, &model, &tool, now, err.to_string()).await);
        }
    }

    if !iac.model_allowed(&model) || !state.gate.registry().model_allowlist.contains(&model) {
        let err = GateError::ModelDenied(model.clone());
        return Ok(record_blocked(state, &iac, &model, &tool, now, err.to_string()).await);
    }

    if let Err(err) = state.gate.authorize_tool(&iac, &tool, now) {
        return Ok(record_blocked(state, &iac, &model, &tool, now, err.to_string()).await);
    }

    let sim_payload = simulate_tool(&tool, &req.ticket, &req.channel, &req.body);

    let mut chain = state.session_chain.lock().await;
    let new_scope = chain.begin_scope(&iac.scope_id);
    if new_scope {
        chain.clear_degraded_for_scope(&iac.scope_id);
    }
    chain
        .validate_iac_anchor(&iac.parent_cp_hash, new_scope)
        .map_err(GateError::NoValidIac)?;

    let parent_cp_hash = chain.next_parent_cp_hash(&iac.parent_cp_hash);
    let session_nonce = chain.next_session_nonce();
    let prev_event_id = chain.last_event_id.clone();

    let context_chunks = vec![serde_json::to_vec(&json!({
        "tool": tool,
        "model": model,
        "simulation": true,
        "payload": sim_payload,
    }))
    .map_err(|e| GateError::Registry(e.to_string()))?];

    let outcome = state
        .gate
        .open_router_session(
            Some(&iac),
            RouterSessionRequest {
                model_id: model.clone(),
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
    drop(chain);

    let iac_hash = iac
        .hash_hex()
        .map_err(|e| GateError::Bus(anyhow::anyhow!(e)))?;

    let mut runtime = state.runtime.lock().await;
    let id = runtime.alloc_id();
    let agent_id = runtime.agent_id.clone();
    let entry = TimelineEntry {
        id: id.clone(),
        kind: TimelineKind::Allowed,
        timestamp: now,
        agent_id,
        scope_id: Some(iac.scope_id.clone()),
        model: Some(model.clone()),
        tool: Some(tool.clone()),
        reason: None,
        cp_hash: Some(cp_hash.clone()),
        parent_cp_hash: Some(parent_cp_hash.clone()),
        iac_hash: Some(iac_hash.clone()),
        bus_event_id: outcome.bus_event_id.clone(),
        simulation: true,
        detail: json!({
            "session_nonce": session_nonce,
            "context_merkle_root": outcome.context_merkle_root,
            "simulation_result": sim_payload,
            "verification": "cp_hash linked to parent; IAC model/tool matched",
            "issuer_pubkey": state.gate.publisher_pubkey_hex(),
        }),
    };
    runtime.receipts.push(EvidenceReceipt {
        action_id: id.clone(),
        timestamp: now,
        model: model.clone(),
        tool: tool.clone(),
        scope_id: iac.scope_id.clone(),
        iac_hash: iac_hash.clone(),
        cp_hash: cp_hash.clone(),
        parent_cp_hash: parent_cp_hash.clone(),
        bus_event_id: outcome.bus_event_id.clone(),
        event: outcome.event,
    });
    if runtime.receipts.len() > MAX_RECEIPTS {
        let overflow = runtime.receipts.len() - MAX_RECEIPTS;
        runtime.receipts.drain(0..overflow);
    }
    runtime.push(entry);

    Ok((
        StatusCode::OK,
        json!({
            "ok": true,
            "status": "allowed",
            "action_id": id,
            "model": model,
            "tool": tool,
            "cp_hash": cp_hash,
            "parent_cp_hash": parent_cp_hash,
            "iac_hash": iac_hash,
            "bus_event_id": outcome.bus_event_id,
            "simulation": true,
            "simulation_result": sim_payload,
        }),
    ))
}

async fn record_blocked(
    state: &DemoServerState,
    iac: &IntentAuthorizationCredential,
    model: &str,
    tool: &str,
    now: i64,
    reason: String,
) -> (StatusCode, Value) {
    let iac_hash = iac.hash_hex().ok();
    let mut runtime = state.runtime.lock().await;
    let id = runtime.alloc_id();
    let agent_id = runtime.agent_id.clone();
    runtime.push(TimelineEntry {
        id: id.clone(),
        kind: TimelineKind::Blocked,
        timestamp: now,
        agent_id,
        scope_id: Some(iac.scope_id.clone()),
        model: Some(model.to_string()),
        tool: Some(tool.to_string()),
        reason: Some(reason.clone()),
        cp_hash: None,
        parent_cp_hash: None,
        iac_hash,
        bus_event_id: None,
        simulation: true,
        detail: json!({
            "blocked": true,
            "reason": reason,
            "issuer_pubkey": state.gate.publisher_pubkey_hex(),
            "note": "Action refused fail-closed; receipt chain not extended."
        }),
    });
    (
        StatusCode::FORBIDDEN,
        json!({
            "ok": false,
            "status": "blocked",
            "action_id": id,
            "model": model,
            "tool": tool,
            "reason": reason,
            "simulation": true,
        }),
    )
}

async fn sever_session(state: &DemoServerState) -> Result<Value, GateError> {
    let now = now_secs();
    let scope_id = {
        let runtime = state.runtime.lock().await;
        runtime
            .active_iac
            .as_ref()
            .map(|i| i.scope_id.clone())
            .or_else(|| None)
            .unwrap_or_else(|| "demo".into())
    };

    let (cp_hash, prev, age) = {
        let chain = state.session_chain.lock().await;
        (
            chain.last_cp_hash.clone(),
            chain.last_event_id.clone(),
            chain.last_router_cp_age_secs(now),
        )
    };

    let bus_id = state
        .gate
        .publish_alert_degraded(
            &scope_id,
            ALERT_REASON_SUBJECT_SEVER,
            now,
            &cp_hash,
            age,
            prev.as_deref(),
        )
        .await?;

    {
        let mut chain = state.session_chain.lock().await;
        chain.mark_degraded(&scope_id, ALERT_REASON_SUBJECT_SEVER, now);
        chain.last_event_id = Some(bus_id.clone());
    }

    let mut runtime = state.runtime.lock().await;
    let id = runtime.alloc_id();
    let agent_id = runtime.agent_id.clone();
    runtime.active_iac = None;
    runtime.push(TimelineEntry {
        id: id.clone(),
        kind: TimelineKind::Severed,
        timestamp: now,
        agent_id,
        scope_id: Some(scope_id.clone()),
        model: None,
        tool: None,
        reason: Some(ALERT_REASON_SUBJECT_SEVER.into()),
        cp_hash: Some(cp_hash.clone()),
        parent_cp_hash: None,
        iac_hash: None,
        bus_event_id: Some(bus_id.clone()),
        simulation: true,
        detail: json!({
            "alert": "membrane.alert.degraded",
            "reason": ALERT_REASON_SUBJECT_SEVER,
            "note": "Session severed; subsequent actions fail closed until a fresh authorization."
        }),
    });

    Ok(json!({
        "ok": true,
        "status": "severed",
        "action_id": id,
        "scope_id": scope_id,
        "reason": ALERT_REASON_SUBJECT_SEVER,
        "bus_event_id": bus_id,
        "last_cp_hash": cp_hash,
    }))
}

fn timeline_to_siem(entry: &TimelineEntry) -> SiemEvent {
    let (event_type, outcome, severity) = match entry.kind {
        TimelineKind::Issued => ("authorization_issued", "issued", "informational"),
        TimelineKind::Allowed => ("allowed_action", "allowed", "informational"),
        TimelineKind::Blocked => ("blocked_action", "blocked", "high"),
        TimelineKind::Severed => ("sever", "severed", "high"),
        TimelineKind::Reset => ("demo_reset", "observed", "informational"),
    };
    SiemEvent {
        schema_version: SIEM_SCHEMA_VERSION.into(),
        timestamp: entry.timestamp,
        event_id: entry.id.clone(),
        event_type: event_type.into(),
        outcome: outcome.into(),
        severity: severity.into(),
        agent_id: entry.agent_id.clone(),
        session_id: entry.scope_id.clone(),
        scope_id: entry.scope_id.clone(),
        models: entry.model.iter().cloned().collect(),
        tools: entry.tool.iter().cloned().collect(),
        policy_hash: entry.iac_hash.clone(),
        receipt_hash: entry.cp_hash.clone(),
        parent_receipt_hash: entry.parent_cp_hash.clone(),
        reason: entry.reason.clone(),
        simulation: entry.simulation,
        source_event_type: format!("demo.timeline.{:?}", entry.kind).to_lowercase(),
    }
}

async fn build_evidence_pack(state: &DemoServerState) -> EvidencePack {
    let chain = state.session_chain.lock().await;
    let runtime = state.runtime.lock().await;
    EvidencePack {
        version: "0.1.0".into(),
        product: "The Membrane".into(),
        exported_at: now_secs(),
        issuer_pubkey: state.gate.publisher_pubkey_hex(),
        agent_id: runtime.agent_id.clone(),
        scope_id: runtime.active_iac.as_ref().map(|i| i.scope_id.clone()),
        simulation: true,
        disclaimer: "Evidence covers gateway-routed Membrane demo traffic only. Tool side-effects are simulated.".into(),
        authorization: runtime.active_iac.as_ref().map(public_iac_view),
        receipts: runtime.receipts.clone(),
        timeline: runtime.timeline.clone(),
        chain_head: chain.last_cp_hash.clone(),
    }
}

pub fn verify_evidence_pack(pack: &EvidencePack) -> ChainVerifyResult {
    let mut errors = Vec::new();
    let mut expected_parent: Option<String> = None;
    let mut checked = 0usize;

    for receipt in &pack.receipts {
        checked += 1;
        match cp_hash_hex(&receipt.event) {
            Ok(computed) => {
                if computed != receipt.cp_hash {
                    errors.push(format!(
                        "{}: cp_hash mismatch (stored {} != recomputed {})",
                        receipt.action_id, receipt.cp_hash, computed
                    ));
                }
            }
            Err(e) => errors.push(format!("{}: digest error: {e}", receipt.action_id)),
        }

        if receipt.event.prev_cp_hash != receipt.parent_cp_hash {
            errors.push(format!(
                "{}: event.prev_cp_hash does not match parent_cp_hash",
                receipt.action_id
            ));
        }

        if let Some(prev) = &expected_parent {
            if &receipt.parent_cp_hash != prev {
                errors.push(format!(
                    "{}: parent_cp_hash {} does not continue prior head {}",
                    receipt.action_id, receipt.parent_cp_hash, prev
                ));
            }
        }

        expected_parent = Some(receipt.cp_hash.clone());
    }

    if let Some(last) = pack.receipts.last() {
        let reset_after = pack
            .timeline
            .iter()
            .any(|t| matches!(t.kind, TimelineKind::Reset) && t.timestamp >= last.timestamp);
        if !reset_after && pack.chain_head != last.cp_hash {
            errors.push(format!(
                "chain_head {} != last receipt {}",
                pack.chain_head, last.cp_hash
            ));
        }
    }

    ChainVerifyResult {
        ok: errors.is_empty(),
        receipts_checked: checked,
        errors,
    }
}

fn public_iac_view(iac: &IntentAuthorizationCredential) -> Value {
    json!({
        "version": iac.version,
        "scope_id": iac.scope_id,
        "permitted_channels": iac.permitted_channels,
        "model_allowlist": iac.model_allowlist,
        "tool_allowlist": iac.tool_allowlist,
        "forbidden_exports": iac.forbidden_exports,
        "valid_until": iac.valid_until,
        "parent_cp_hash": iac.parent_cp_hash,
        "signature": iac.signature,
        // Explicitly omit any private key material (none is stored on IAC).
    })
}

fn simulate_tool(
    tool: &str,
    ticket: &Option<String>,
    channel: &Option<String>,
    body: &Option<String>,
) -> Value {
    let body = body
        .clone()
        .unwrap_or_else(|| "Acknowledged — investigating.".into());
    match tool {
        "jira.comment" => json!({
            "simulated": true,
            "system": "jira",
            "action": "comment",
            "ticket": ticket.clone().unwrap_or_else(|| "INC-1042".into()),
            "body": body,
            "result": "Comment would be posted (simulation — no external call)."
        }),
        "slack.post" => json!({
            "simulated": true,
            "system": "slack",
            "action": "post",
            "channel": channel.clone().unwrap_or_else(|| "#incidents".into()),
            "body": body,
            "result": "Message would be posted (simulation — no external call)."
        }),
        other => json!({
            "simulated": true,
            "tool": other,
            "result": "Unrecognized tool in simulator."
        }),
    }
}

fn demo_err(err: GateError) -> Response {
    let status = match &err {
        GateError::NoValidIac(_)
        | GateError::InvalidIacSignature(_)
        | GateError::ChannelDenied(_)
        | GateError::ModelDenied(_)
        | GateError::ToolDenied(_)
        | GateError::ExportForbidden(_)
        | GateError::ContextBoundExceeded
        | GateError::SessionDegraded(_, _)
        | GateError::SessionStale(_, _) => StatusCode::FORBIDDEN,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(json!({
            "ok": false,
            "error": err.to_string(),
            "type": "demo_error"
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
mod tests {
    use super::*;
    use membrane_core::{BusPublisher, BusPublisherConfig};
    use nostr::Keys;

    fn test_state() -> DemoServerState {
        let keys = Keys::generate();
        let publisher = BusPublisher::new(BusPublisherConfig {
            relay_url: "memory://demo".into(),
            keys,
        });
        let gate = Arc::new(Gate::new(demo_registry(), publisher));
        DemoServerState {
            gate,
            session_chain: Arc::new(Mutex::new(SessionChainState::genesis())),
            runtime: Arc::new(Mutex::new(DemoRuntime::new())),
        }
    }

    #[tokio::test]
    async fn issue_allow_block_sever_evidence() {
        let state = test_state();
        issue_authorization(&state, 900).await.unwrap();

        let (status, body) = run_tool_action(
            &state,
            ActionRequest {
                tool: "jira.comment".into(),
                model: None,
                ticket: Some("INC-1".into()),
                channel: None,
                body: Some("looking".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "allowed");

        let (status, body) = run_tool_action(
            &state,
            ActionRequest {
                tool: DEMO_BLOCKED_TOOL.into(),
                model: None,
                ticket: None,
                channel: None,
                body: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["status"], "blocked");

        let (status, body) = run_tool_action(
            &state,
            ActionRequest {
                tool: "jira.comment".into(),
                model: Some(DEMO_SWAP_MODEL.into()),
                ticket: None,
                channel: None,
                body: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body["reason"].as_str().unwrap().contains("model"));

        sever_session(&state).await.unwrap();
        let (status, body) = run_tool_action(
            &state,
            ActionRequest {
                tool: "slack.post".into(),
                model: None,
                ticket: None,
                channel: None,
                body: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["status"], "blocked");

        let pack = build_evidence_pack(&state).await;
        let verify = verify_evidence_pack(&pack);
        assert!(verify.ok, "errors: {:?}", verify.errors);
        assert_eq!(verify.receipts_checked, 1);

        let runtime = state.runtime.lock().await;
        let siem_events: Vec<_> = runtime.timeline.iter().map(timeline_to_siem).collect();
        drop(runtime);
        for required in [
            "authorization_issued",
            "allowed_action",
            "blocked_action",
            "sever",
        ] {
            assert!(
                siem_events.iter().any(|event| event.event_type == required),
                "missing SIEM event type {required}"
            );
        }
        let jsonl = render_jsonl(&siem_events).unwrap();
        assert!(!jsonl.contains("looking"));
        assert!(jsonl.lines().all(|line| {
            serde_json::from_str::<SiemEvent>(line)
                .map(|event| event.simulation)
                .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn evidence_detects_tampered_hash() {
        let state = test_state();
        issue_authorization(&state, 900).await.unwrap();
        run_tool_action(
            &state,
            ActionRequest {
                tool: "slack.post".into(),
                model: None,
                ticket: None,
                channel: Some("#ops".into()),
                body: Some("hi".into()),
            },
        )
        .await
        .unwrap();

        let mut pack = build_evidence_pack(&state).await;
        pack.receipts[0].cp_hash = "ab".repeat(32);
        let verify = verify_evidence_pack(&pack);
        assert!(!verify.ok);
        assert!(!verify.errors.is_empty());
    }
}
