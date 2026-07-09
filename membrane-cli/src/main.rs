use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use chrono::Duration;
use clap::{Parser, Subcommand};
use membrane_core::{
    BusPublisher, BusPublisherConfig, EventType, HttpOtsStamper, IntentAuthorizationCredential,
    MembraneEvent, MembranePayload, MockOtsStamper, OtsStamper, RollupBundle, SessionChainState,
    SignedRollupBundle, build_rollup_bundle, day_bounds_utc, fetch_membrane_events,
    fetch_session_chain_bootstrap, keys_from_nsec, membrane_kind_for, npub_from_keys,
    subscribe_and_compute_bus_root, validate_rollup_bundle,
};
use membrane_gate::{
    ChannelRegistry, Gate, GateServerState, LlmProxy, RouterSessionRequest, run_gate_server,
};

mod chat;
mod config;

use chat::ChatClient;
use config::{MembraneConfig, config_path, write_example_config};

#[derive(Parser)]
#[command(name = "membrane", about = "The Membrane Phase 0 prototype — Nostr attestation bus")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Attestation bus operations
    Bus {
        #[command(subcommand)]
        command: BusCommands,
    },
    /// IAC gate and local LLM proxy (llama.cpp)
    Gate {
        #[command(subcommand)]
        command: GateCommands,
    },
    /// Daily rollup export, sign, and OTS stamp
    Rollup {
        #[command(subcommand)]
        command: RollupCommands,
    },
    /// Intent Authorization Credential tools
    Iac {
        #[command(subcommand)]
        command: IacCommands,
    },
    /// Sovereign local LLM chat through the membrane gate (auto IAC + receipts)
    Chat {
        #[arg(long)]
        message: Option<String>,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long)]
        gate_url: Option<String>,
        #[arg(long, env = "MEMBRANE_RELAY_URL")]
        relay_url: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Session chain status and bus receipts
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Write default config to ~/.config/membrane/config.yaml
    Init,
    /// Fail-closed demo: route without IAC, then with IAC
    Demo {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, default_value = "tools/channel-registry.example.yaml")]
        registry: PathBuf,
    },
}

#[derive(Subcommand)]
enum BusCommands {
    /// Publish a test MembraneEvent (kind 31990)
    PublishTest {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
    },
    /// Subscribe to bus events and recompute bus_root
    Subscribe {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long)]
        since: Option<i64>,
    },
}

#[derive(Subcommand)]
enum GateCommands {
    /// Start gate HTTP server (validates IAC before proxying to llama.cpp)
    Start {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, default_value = "tools/channel-registry.example.yaml")]
        registry: PathBuf,
        #[arg(long, default_value = "tools/demo-iac.json")]
        iac: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8787")]
        listen: String,
    },
}

#[derive(Subcommand)]
enum RollupCommands {
    /// Export RollupBundle for a UTC day from the attestation bus
    Export {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, value_name = "YYYY-MM-DD")]
        day: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Sign a RollupBundle and write signed JSON
    Sign {
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Stamp ots_digest with OpenTimestamps and publish membrane.anchor.ots
    Stamp {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        ots_out: PathBuf,
        #[arg(long)]
        mock: bool,
    },
    /// Export, sign, and stamp rollup for a UTC day (default: yesterday)
    Daily {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, value_name = "YYYY-MM-DD")]
        day: Option<String>,
        #[arg(long, default_value = "/var/lib/membrane/rollup")]
        work_dir: PathBuf,
        #[arg(long)]
        mock: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Show chain head, active IAC, and recent activity
    Status {
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, env = "MEMBRANE_RELAY_URL")]
        relay_url: Option<String>,
        #[arg(long)]
        gate_url: Option<String>,
    },
    /// List router CP events from the attestation bus
    Receipts {
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long, env = "MEMBRANE_RELAY_URL")]
        relay_url: Option<String>,
        #[arg(long, default_value = "86400")]
        since_secs: i64,
    },
}

