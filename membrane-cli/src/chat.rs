use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use membrane_core::{
    alert_degraded_payload, fetch_membrane_events, fetch_session_chain_bootstrap, BusPublisher,
    BusPublisherConfig, EventType, IntentAuthorizationCredential, MembranePayload,
    SessionChainState, ALERT_REASON_SUBJECT_SEVER,
};
use membrane_gate::ChatMessage;
use nostr::Keys;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::config::MembraneConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnReceipt {
    pub timestamp: i64,
    pub scope_id: String,
    pub session_nonce: u64,
    pub cp_hash: String,
    pub context_merkle_root: String,
    pub parent_cp_hash: String,
    pub bus_event_id: Option<String>,
    pub prompt_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLog {
    pub scope_id: String,
    pub model: String,
    pub gate_url: String,
    pub relay_url: String,
    pub started_at: i64,
    pub turns: Vec<TurnReceipt>,
}

pub struct ChatClient {
    pub config: MembraneConfig,
    keys: Keys,
    http: reqwest::Client,
    messages: Vec<ChatMessage>,
    session_log: SessionLog,
    log_path: PathBuf,
}

impl ChatClient {
    pub async fn new(config: MembraneConfig, keys: Keys) -> Result<Self> {
        let scope_id = format!("sovereign-{}", now_secs());
        let log_path = MembraneConfig::sessions_dir()?.join(format!("{scope_id}.json"));
        let session_log = SessionLog {
            scope_id: scope_id.clone(),
            model: config.model.clone(),
            gate_url: config.gate_url.clone(),
            relay_url: config.relay_url.clone(),
            started_at: now_secs(),
            turns: Vec::new(),
        };
        Ok(Self {
            config,
            keys,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            messages: Vec::new(),
            session_log,
            log_path,
        })
    }

    pub async fn run_repl(&mut self) -> Result<()> {
        println!("Membrane sovereign chat");
        println!("  gate:  {}", self.config.gate_url);
        println!("  model: {}", self.config.model);
        println!("  scope: {}", self.session_log.scope_id);
        println!("  log:   {}", self.log_path.display());
        println!("Type a message and press Enter. Commands: /status, /receipts, /quit\n");

        let mut line = String::new();
        loop {
            line.clear();
            print!("you> ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            if std::io::stdin().read_line(&mut line)? == 0 {
                break;
            }
            let input = line.trim();
            if input.is_empty() {
                continue;
            }
            match input {
                "/quit" | "/exit" => break,
                "/status" => {
                    session_status(&self.config, &self.keys).await?;
                    continue;
                }
                "/receipts" => {
                    print_session_receipts(&self.session_log);
                    continue;
                }
                cmd if cmd.starts_with('/') => {
                    println!("unknown command: {cmd}");
                    continue;
                }
                _ => {}
            }

            match self.send_turn(input).await {
                Ok(reply) => println!("\nassistant> {reply}\n"),
                Err(err) => eprintln!("error: {err:#}\n"),
            }
        }

        self.flush_log()?;
        println!("session log saved to {}", self.log_path.display());
        Ok(())
    }

    pub async fn send_once(&mut self, prompt: &str) -> Result<String> {
        let reply = self.send_turn(prompt).await?;
        self.flush_log()?;
        Ok(reply)
    }

    async fn send_turn(&mut self, prompt: &str) -> Result<String> {
        self.messages.push(ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
        });

        let iac = ensure_session_iac(&self.config, &self.keys, &self.session_log.scope_id).await?;

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": self.messages,
            "stream": false,
        });

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let iac_json = serde_json::to_string(&iac)?;
        headers.insert(
            "x-membrane-iac",
            HeaderValue::from_str(&iac_json).context("IAC header")?,
        );

        let url = format!(
            "{}/v1/chat/completions",
            self.config.gate_url.trim_end_matches('/')
        );
        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let resp_headers = resp.headers().clone();
        let text = resp.text().await?;

        if !status.is_success() {
            if let Ok(err) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(msg) = err.pointer("/error/message").and_then(|v| v.as_str()) {
                    bail!("gate rejected request ({status}): {msg}");
                }
            }
            bail!("gate error ({status}): {text}");
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let reply = parsed
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        self.messages.push(ChatMessage {
            role: "assistant".into(),
            content: reply.clone(),
        });

        let receipt = TurnReceipt {
            timestamp: now_secs(),
            scope_id: header_str(&resp_headers, "x-membrane-scope-id")
                .unwrap_or_else(|| self.session_log.scope_id.clone()),
            session_nonce: header_str(&resp_headers, "x-membrane-session-nonce")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            cp_hash: header_str(&resp_headers, "x-membrane-cp-hash").unwrap_or_default(),
            context_merkle_root: header_str(&resp_headers, "x-membrane-context-root")
                .unwrap_or_default(),
            parent_cp_hash: header_str(&resp_headers, "x-membrane-parent-cp-hash")
                .unwrap_or_default(),
            bus_event_id: header_str(&resp_headers, "x-membrane-bus-event-id"),
            prompt_preview: truncate(prompt, 80),
        };

        println!(
            "[receipt] nonce={} cp={}…",
            receipt.session_nonce,
            &receipt.cp_hash[..16.min(receipt.cp_hash.len())]
        );

        self.session_log.turns.push(receipt);
        self.flush_log()?;
        Ok(reply)
    }

    fn flush_log(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.session_log)?;
        std::fs::write(&self.log_path, json)?;
        Ok(())
    }
}

