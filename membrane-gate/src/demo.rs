//! Attestable local demo: signed authorizations, simulated tools, evidence packs.
//!
//! All `/demo/*` routes are demo-only. Production gate starts omit `DemoServerState`.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use membrane_core::event::MembraneEvent;
use membrane_core::iac::IntentAuthorizationCredential;
use membrane_core::rollup::{cp_hash_hex, GENESIS_CP_HASH};
use membrane_core::{SessionChainState, ALERT_REASON_SUBJECT_SEVER};
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

#[derive(Clone)]
pub struct DemoServerState {
    pub gate: Arc<Gate>,
    pub session_chain: Arc<Mutex<SessionChainState>>,
    pub runtime: Arc<Mutex<DemoRuntime>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        llama_cpp_url: None,
    }
}

pub fn demo_router(state: DemoServerState) -> Router {
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
        .with_state(state)
}

pub async fn run_attestable_demo(state: DemoServerState, listen: &str) -> anyhow::Result<()> {
    let app = demo_router(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(listen = %listen, "attestable demo listening (demo-only endpoints enabled)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn demo_health(State(state): State<DemoServerState>) -> impl IntoResponse {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    let runtime = state.runtime.lock().await;
    let delta_t_secs = state.gate.registry().delta_t_secs;
    Json(json!({
        "status": "ok",
        "gate": "attestable-demo",
        "demo": true,
        "simulation": true,
        "delta_t_secs": delta_t_secs,
        "last_cp_age_secs": chain.last_router_cp_age_secs(now),
        "active_scope_id": chain.active_scope_id,
        "degraded_scope_id": chain.degraded_scope_id,
        "degraded_reason": chain.degraded_reason,
        "has_authorization": runtime.active_iac.is_some(),
        "timeline_len": runtime.timeline.len(),
    }))
}

async fn api_overview(State(state): State<DemoServerState>) -> impl IntoResponse {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    let runtime = state.runtime.lock().await;
    let iac = runtime.active_iac.clone();
    let ttl_remaining = iac.as_ref().map(|i| (i.valid_until - now).max(0));
    let chain_fresh = !chain.is_router_stale(now, state.gate.registry().delta_t_secs);

    Json(json!({
        "product": "Attestable",
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

async fn api_timeline(State(state): State<DemoServerState>) -> impl IntoResponse {
    let runtime = state.runtime.lock().await;
    Json(json!({ "events": runtime.timeline }))
}

async fn api_action_detail(
    State(state): State<DemoServerState>,
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
    State(state): State<DemoServerState>,
    Json(req): Json<IssueRequest>,
) -> Response {
    match issue_authorization(&state, req.ttl_secs.max(60)).await {
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
    State(state): State<DemoServerState>,
    Json(req): Json<ActionRequest>,
) -> Response {
    match run_tool_action(&state, req).await {
        Ok((status, body)) => (status, Json(body)).into_response(),
        Err(err) => demo_err(err),
    }
}

async fn api_sever(State(state): State<DemoServerState>) -> Response {
    match sever_session(&state).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => demo_err(err),
    }
}

async fn api_reset(State(state): State<DemoServerState>) -> Response {
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

async fn api_evidence_export(State(state): State<DemoServerState>) -> impl IntoResponse {
    let pack = build_evidence_pack(&state).await;
    Json(pack)
}

async fn api_evidence_verify(
    State(_state): State<DemoServerState>,
    Json(pack): Json<EvidencePack>,
) -> impl IntoResponse {
    Json(verify_evidence_pack(&pack))
}

async fn issue_authorization(state: &DemoServerState, ttl_secs: i64) -> Result<Value, GateError> {
    let now = now_secs();
    let chain = state.session_chain.lock().await;
    // Fresh demo scope after sever: clear degraded for new scope issuance.
    let scope_id = format!("attestable-demo-{now}");
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
            .unwrap_or_else(|| "attestable-demo".into())
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

async fn build_evidence_pack(state: &DemoServerState) -> EvidencePack {
    let chain = state.session_chain.lock().await;
    let runtime = state.runtime.lock().await;
    EvidencePack {
        version: "0.1.0".into(),
        product: "Attestable".into(),
        exported_at: now_secs(),
        issuer_pubkey: state.gate.publisher_pubkey_hex(),
        agent_id: runtime.agent_id.clone(),
        scope_id: runtime.active_iac.as_ref().map(|i| i.scope_id.clone()),
        simulation: true,
        disclaimer: "Evidence covers gateway-routed Attestable demo traffic only. Tool side-effects are simulated.".into(),
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
            "type": "attestable_demo_error"
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
            relay_url: "memory://attestable-demo".into(),
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