#[derive(Subcommand)]
enum IacCommands {
    /// Issue a short-lived session IAC bound to the current CP chain head
    Issue {
        #[arg(long, env = "MEMBRANE_RELAY_URL", default_value = "ws://127.0.0.1:7777")]
        relay: String,
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long)]
        model: String,
        #[arg(long)]
        scope_id: Option<String>,
        #[arg(long, default_value = "3600")]
        ttl_secs: i64,
        #[arg(long)]
        parent_cp_hash: Option<String>,
        #[arg(long, default_value = "local-llm")]
        channel: Vec<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Sign an IAC JSON file with NOSTR_NSEC
    Sign {
        #[arg(long, env = "NOSTR_NSEC")]
        nsec: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Verify IAC signature against a subject pubkey
    Verify {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        pubkey: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("membrane=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Bus { command } => match command {
            BusCommands::PublishTest { relay, nsec } => bus_publish_test(&relay, nsec).await,
            BusCommands::Subscribe { relay, since } => bus_subscribe(&relay, since).await,
        },
        Commands::Gate { command } => match command {
            GateCommands::Start {
                relay,
                nsec,
                registry,
                iac,
                listen,
            } => gate_start(&relay, nsec, &registry, &iac, &listen).await,
        },
        Commands::Rollup { command } => match command {
            RollupCommands::Export {
                relay,
                nsec,
                day,
                out,
            } => rollup_export(&relay, nsec, &day, &out).await,
            RollupCommands::Sign { nsec, input, out } => rollup_sign(nsec, &input, &out),
            RollupCommands::Stamp {
                relay,
                nsec,
                input,
                ots_out,
                mock,
            } => rollup_stamp(&relay, nsec, &input, &ots_out, mock).await,
            RollupCommands::Daily {
                relay,
                nsec,
                day,
                work_dir,
                mock,
            } => rollup_daily(&relay, nsec, day.as_deref(), &work_dir, mock).await,
        },
        Commands::Iac { command } => match command {
            IacCommands::Issue {
                relay,
                nsec,
                model,
                scope_id,
                ttl_secs,
                parent_cp_hash,
                channel,
                out,
            } => {
                iac_issue(
                    &relay,
                    nsec,
                    &model,
                    scope_id.as_deref(),
                    ttl_secs,
                    parent_cp_hash.as_deref(),
                    &channel,
                    &out,
                )
                .await
            }
            IacCommands::Sign { nsec, input, out } => iac_sign(nsec, &input, &out),
            IacCommands::Verify { input, pubkey } => iac_verify(&input, &pubkey),
        },
        Commands::Demo {
            relay,
            nsec,
            registry,
        } => run_demo(&relay, nsec, &registry).await,
        Commands::Chat {
            message,
            nsec,
            gate_url,
            relay_url,
            model,
        } => run_chat(nsec, gate_url, relay_url, model, message).await,
        Commands::Session { command } => match command {
            SessionCommands::Status {
                nsec,
                relay_url,
                gate_url,
            } => run_session_status(nsec, relay_url, gate_url).await,
            SessionCommands::Receipts {
                nsec,
                relay_url,
                since_secs,
            } => run_session_receipts(nsec, relay_url, since_secs).await,
        },
        Commands::Init => run_init(),
    }
}

async fn run_chat(
    nsec: Option<String>,
    gate_url: Option<String>,
    relay_url: Option<String>,
    model: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let mut cfg = MembraneConfig::load()?;
    if let Some(u) = gate_url {
        cfg.gate_url = u;
    }
    if let Some(u) = relay_url {
        cfg.relay_url = u;
    }
    if let Some(m) = model {
        cfg.model = m;
    }

    let mut client = ChatClient::new(cfg, keys).await?;
    if let Some(msg) = message {
        let reply = client.send_once(&msg).await?;
        println!("{reply}");
    } else {
        client.run_repl().await?;
    }
    Ok(())
}

async fn run_session_status(
    nsec: Option<String>,
    relay_url: Option<String>,
    gate_url: Option<String>,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let mut cfg = MembraneConfig::load()?;
    if let Some(u) = relay_url {
        cfg.relay_url = u;
    }
    if let Some(u) = gate_url {
        cfg.gate_url = u;
    }
    chat::session_status(&cfg, &keys).await
}

async fn run_session_receipts(
    nsec: Option<String>,
    relay_url: Option<String>,
    since_secs: i64,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let mut cfg = MembraneConfig::load()?;
    if let Some(u) = relay_url {
        cfg.relay_url = u;
    }
    chat::list_receipts(&cfg, &keys, since_secs).await
}

fn run_init() -> Result<()> {
    let path = config_path().context("HOME not set")?;
    if path.exists() {
        println!("config already exists: {}", path.display());
        return Ok(());
    }
    write_example_config(&path)?;
    println!("wrote {}", path.display());
    println!("edit gate_url / relay_url / model for your setup");
    Ok(())
}

