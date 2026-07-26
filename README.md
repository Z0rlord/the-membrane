# The Membrane

The Membrane is a fail-closed authorization gateway for AI agents with production write access. Every model and tool call needs a live, signed, time-bounded scope; each action writes a tamper-evident receipt; broken continuity blocks or severs the agent.

**Public landing:** [membrane.dojopop.live](https://membrane.dojopop.live) · source in [`site/`](site/)

## Who it’s for

**Sovereigns · Nation-states · Enterprise** — one product, three customer classes.

A fail-closed authorization gateway for AI agents with production or operational write access. Customers self-host the gate, hold their own keys, and prove agent actions with live signed scopes and tamper-evident receipts. High-assurance and mil/gov postures sit with sovereign and nation-state operators; enterprise runs the same gate for production agent write access under its own control.

## What’s real vs demo

| Real today | Demo / sandbox |
|------------|----------------|
| Fail-closed gate checks, signed time-bounded scopes (IAC), tamper-evident CP receipt chain, block and sever | Hosted [membrane-demo.dojopop.live](https://membrane-demo.dojopop.live) and local `membrane demo` — same gate logic, **simulated** Jira / Slack / GitHub tool side effects |
| Operator SIEM export (JSON Lines / OCSF-inspired) and optional fail-open webhook | Public sandbox does not ship webhook traffic or hold production credentials |
| Production gate tool path when a real connector is configured (see [docs/github-connector.md](docs/github-connector.md)) | Without a connector, tool invokes stay simulated |

## How we sell / stage

Self-hosted pilots in your environment, then license and support. No hosted production SaaS.

## Documents

| File | Description |
|------|-------------|
| [docs/product.md](docs/product.md) | Product positioning (sovereigns, nation-states, enterprise) |
| [docs/siem-export.md](docs/siem-export.md) | Vendor-neutral SIEM/SOC export (JSON Lines and OCSF-inspired JSON) |
| [docs/github-connector.md](docs/github-connector.md) | Real GitHub tool path on the production gate |
| [site/](site/) | Public landing page ([membrane.dojopop.live](https://membrane.dojopop.live)) |
| [docs/demo.md](docs/demo.md) | Local product dashboard — one-command demo |
| [docs/whitepaper.md](docs/whitepaper.md) | Full specification (v0.9.14) — architecture & research |
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

## Local demo dashboard

**Primary path for anyone cloning the repo** — no secrets, no relay, no paid APIs.

Public marketing site (static): **[membrane.dojopop.live](https://membrane.dojopop.live)** — source in [`site/`](site/). Preview locally with `python3 -m http.server 8080 --directory site`.

Open the isolated public sandbox at
**[membrane-demo.dojopop.live](https://membrane-demo.dojopop.live)**, or run
the same browser demo locally. Both paths use ephemeral keys and an in-memory
bus; tool side effects are simulated. No production credentials.

```bash
cargo run -p membrane-cli -- demo
# open http://127.0.0.1:8790/
```

See [docs/demo.md](docs/demo.md) for the six-step flow. Demo HTTP routes live under `/demo/api/*` and are **not** enabled by `membrane gate start`.

`membrane demo` is the single public demo command. The old `membrane landing-demo` spelling remains a hidden, deprecated alias. The relay-backed operator test moved to `membrane iac-smoke`.

## Full stack (operators)

For the live gate, attestation bus, and session IAC path you need:

1. A local or operator-controlled relay
2. Your own `NOSTR_NSEC` (never commit)
3. An IAC **issued and signed by that same key** (`membrane iac issue` / `iac sign`)

The gate verifies the IAC against the signer pubkey. Bundled files such as `tools/demo-iac.json` only work when your `NOSTR_NSEC` matches the key that signed them — an arbitrary nsec will fail closed. Prefer issuing a fresh session IAC for your key.

1. **Local relay** (self-hosted bus — do not use public relays for writes):

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

**Tool invoke (real GitHub connector):** `POST /v1/tools/invoke` after `membrane iac issue --tool github.comment`. Requires `MEMBRANE_GITHUB_TOKEN` (or `GITHUB_TOKEN`) and a non-empty `github_repo_allowlist` in the channel registry. Out-of-scope tools (for example `github.merge`) are blocked with a receipt before any GitHub HTTP. Helper: `membrane tools invoke …`. Full recipe: [docs/github-connector.md](docs/github-connector.md). Do not enable this on the public demo sandbox.

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
membrane sever --scope-id sovereign-1234567890
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

Protocol foundations, attestation bus details, BCI channel research, and zk roadmap live here so cold readers meet the **product** first. None of this changes the sovereign / nation-state / enterprise customer framing above.

**Foundations:** SHA-256 Merkle commitments, signed Chain Proof receipts, TEE attestation, and web-of-trust witnesses, with zk-STARK proofs on the roadmap. Optional daily [OpenTimestamps](https://opentimestamps.org/) rollups provide independently verifiable audit time. The same fail-closed boundary model can extend to local AI, cloud inference, BCI telemetry, and other exogenous channels without splitting the product.

**Phase 0 sketch (no invasive implant required):** OpenBCI or Muse → [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) → local TEE prover → optional local LLM session gate → self-hosted attestation bus → daily OTS rollup → fail closed on stale/missing attestation. See [appendix-open-research.md](docs/appendix-open-research.md).

### Attestation bus (Nostr kinds)

**Production relay:**

| | |
|---|---|
| Public | `wss://membrane-relay.dojopop.live` (after tunnel DNS — see `deploy/relay/`) |
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

Do **not** use `relay.dojopop.live` for Membrane attestation — it allowlists DojoPop kinds only. Use the dedicated bus above.

## Status

Gate, session-scoped IAC, CP receipt chain, sovereign/`membrane chat` client, SIEM export, and daily OTS rollup CLI are real. Hosted sandbox tools remain simulated; production connectors are opt-in (see GitHub connector docs). No Winterfell STARK or BCI integration yet.

## License

- Documentation: [CC BY 4.0](LICENSE)
- Code: AGPL-3.0

## Author

Zorie R. Barber