pub async fn ensure_session_iac(
    config: &MembraneConfig,
    keys: &Keys,
    scope_id: &str,
) -> Result<IntentAuthorizationCredential> {
    let path = MembraneConfig::active_iac_path()?;
    let now = now_secs();

    if path.exists() {
        let iac: IntentAuthorizationCredential =
            serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        if iac.scope_id == scope_id && iac.is_valid_at(now) && iac.model_allowed(&config.model) {
            if let Err(err) = iac.verify_signature(&keys.public_key().to_hex()) {
                eprintln!("warning: active IAC invalid ({err}), re-issuing");
            } else {
                return Ok(iac);
            }
        }
    }

    let (parent_cp_hash, _, _) =
        fetch_session_chain_bootstrap(&config.relay_url, &keys.public_key().to_hex()).await?;

    let mut iac = IntentAuthorizationCredential::new_session(
        scope_id,
        &config.model,
        parent_cp_hash,
        now + config.ttl_secs,
        vec!["local-llm".into()],
        vec!["cloud-telemetry".into(), "training-retention".into()],
    );
    iac.sign(keys).map_err(|e| anyhow::anyhow!("{e}"))?;

    std::fs::write(&path, serde_json::to_string_pretty(&iac)?)?;
    Ok(iac)
}

pub async fn session_status(config: &MembraneConfig, keys: &Keys) -> Result<()> {
    let pubkey = keys.public_key().to_hex();
    let since = now_secs() - 86_400 * 7;
    let events = fetch_membrane_events(&config.relay_url, Some(since), 5_000).await?;
    let chain = SessionChainState::from_bus_events(&events, &pubkey);
    let now = now_secs();
    let delta_t = config.delta_t_secs;
    let cp_age = chain.last_router_cp_age_secs(now);

    println!("Membrane session status");
    println!("  subject:       {pubkey}");
    println!("  relay:         {}", config.relay_url);
    println!("  gate:          {}", config.gate_url);
    println!("  chain head:    {}", chain.last_cp_hash);
    println!(
        "  last event id: {}",
        chain.last_event_id.as_deref().unwrap_or("<none>")
    );
    println!("  last nonce:    {}", chain.session_nonce);
    println!("  Δt (liveness): {delta_t}s");
    match cp_age {
        Some(age) => {
            let stale = age > delta_t as i64;
            println!(
                "  last router CP: {age}s ago{}",
                if stale { " (STALE)" } else { "" }
            );
        }
        None => println!("  last router CP: <none>"),
    }

    if let Some(scope) = &chain.degraded_scope_id {
        println!(
            "  DEGRADED:      scope={scope} reason={}",
            chain.degraded_reason.as_deref().unwrap_or("unknown")
        );
    }

    if let Ok(path) = MembraneConfig::active_iac_path() {
        if path.exists() {
            let iac: IntentAuthorizationCredential =
                serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            let valid = iac.is_valid_at(now);
            let degraded = chain.is_scope_degraded(&iac.scope_id);
            println!(
                "  active IAC:    scope={} valid_until={} valid={valid} degraded={degraded}",
                iac.scope_id, iac.valid_until
            );
        }
    }

    let router_count = events
        .iter()
        .filter(|e| e.subject_pubkey == pubkey && e.event_type == EventType::CpRouter)
        .count();
    let anchor_count = events
        .iter()
        .filter(|e| e.subject_pubkey == pubkey && e.event_type == EventType::AnchorOts)
        .count();
    println!("  router CPs (7d): {router_count}");
    println!("  OTS anchors (7d): {anchor_count}");

    if let Ok(health) = fetch_gate_health(&config.gate_url).await {
        println!(
            "  gate health:   {}",
            health.get("status").and_then(|v| v.as_str()).unwrap_or("?")
        );
        if let Some(age) = health.get("last_cp_age_secs") {
            println!("  gate CP age:   {age}");
        }
    }

    Ok(())
}