fn load_keys(nsec: Option<String>) -> Result<nostr::Keys> {
    let nsec = nsec.context("NOSTR_NSEC required (set env or --nsec)")?;
    keys_from_nsec(&nsec)
}

async fn bus_publish_test(relay: &str, nsec: Option<String>) -> Result<()> {
    let keys = load_keys(nsec)?;
    let publisher = BusPublisher::new(BusPublisherConfig {
        relay_url: relay.to_string(),
        keys: keys.clone(),
    });

    let now = now_secs();
    let mut event = MembraneEvent::new(
        EventType::CpLiveness,
        keys.public_key().to_hex(),
        "0".repeat(64),
        now,
        MembranePayload::Generic(serde_json::json!({
            "note": "phase-0 publish-test",
            "bus_probe": true
        })),
    );

    let id = publisher.publish(&mut event, None).await?;
    println!(
        "published kind {} event {}",
        membrane_kind_for(event.event_type),
        id.to_hex()
    );
    println!("npub: {}", npub_from_keys(&keys)?);
    Ok(())
}

async fn bus_subscribe(relay: &str, since: Option<i64>) -> Result<()> {
    let (events, root) = subscribe_and_compute_bus_root(relay, since).await?;
    println!("events: {}", events.len());
    for event in &events {
        println!(
            "  {} @ {} type={}",
            event.timestamp,
            event.subject_pubkey,
            event.event_type.as_str()
        );
    }
    println!("bus_root: {}", root.unwrap_or_else(|| "<empty>".into()));
    Ok(())
}

async fn gate_start(
    relay: &str,
    nsec: Option<String>,
    registry_path: &PathBuf,
    iac_path: &PathBuf,
    listen: &str,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let registry = ChannelRegistry::load(registry_path)?;
    let default_iac: IntentAuthorizationCredential =
        serde_json::from_str(&std::fs::read_to_string(iac_path)?)?;

    let publisher = BusPublisher::new(BusPublisherConfig {
        relay_url: relay.to_string(),
        keys: keys.clone(),
    });
    let gate = Arc::new(Gate::new(registry.clone(), publisher));

    gate.validate_iac(Some(&default_iac), now_secs())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let pubkey = keys.public_key().to_hex();
    let (last_cp_hash, last_event_id, session_nonce) =
        fetch_session_chain_bootstrap(relay, &pubkey).await?;
    let mut session_chain = SessionChainState::genesis();
    session_chain.last_cp_hash = last_cp_hash.clone();
    session_chain.last_event_id = last_event_id;
    session_chain.session_nonce = session_nonce;
    println!(
        "gate: chain head cp_hash={last_cp_hash} session_nonce={session_nonce}"
    );

    let llama = registry
        .llama_cpp_url
        .as_deref()
        .unwrap_or("<mock>");
    println!("gate: IAC ok, llama.cpp={llama}, listen={listen}");

    let proxy = Arc::new(LlmProxy::new(registry.llama_cpp_url.clone()));
    let state = GateServerState {
        gate,
        proxy,
        default_iac: Some(default_iac),
        session_chain: Arc::new(tokio::sync::Mutex::new(session_chain)),
    };

    run_gate_server(state, listen).await
}

async fn rollup_export(
    relay: &str,
    nsec: Option<String>,
    day: &str,
    out: &PathBuf,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let (period_start, period_end) = day_bounds_utc(day)?;
    let events = fetch_membrane_events(relay, Some(period_start), 5_000).await?;
    let bundle = build_rollup_bundle(
        &events,
        &keys.public_key().to_hex(),
        period_start,
        period_end,
    )?;
    validate_rollup_bundle(&bundle)?;

    let json = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(out, json)?;
    println!("rollup exported for {day}");
    println!("  period: {period_start} .. {period_end}");
    println!("  cp_chain_root: {}", bundle.cp_chain_root);
    println!("  last_bus_root: {}", bundle.last_bus_root);
    println!("  out: {}", out.display());
    Ok(())
}

fn rollup_sign(nsec: Option<String>, input: &PathBuf, out: &PathBuf) -> Result<()> {
    let keys = load_keys(nsec)?;
    let bundle: RollupBundle =
        serde_json::from_str(&std::fs::read_to_string(input)?).context("parse rollup bundle")?;
    validate_rollup_bundle(&bundle)?;
    let signed = SignedRollupBundle::sign(bundle, &keys)?;
    let json = serde_json::to_string_pretty(&signed)?;
    std::fs::write(out, json)?;
    println!("signed rollup written to {}", out.display());
    println!("  ots_digest: {}", signed.ots_digest_hex()?);
    Ok(())
}

