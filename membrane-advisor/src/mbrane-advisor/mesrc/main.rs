//! Read-only advisor CLI. Reads a snapshot and a registry, prints a report or a patch to
//! stdout. It has no write path: it never opens the registry for writing and never talks
//! to the gate. Feed it with `curl -s http://127.0.0.1:8788/audit | membrane-advisor ...`.
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use membrane_advisor::{analyze, render_text};
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
    let report = analyze(&snapshot, &registry_text, &args.registry, args.min_denials)?;
    match args.format {
        Format::Text => print!("{}", render_text(&report)),
        Format::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        Format::Patch => print!("{}", report.patch),
    }
    Ok(())
}
