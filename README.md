# The Membrane

The Membrane is a fail-closed authorization gateway for AI agents with production write access. Every model and tool call needs a live, signed, time-bounded scope; each action writes a tamper-evident receipt; broken continuity blocks or severs the agent.

The Membrane began with the concept of a firewall between human cognition and AI silicon, developed in the [cognitive boundary research](docs/whitepaper.md); the agent gate applies that boundary to model and tool calls.

**Public landing:** [membrane.dojopop.live](https://membrane.dojopop.live) · source in [`site/`](site/)

## Run it under your own control

Self-host the gate and hold your own keys. The Membrane checks authorization before an agent reaches your models, tools, or production systems, and keeps receipts for the actions that pass through it.

## Software and demo

The Rust workspace includes the authorization gate, CLI, receipt and attestation components, read-only operator dashboard, and deterministic recommendation advisor. The gate authenticates callers through caller-bound signed IACs and request proofs; operator-owned identity grants limit access and support revocation.

The repo also includes a demo of the operator workflow: authorization checks and receipt chaining. It uses ephemeral keys and an in-memory bus, and tool effects are simulated.

- **Run the software:** configure your operator registry, relay, signed IACs, caller keys and grants. Connect the gate to your model backend and supported tools. See [caller identity](docs/caller-identity.md) and the [GitHub connector](docs/github-connector.md).
- **Try the demo:** open [membrane-demo.dojopop.live](https://membrane-demo.dojopop.live) or run `membrane demo` locally.
- **Export telemetry:** use JSON Lines or OCSF-inspired SIEM export, with an optional fail-open webhook shipper. The demo does not send webhook traffic.

## Documents

| File | Description |
|------|-------------|
| [docs/product.md](docs/product.md) | Product overview and deployment |
| [docs/siem-export.md](docs/siem-export.md) | Vendor-neutral SIEM/SOC export (JSON Lines and OCSF-inspired JSON) |
| [docs/github-connector.md](docs/github-connector.md) | GitHub connector configuration |
| [site/](site/) | Public landing page ([membrane.dojopop.live](https://membrane.dojopop.live)) |
| [docs/demo.md](docs/demo.md) | Local product dashboard - one-command demo |
| [docs/whitepaper.md](docs/whitepaper.md) | Full specification (v0.9.14) - architecture & research |
| [docs/appendix-open-research.md](docs/appendix-open-research.md) | Open-source BCI stacks, security research, Phase 0 path |
| [docs/the-membrane-complete.md](docs/the-membrane-complete.md) | Single-file edition (whitepaper + Appendix B) |
| [docs/the-membrane-complete.pdf](docs/the-membrane-complete.pdf) | PDF export with table of contents |

Rebuild MD/PDF: `./scripts/build-paper.sh`

## Core idea

```text
 Agent / model traffic       THE MEMBRANE GATE       Production systems
 + proposed tool actions     (fail-closed authz)     (code, infra, data)
          │                           │                        │
          └── signed scope + TTL ─────┴── chained receipts ───┘
                                      │
                           block / sever on failure
```

Make the gate the required path for in-scope agents. A routed model or tool call proceeds only with a live signed authorization for its model, tools, task, and lifetime; each action links to the prior receipt so continuity failures are visible and enforceable. Observability explains after the fact; filters rewrite prompts; The Membrane **enforces** before production is touched.

## Worked example: gating agents that touch licensed content

Detection finds infringing or synthetic media after it spreads. The Membrane works one layer earlier: an agent that fetches, transforms, or republishes licensed assets passes through the gate first, and the gate fails closed when provenance is missing or unverifiable.

The setup: an agent drafts or edits content built from third-party assets (footage, stills, music, character IP). Each tool call - `content.fetch`, `content.transform`, `content.publish` - needs a live, signed, time-bounded IAC naming the asset and the permitted operation. At scope-issuance time, operator policy checks the asset's provenance:

- **C2PA-signed asset.** The asset carries a Content Credential: a signed manifest of assertions and claims about who created it and how it was edited. Policy verifies the claim signature and checks the signing credential was valid and unrevoked when the claim was signed.
- **Missing manifest, bad signature, revoked credential, untrusted signer.** No scope is issued. The call is blocked and the denial is written into the receipt chain. Missing provenance is a denial, not a warning.
- **Allowed calls.** Each one writes a tamper-evident CP receipt chained to the prior receipt, so the audit record shows which signed scope authorized which operation on which asset.

Provenance verification is operator policy evaluated when the IAC is issued. The repo ships no C2PA verifier today; adding one follows the connector-and-policy pattern in [docs/github-connector.md](docs/github-connector.md).

Detection after publication is always behind the leak. A fail-closed gate makes "unverified content never enters the pipeline" the default state.

## Requirements

- **Rust (stable) with Cargo.** Install with [rustup](https://rustup.rs): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`. Built and tested on stable Rust 1.99; older toolchains have not been verified.
- A C toolchain and `git` for building dependencies (`build-essential` on Debian/Ubuntu).
- Docker, only for the optional local Nostr relay in the operator full-stack path. The demo needs no Docker.

First build takes about a minute. Check your setup with `cargo test --workspace`.

## Local demo dashboard

No secrets, relay, or paid APIs needed. Open the public sandbox at [membrane-demo.dojopop.live](https://membrane-demo.dojopop.live), or run the same demo locally. Preview the landing site with `python3 -m http.server 8080 --directory site`.

```bash
cargo run -p membrane-cli -- demo
# open http://127.0.0.1:8790/
```

See [docs/demo.md](docs/demo.md) for the six-step flow. Demo HTTP routes live under `/demo/api/*` and are **not** enabled by `membrane gate start`.

The relay-backed operator test is `membrane iac-smoke`.

## Full stack (operators)

For the live gate, attestation bus, and session IAC path you need:

1. A local or operator-controlled relay
2. Your own `NOSTR_NSEC` (never commit)
3. An IAC **issued and signed by that same key** (`membrane iac issue` / `iac sign`)

The gate verifies the IAC against the signer pubkey. Bundled files such as `tools/demo-iac.json` only work when your `NOSTR_NSEC` matches the key that signed them - an arbitrary nsec will fail closed. Prefer issuing a fresh session IAC for your key.

1. **Local relay** (self-hosted bus - do not use public relays for writes):

```bash
docker run --rm -d --name membrane-relay -p 7777:8080 \
  -v "$PWD/tools/relay-local.toml:/usr/src/app/config.toml:ro" \
  scsibug/nostr-rs-relay:0.10.0
```

2. **Build:**

```bash
cargo build --release
```

3. **Set signing key** (never commit):

```bash
export NOSTR_NSEC='nsec1...'   # or: doppler run -- ...
export MEMBRANE_RELAY_URL='ws://127.0.0.1:7777'
```

4. **Commands:**

```bash
cargo run -- bus publish-test          # kind 31990 test event
cargo run -- bus subscribe             # fetch events + recompute bus_root
cargo run -- iac-smoke                 # technical fail-closed IAC/relay smoke test
cargo run -- gate start --iac <your-signed-iac.json>   # HTTP gate on :8787 → model API

# Session-scoped IAC (binds to current cp_chain head, short TTL)
cargo run -- iac issue --model qwen2.5-0.5b-instruct --ttl-secs 3600 --out session-iac.json
curl -s http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H "X-Membrane-IAC: $(cat session-iac.json)" \
  -d '{"model":"qwen2.5-0.5b-instruct","messages":[{"role":"user","content":"hi"}]}'

# Daily rollup (independent timestamping)
cargo run -- rollup export --day 2026-07-05 --out rollup.json
cargo run -- rollup sign --input rollup.json --out rollup.signed.json
cargo run -- rollup stamp --input rollup.signed.json --ots-out rollup.ots
```

**Model API:** point `model_api_url` in your channel registry at any allowed backend that speaks the chat/completions wire format (default example: `http://127.0.0.1:8080/v1/chat/completions`). The gate falls back to mock responses if the backend is unreachable.

**Gate HTTP:** `POST /v1/chat/completions` with a standard chat/completions JSON body. Pass a **session-scoped** IAC via `X-Membrane-IAC` header (JSON or base64 JSON). Each turn publishes `membrane.cp.router` with a context Merkle root and chains `parent_cp_hash` to the prior CP. Issue session IACs with `membrane iac issue` (binds `parent_cp_hash` to the current chain head). A static `--iac` file is only valid when signed by the same key the gate is running as.

**Tool invoke (GitHub connector):** `POST /v1/tools/invoke` after `membrane iac issue --tool github.comment`. Requires `MEMBRANE_GITHUB_TOKEN` (or `GITHUB_TOKEN`) and a non-empty `github_repo_allowlist` in the channel registry. Out-of-scope tools (for example `github.merge`) are blocked with a receipt before any GitHub HTTP. Helper: `membrane tools invoke …`. Full recipe: [docs/github-connector.md](docs/github-connector.md). Do not enable this on the public demo sandbox.

### Self-hosted session (local LLM with receipts)

```bash
# One-time setup
membrane init    # writes ~/.config/membrane/config.yaml
export NOSTR_NSEC='nsec1...'

# Interactive chat (auto-issues session IAC, prints CP receipt each turn)
membrane chat

# One-shot
membrane chat --message "summarize my threat model"

# Audit your chain
membrane session status
membrane session receipts --since-secs 86400

# Export standard SIEM telemetry (no signing key required)
membrane evidence export --format jsonl --since-secs 86400 --out membrane-siem.jsonl
membrane evidence export --format ocsf --since-secs 86400 --out membrane-siem.ocsf.json

# Sever active session (fail-closed; requires fresh IAC to resume)
membrane sever
membrane sever --scope-id session-1234567890
```

Each turn returns `X-Membrane-CP-Hash`, `X-Membrane-Session-Nonce`, and related headers from the gate. Session logs are saved under `~/.local/share/membrane/sessions/`.

**SIEM/SOC export:** signed bus events can be projected as vendor-neutral JSON
Lines or an explicitly OCSF-inspired JSON pack. This lets existing SOC, SIEM,
and SOAR tooling consume authorization-issued, allowed, blocked, sever, and
stale/degraded telemetry without making the Membrane a monitoring product or
claiming a vendor partnership. Set `MEMBRANE_SIEM_WEBHOOK_URL` to enable the
fail-open live webhook shipper (retries, optional dead-letter). See
[docs/siem-export.md](docs/siem-export.md).

**Severance:** `membrane sever` publishes `membrane.alert.degraded` with `reason: subject_sever`, removes the local active IAC, and blocks further chat on that scope until you issue a fresh IAC (`membrane iac issue`). The gate also runs a Δt watchdog (default 300s): if no `membrane.cp.router` arrives within Δt, it publishes `membrane.alert.degraded` and rejects chat fail-closed. Check staleness via `membrane session status` or `GET /health` (`delta_t_secs`, `last_cp_age_secs`).

**Tailnet example:**

```bash
membrane chat --gate-url http://relay-2:8787 \
  --relay-url ws://relay-2:7778 \
  --model qwen2.5-0.5b-instruct
```

### Layout

```text
schemas/              JSON Schema (MembraneEvent, IAC, RollupBundle, membrane.cp.router)
membrane-core/        Events, Merkle, attestation bus publisher/subscriber
membrane-gate/        IAC fail-closed gate + pluggable model API proxy
membrane-cli/         `membrane` binary
tools/                channel registry YAML, local relay config
```

**Stack:** Rust workspace (`membrane-core`, `membrane-gate`, `membrane-cli`). AGPL-3.0 for code.

### Git backup (GRASP / gitworkshop)

Self-hosted [ngit-grasp](https://ngit.dev/grasp/) mirrors this repo to Nostr git (syncs with [gitworkshop.dev](https://gitworkshop.dev)).

| | |
|---|---|
| Public | `https://membrane-grasp.dojopop.live` |
| Clone | `nostr://npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/membrane-grasp.dojopop.live/the-membrane` |
| Deploy / push | [`deploy/grasp/README.md`](deploy/grasp/README.md) |

GitHub is day-to-day; Grasp is the decentralized backup remote.

---

## Architecture & research

Protocol foundations, attestation bus details, BCI channel research and the zk roadmap are documented below.

**Foundations:** SHA-256 Merkle commitments, signed Chain Proof receipts, TEE attestation, and web-of-trust witnesses, with zk-STARK proofs on the roadmap. Optional daily [OpenTimestamps](https://opentimestamps.org/) rollups provide independently verifiable audit time. The same fail-closed boundary model can extend to local AI, cloud inference, BCI telemetry, and other exogenous channels without splitting the product.

**Phase 0 sketch (no invasive implant required):** OpenBCI or Muse → [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) → local TEE prover → optional local LLM session gate → self-hosted attestation bus → daily OTS rollup → fail closed on stale/missing attestation. See [appendix-open-research.md](docs/appendix-open-research.md).

### Attestation bus (Nostr kinds)

**Production relay:**

| | |
|---|---|
| Public | `wss://membrane-relay.dojopop.live` (after tunnel DNS - see `deploy/relay/`) |
| Kinds | 31990, 31991 only |
| Deploy | `./deploy/relay/deploy.sh` |

```bash
export MEMBRANE_RELAY_URL='wss://membrane-relay.dojopop.live'
```

| MembraneEvent.type | Nostr kind | tag `k` |
|--------------------|------------|---------|
| `membrane.cp.*`, `membrane.iac`, `membrane.anchor.ots` | 31990 | `the-membrane-*` |
| `membrane.alert.degraded`, `membrane.action.blocked` | 31991 | `the-membrane-alert-degraded`, `the-membrane-action-blocked` |

Common tags: `p` (subject pubkey), `e` (prior event id). Content is canonical `MembraneEvent` JSON (metadata only).

Do **not** use `relay.dojopop.live` for Membrane attestation - it allowlists DojoPop kinds only. Use the dedicated bus above.

## Status

The workspace includes the gate, caller-bound IACs and identity grants, CP receipt chain, `membrane chat` client, operator dashboard, recommendation advisor, SIEM export and daily OTS rollup CLI. The demo is a simulated approximation of the operator workflow. Configure supported connectors for external tool execution (see GitHub connector docs). Winterfell STARK and BCI integrations remain research work.

## License

- Documentation: [CC BY 4.0](LICENSE)
- Code: AGPL-3.0

## Author

Zorie R. Barber

## Local operator audit

A separate read-only Rust dashboard observes the running gate: authorization decisions,
matched rules, liveness, loaded registry policy and deny rate. Loopback only, no policy
editor or gate write path. See [local run instructions and limits](docs/operator-dashboard.md).