async fn rollup_stamp(
    relay: &str,
    nsec: Option<String>,
    input: &PathBuf,
    ots_out: &PathBuf,
    mock: bool,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let signed: SignedRollupBundle =
        serde_json::from_str(&std::fs::read_to_string(input)?).context("parse signed rollup")?;
    validate_rollup_bundle(&signed.bundle)?;

    let digest = signed.ots_digest()?;
    let digest_hex = hex::encode(digest);
    println!("ots_digest: {digest_hex}");

    let stamp = if mock {
        MockOtsStamper.stamp_digest(digest).await?
    } else {
        HttpOtsStamper::new(HttpOtsStamper::default_calendars())?
            .stamp_digest(digest)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
    };

    std::fs::write(ots_out, &stamp.proof_bytes)?;
    println!(
        "OTS proof written to {} (calendar: {})",
        ots_out.display(),
        stamp.calendar_url
    );

    let publisher = BusPublisher::new(BusPublisherConfig {
        relay_url: relay.to_string(),
        keys: keys.clone(),
    });

    let now = now_secs();
    let mut anchor = MembraneEvent::new(
        EventType::AnchorOts,
        keys.public_key().to_hex(),
        signed.bundle.last_cp_hash.clone(),
        now,
        MembranePayload::Generic(serde_json::json!({
            "target": digest_hex,
            "ots_b64": stamp.proof_b64(),
            "period_end": signed.bundle.period_end,
            "cp_chain_root": signed.bundle.cp_chain_root,
            "last_bus_root": signed.bundle.last_bus_root,
        })),
    );

    let id = publisher.publish(&mut anchor, None).await?;
    println!(
        "published membrane.anchor.ots kind {} event {}",
        membrane_kind_for(EventType::AnchorOts),
        id.to_hex()
    );
    Ok(())
}

async fn iac_issue(
    relay: &str,
    nsec: Option<String>,
    model: &str,
    scope_id: Option<&str>,
    ttl_secs: i64,
    parent_cp_hash: Option<&str>,
    channels: &[String],
    out: &PathBuf,
) -> Result<()> {
    let keys = load_keys(nsec)?;
    let now = now_secs();
    let scope_id = scope_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("session-{}", now));

    let parent = match parent_cp_hash {
        Some(hash) => hash.to_string(),
        None => {
            let (head, _, _) =
                fetch_session_chain_bootstrap(relay, &keys.public_key().to_hex()).await?;
            head
        }
    };

    let mut iac = IntentAuthorizationCredential::new_session(
        scope_id.clone(),
        model,
        parent.clone(),
        now + ttl_secs,
        channels.to_vec(),
        vec!["cloud-telemetry".into(), "training-retention".into()],
    );
    iac.sign(&keys).map_err(|e| anyhow::anyhow!("{e}"))?;

    let json = serde_json::to_string_pretty(&iac)?;
    std::fs::write(out, json)?;
    println!("session IAC written to {}", out.display());
    println!("  scope_id: {scope_id}");
    println!("  model: {model}");
    println!("  valid_until: {} (+{ttl_secs}s)", iac.valid_until);
    println!("  parent_cp_hash: {parent}");
    println!("  signer: {}", keys.public_key().to_hex());
    Ok(())
}

fn iac_sign(nsec: Option<String>, input: &PathBuf, out: &PathBuf) -> Result<()> {
    let keys = load_keys(nsec)?;
    let mut iac: IntentAuthorizationCredential =
        serde_json::from_str(&std::fs::read_to_string(input)?).context("parse IAC")?;
    iac.sign(&keys).map_err(|e| anyhow::anyhow!("{e}"))?;
    let json = serde_json::to_string_pretty(&iac)?;
    std::fs::write(out, json)?;
    println!("signed IAC written to {}", out.display());
    println!("  signer: {}", keys.public_key().to_hex());
    Ok(())
}

fn iac_verify(input: &PathBuf, pubkey: &str) -> Result<()> {
    let iac: IntentAuthorizationCredential =
        serde_json::from_str(&std::fs::read_to_string(input)?).context("parse IAC")?;
    iac.verify_signature(pubkey).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("IAC signature valid for pubkey {pubkey}");
    Ok(())
}

