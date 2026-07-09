use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use membrane_core::{
    EventType, IntentAuthorizationCredential, MembranePayload,
    fetch_membrane_events, fetch_session_chain_bootstrap,
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

        let iac = ensure_session_iac(
            &self.config,
            &self.keys,
            &self.session_log.scope_id,
        )
        .await?;

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
        let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
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
        if iac.scope_id == scope_id
            && iac.is_valid_at(now)
            && iac.model_allowed(&config.model)
        {
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
    let (last_cp_hash, last_event_id, session_nonce) =
        fetch_session_chain_bootstrap(&config.relay_url, &pubkey).await?;

    println!("Membrane session status");
    println!("  subject:       {pubkey}");
    println!("  relay:         {}", config.relay_url);
    println!("  gate:          {}", config.gate_url);
    println!("  chain head:    {last_cp_hash}");
    println!("  last event id: {}", last_event_id.unwrap_or_else(|| "<none>".into()));
    println!("  last nonce:    {session_nonce}");

    if let Ok(path) = MembraneConfig::active_iac_path() {
        if path.exists() {
            let iac: IntentAuthorizationCredential =
                serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            let valid = iac.is_valid_at(now_secs());
            println!(
                "  active IAC:    scope={} valid_until={} valid={valid}",
                iac.scope_id, iac.valid_until
            );
        }
    }

    let since = now_secs() - 86_400;
    let events = fetch_membrane_events(&config.relay_url, Some(since), 2_000).await?;
    let router_count = events
        .iter()
        .filter(|e| e.subject_pubkey == pubkey && e.event_type == EventType::CpRouter)
        .count();
    let anchor_count = events
        .iter()
        .filter(|e| e.subject_pubkey == pubkey && e.event_type == EventType::AnchorOts)
        .count();
    println!("  router CPs (24h): {router_count}");
    println!("  OTS anchors (24h): {anchor_count}");
    Ok(())
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