pub async fn sever_session(
    config: &MembraneConfig,
    keys: &Keys,
    scope_id: Option<&str>,
) -> Result<()> {
    let pubkey = keys.public_key().to_hex();
    let since = now_secs() - 86_400 * 7;
    let events = fetch_membrane_events(&config.relay_url, Some(since), 5_000).await?;
    let chain = SessionChainState::from_bus_events(&events, &pubkey);
    let now = now_secs();

    let scope_id = match scope_id {
        Some(s) => s.to_string(),
        None => {
            if let Ok(path) = MembraneConfig::active_iac_path() {
                if path.exists() {
                    let iac: IntentAuthorizationCredential =
                        serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                    iac.scope_id
                } else {
                    bail!("no --scope-id and no active IAC; pass --scope-id");
                }
            } else {
                bail!("no --scope-id and no active IAC; pass --scope-id");
            }
        }
    };

    let publisher = BusPublisher::new(BusPublisherConfig {
        relay_url: config.relay_url.clone(),
        keys: keys.clone(),
    });

    let cp_age = chain.last_router_cp_age_secs(now);
    let mut event = membrane_core::MembraneEvent::new(
        membrane_core::EventType::AlertDegraded,
        &pubkey,
        &chain.last_cp_hash,
        now,
        alert_degraded_payload(
            ALERT_REASON_SUBJECT_SEVER,
            &scope_id,
            cp_age,
            config.delta_t_secs,
        ),
    );

    let prev = chain.last_event_id.as_deref();
    let event_id = publisher.publish(&mut event, prev).await?;
    println!("published membrane.alert.degraded (subject sever)");
    println!("  event id: {}", event_id.to_hex());
    println!("  scope_id: {scope_id}");
    println!("  reason:   {ALERT_REASON_SUBJECT_SEVER}");

    if let Ok(path) = MembraneConfig::active_iac_path() {
        if path.exists() {
            let iac: IntentAuthorizationCredential =
                serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            if iac.scope_id == scope_id {
                std::fs::remove_file(&path)?;
                println!("  removed active IAC at {}", path.display());
            }
        }
    }

    println!("issue fresh IAC with `membrane iac issue` before resuming chat");
    Ok(())
}

async fn fetch_gate_health(gate_url: &str) -> Result<serde_json::Value> {
    let url = format!("{}/health", gate_url.trim_end_matches('/'));
    let resp = reqwest::get(&url).await?.error_for_status()?;
    Ok(resp.json().await?)
}

pub async fn list_receipts(config: &MembraneConfig, keys: &Keys, since_secs: i64) -> Result<()> {
    let pubkey = keys.public_key().to_hex();
    let since = now_secs() - since_secs;
    let events = fetch_membrane_events(&config.relay_url, Some(since), 5_000).await?;

    println!(
        "Router CP receipts for {} (last {}s)",
        &pubkey[..16],
        since_secs
    );
    let mut count = 0usize;
    for event in events {
        if event.subject_pubkey != pubkey || event.event_type != EventType::CpRouter {
            continue;
        }
        let MembranePayload::Router(p) = &event.payload else {
            continue;
        };
        count += 1;
        println!(
            "  @{} nonce={} model={} context={}… cp_parent={}…",
            event.timestamp,
            p.session_nonce,
            p.model_id,
            &p.context_merkle_root[..16.min(p.context_merkle_root.len())],
            &p.parent_cp_hash[..16.min(p.parent_cp_hash.len())],
        );
    }
    if count == 0 {
        println!("  (no router CPs in window)");
    }
    Ok(())
}

pub fn print_session_receipts(log: &SessionLog) {
    if log.turns.is_empty() {
        println!("  (no turns yet)");
        return;
    }
    for t in &log.turns {
        println!(
            "  turn nonce={} cp={} prompt={:?}",
            t.session_nonce, t.cp_hash, t.prompt_preview
        );
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}