async fn rollup_daily(
    relay: &str,
    nsec: Option<String>,
    day: Option<&str>,
    work_dir: &PathBuf,
    mock: bool,
) -> Result<()> {
    let day = match day {
        Some(d) => d.to_string(),
        None => {
            let yesterday = chrono::Utc::now() - chrono::Duration::days(1);
            yesterday.format("%Y-%m-%d").to_string()
        }
    };

    std::fs::create_dir_all(work_dir)?;
    let rollup_path = work_dir.join(format!("rollup-{day}.json"));
    let signed_path = work_dir.join(format!("rollup-{day}.signed.json"));
    let ots_path = work_dir.join(format!("rollup-{day}.ots"));

    rollup_export(relay, nsec.clone(), &day, &rollup_path).await?;
    rollup_sign(nsec.clone(), &rollup_path, &signed_path)?;
    rollup_stamp(relay, nsec, &signed_path, &ots_path, mock).await?;
    println!("daily rollup complete for {day}");
    Ok(())
}

async fn run_demo(relay: &str, nsec: Option<String>, registry_path: &PathBuf) -> Result<()> {
    let keys = load_keys(nsec)?;
    let registry = ChannelRegistry::load(registry_path)?;
    let publisher = BusPublisher::new(BusPublisherConfig {
        relay_url: relay.to_string(),
        keys: keys.clone(),
    });
    let gate = Gate::new(registry.clone(), publisher);

    let now = now_secs();
    let req = RouterSessionRequest {
        model_id: "sha256:demo-model".into(),
        context_chunks: vec![b"demo prompt chunk".to_vec()],
        session_nonce: 42,
        parent_cp_hash: "0".repeat(64),
    };

    println!("=== Step 1: open router without IAC (expect fail-closed) ===");
    match gate.open_router_session(None, req.clone(), now, None).await {
        Ok(_) => bail!("expected fail-closed without IAC"),
        Err(err) => println!("FAIL-CLOSED: {err}"),
    }

    let iac = demo_iac(&keys, now);
    println!("\n=== Step 2: open router with valid IAC (expect bus event) ===");
    let outcome = gate.open_router_session(Some(&iac), req.clone(), now, None).await?;
    println!("OK: membrane.cp.router published");
    println!("  bus_event_id: {}", outcome.bus_event_id.as_deref().unwrap_or_default());
    println!("  context_merkle_root: {}", outcome.context_merkle_root);
    println!("  cp_hash: {}", outcome.cp_hash);

    let req2 = RouterSessionRequest {
        model_id: "sha256:demo-model".into(),
        context_chunks: vec![b"second turn".to_vec()],
        session_nonce: 43,
        parent_cp_hash: outcome.cp_hash.clone(),
    };
    println!("\n=== Step 2b: chained router CP (parent = prior cp_hash) ===");
    let outcome2 = gate
        .open_router_session(Some(&iac), req2, now + 1, outcome.bus_event_id.as_deref())
        .await?;
    println!("  cp_hash: {}", outcome2.cp_hash);
    if outcome.cp_hash == outcome2.cp_hash {
        bail!("expected distinct cp hashes across chain");
    }

    let events = fetch_membrane_events(relay, Some(now - 60), 100).await?;
    let root = membrane_core::bus_root_from_events(&events)?;
    println!("\n=== Step 3: recompute bus_root from relay ===");
    println!("  events fetched: {}", events.len());
    println!("  bus_root: {}", root.unwrap_or_else(|| "<empty>".into()));

    let proxy = LlmProxy::new(registry.llama_cpp_url.clone());
    let llm = proxy.complete("sha256:demo-model", "membrane demo").await?;
    println!("\n=== Step 4: local LLM proxy (llama.cpp or mock) ===");
    println!("  {llm}");

    Ok(())
}

fn demo_iac(keys: &nostr::Keys, now: i64) -> IntentAuthorizationCredential {
    let mut iac = IntentAuthorizationCredential {
        version: IntentAuthorizationCredential::SCHEMA_VERSION.to_string(),
        scope_id: "demo-session-001".into(),
        permitted_channels: vec!["local-llm".into()],
        model_allowlist: vec!["sha256:demo-model".into()],
        decoder_version: None,
        stimulation_policy: None,
        context_merkle_bound: "f".repeat(64),
        forbidden_exports: vec!["cloud-telemetry".into(), "training-retention".into()],
        valid_until: now + Duration::minutes(5).num_seconds(),
        parent_cp_hash: "0".repeat(64),
        signature: None,
    };
    iac.sign(keys).expect("demo IAC sign");
    iac
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}
