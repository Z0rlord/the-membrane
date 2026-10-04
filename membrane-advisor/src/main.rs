//! Read-only advisor CLI. Reads a snapshot and a registry, prints a report or a patch to
//! stdout. It has no write path: it never opens the registry for writing and never talks
//! to the gate. Feed it with `curl -s http://127.0.0.1:8788/audit | membrane-advisor ...`.
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use membrane_advisor::seam::{from_config, Config, ReqwestTransport};
use membrane_advisor::{analyze, annotate, render_text};
use membrane_gate::audit::Snapshot;
use std::io::Read;

#[derive(Clone, ValueEnum)]
enum Format {
    Text,
    Json,
    /// Only the unified diff, suitable for `git apply`.
    Patch,
}

#[derive(Parser)]
#[command(about = "Deterministic, read-only policy recommendations from gate denials")]
struct Args {
    /// Snapshot JSON from the gate's loopback /audit endpoint; `-` reads stdin.
    #[arg(long, default_value = "-")]
    snapshot: String,
    /// Channel registry YAML the gate runs with. Read only.
    #[arg(long)]
    registry: String,
    /// Ignore denial groups seen fewer times than this.
    #[arg(long, default_value_t = 1)]
    min_denials: usize,
    /// Optional JSON file selecting a decision backend (clef_local, clef_workers_ai,
    /// jev_api). Adds advisory triage notes; the recommendations and patch are unchanged.
    #[arg(long)]
    model_config: Option<String>,
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let raw = if args.snapshot == "-" {
        let mut s = String::new();
        std::io::stdin().take(4_000_000).read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(&args.snapshot).with_context(|| args.snapshot.clone())?
    };
    let snapshot: Snapshot = serde_json::from_str(&raw).context("parsing audit snapshot")?;
    let registry_text =
        std::fs::read_to_string(&args.registry).with_context(|| args.registry.clone())?;
    let mut report = analyze(&snapshot, &registry_text, &args.registry, args.min_denials)?;
    if let Some(path) = &args.model_config {
        let cfg: Config =
            serde_json::from_str(&std::fs::read_to_string(path).with_context(|| path.clone())?)
                .context("parsing model config")?;
        let backend = from_config(&cfg, ReqwestTransport)?;
        annotate(&mut report, &backend);
    }
    match args.format {
        Format::Text => print!("{}", render_text(&report)),
        Format::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        Format::Patch => print!("{}", report.patch),
    }
    Ok(())
}
