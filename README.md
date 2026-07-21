# The Membrane

A fail-closed control point for AI agents with production access — and the broader research architecture for a subject-controlled cognitive boundary.

**Public landing:** [membrane.dojopop.live](https://membrane.dojopop.live) · source in [`site/`](site/)

For model and tool traffic routed through the gateway, live signed authorizations bind the allowed model, tools, and action scope. Allowed actions extend a tamper-evident, hash-linked receipt chain; missing, expired, out-of-scope, or discontinuous authorization blocks the action and can sever the agent.

The broader Membrane research program remains a **cognitive boundary**: a nervous-system firewall for governing which external channels may cross a subject-controlled boundary, whether they match prior commitments, and when they must fail closed.

## Documents

| File | Description |
|------|-------------|
| [docs/product.md](docs/product.md) | **The Membrane** — enterprise product positioning (agent integrity gateway) |
| [docs/siem-export.md](docs/siem-export.md) | Vendor-neutral SIEM/SOC export (JSON Lines and OCSF-inspired JSON) |
| [site/](site/) | Public landing page ([membrane.dojopop.live](https://membrane.dojopop.live)) |
| [docs/demo.md](docs/demo.md) | Local product dashboard — one-command demo |
| [docs/whitepaper.md](docs/whitepaper.md) | Full specification (v0.9.14) |
| [docs/appendix-open-research.md](docs/appendix-open-research.md) | Open-source BCI stacks, security research, and Phase 0 prototype path |
| [docs/the-membrane-complete.md](docs/the-membrane-complete.md) | **Single-file edition** (whitepaper + Appendix B) |
| [docs/the-membrane-complete.pdf](docs/the-membrane-complete.pdf) | PDF export with table of contents |

Rebuild MD/PDF: `./scripts/build-paper.sh`

## Core idea

```text
 Agent / model traffic       THE MEMBRANE GATE       Production systems
 + proposed tool actions     (fail-closed control)   (code, infra, data)
          │                           │                        │
          └── signed policy + scope ──┴── chained receipts ───┘
                                      │
                           block / sever on failure
```

**Product wedge:** Make the gate the required path for in-scope agents. A routed model or tool call proceeds only with a live signed authorization for its model, tools, task, and lifetime; each action links to the prior receipt so continuity failures are visible and enforceable.

**Protocol and research foundation:** SHA-256 Merkle commitments, signed Chain Proof receipts, TEE attestation, and web-of-trust witnesses, with zk-STARK proofs on the roadmap. Optional daily [OpenTimestamps](https://opentimestamps.org/) rollups provide independently verifiable audit time. The same fail-closed boundary model extends to local AI, cloud inference, BCI telemetry, and other exogenous channels without changing the sovereignty thesis.

**Scope:** Membrane enforces and provides evidence for traffic routed through its gate. It does not prove hidden reasoning, prove deletion, observe off-gateway activity, read minds, or guarantee regulatory compliance.

## Phase 0 (no invasive implant required)

OpenBCI or Muse → [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) → local TEE prover (channel + bus Merkle roots) → optional **local** LLM session gate → self-hosted **attestation bus** → daily **OTS** rollup on `cp_chain_root` → fail closed on stale/missing attestation.

See [appendix-open-research.md](docs/appendix-open-research.md) for libraries and papers.

## Phase 0 prototype — Nostr attestation bus

**Production relay:**

| | |
|---|---|
| Tailnet | `ws://relay-2:7778` |
| Public | `wss://membrane-relay.dojopop.live` (after tunnel DNS — see `deploy/relay/`) |
| Kinds | 31990, 31991 only |
| Deploy | `./deploy/relay/deploy.sh` |

```bash
export MEMBRANE_RELAY_URL='wss://membrane-relay.dojopop.live'
# or tailnet-only:
export MEMBRANE_RELAY_URL='ws://relay-2:7778'
```

**Stack choice:** Rust workspace (`membrane-core`, `membrane-gate`, `membrane-cli`) — aligns with Winterfell STARK path (Phase 1) and Ed25519/Nostr signing. AGPL-3.0 for code.

### Layout

```text
schemas/              JSON Schema (MembraneEvent, IAC, RollupBundle, membrane.cp.router)
membrane-core/        Events, Merkle (§5.1), Nostr bus publisher/subscriber
membrane-gate/        IAC fail-closed gate + pluggable model API proxy
membrane-cli/         `membrane` binary
tools/                channel registry YAML, local relay config
```

### Local demo dashboard

**Primary path for anyone cloning the repo** — no secrets, no relay, no paid APIs.

Public marketing site (static): **[membrane.dojopop.live](https://membrane.dojopop.live)** — source in [`site/`](site/). Preview locally with `python3 -m http.server 8080 --directory site`.

Open the isolated, simulation-only public sandbox at
**[membrane-demo.dojopop.live](https://membrane-demo.dojopop.live)**, or run
the same browser demo locally. Both paths use ephemeral keys and an in-memory
bus; no production gate, relay, credentials, or tool integrations are used.

```bash
cargo run -p membrane-cli -- demo
# open http://127.0.0.1:8790/
```

See [docs/demo.md](docs/demo.md) for the six-step flow. Demo HTTP routes live under `/demo/api/*` and are **not** enabled by `membrane gate start`.

`membrane demo` is the single public demo command. The old `membrane landing-demo` spelling remains a hidden, deprecated alias. The relay-backed operator test moved to `membrane iac-smoke`.

### Full stack (operators)

For the live gate, attestation bus, and session IAC path you need:

1. A local or operator-controlled relay
2. Your own `NOSTR_NSEC` (never commit)
3. An IAC **issued and signed by that same key** (`membrane iac issue` / `iac sign`)

The gate verifies the IAC against the signer pubkey. Bundled files such as `tools/demo-iac.json` only work when your `NOSTR_NSEC` matches the key that signed them — an arbitrary nsec will fail closed. Prefer issuing a fresh session IAC for your key.

1. **Local relay** (self-hosted bus per Appendix B — do not use public relays for writes):

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

# Daily rollup (Appendix B Cold C)
cargo run -- rollup export --day 2026-07-05 --out rollup.json
cargo run -- rollup sign --input rollup.json --out rollup.signed.json
cargo run -- rollup stamp --input rollup.signed.json --ots-out rollup.ots
```

**Model API:** point `model_api_url` in your channel registry at any allowed backend that speaks the chat/completions wire format (default example: `http://127.0.0.1:8080/v1/chat/completions`). The gate falls back to mock responses if the backend is unreachable.

**Gate HTTP:** `POST /v1/chat/completions` with a standard chat/completions JSON body. Pass a **session-scoped** IAC via `X-Membrane-IAC` header (JSON or base64 JSON). Each turn publishes `membrane.cp.router` with a context Merkle root and chains `parent_cp_hash` to the prior CP. Issue session IACs with `membrane iac issue` (binds `parent_cp_hash` to the current chain head). A static `--iac` file is only valid when signed by the same key the gate is running as.

### Sovereign session (local LLM with receipts)

For users who want **local inference with an audit trail** — no curl, no manual IAC headers:

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
claiming a vendor partnership. See [docs/siem-export.md](docs/siem-export.md).

**Severance:** `membrane sever` publishes `membrane.alert.degraded` with `reason: subject_sever`, removes the local active IAC, and blocks further chat on that scope until you issue a fresh IAC (`membrane iac issue`). The gate also runs a Δt watchdog (default 300s): if no `membrane.cp.router` arrives within Δt, it publishes `membrane.alert.degraded` and rejects chat fail-closed. Check staleness via `membrane session status` or `GET /health` (`delta_t_secs`, `last_cp_age_secs`).

**Tailnet example (relay-2):**

```bash
membrane chat --gate-url http://relay-2:8787 \
  --relay-url ws://relay-2:7778 \
  --model qwen2.5-0.5b-instruct
```

### Nostr mapping (Appendix B)

| MembraneEvent.type | Nostr kind | tag `k` |
|--------------------|------------|---------|
| `membrane.cp.*`, `membrane.iac`, `membrane.anchor.ots` | 31990 | `the-membrane-*` |
| `membrane.alert.degraded`, `membrane.action.blocked` | 31991 | `the-membrane-alert-degraded`, `the-membrane-action-blocked` |

Common tags: `p` (subject pubkey), `e` (prior event id). Content is canonical `MembraneEvent` JSON (metadata only).

### dojopop relay (legacy note)

Do **not** use `relay.dojopop.live` for Membrane attestation — it allowlists DojoPop kinds only.
Use the dedicated bus above. For integration smoke tests against DojoPop infra, you would need
to add kinds 31990/31991 to that relay separately (not recommended).

### Git backup (GRASP / gitworkshop)

Self-hosted [ngit-grasp](https://ngit.dev/grasp/) on relay-2 mirrors this repo to Nostr git (syncs with [gitworkshop.dev](https://gitworkshop.dev)).

| | |
|---|---|
| Public | `https://membrane-grasp.dojopop.live` |
| Tailnet | `http://relay-2:7334` |
| Clone | `nostr://npub1ddyhkk6w993rcctxc0c3fnacx3xqrffk53d0sn7af3g895sg80fqa9hza9/membrane-grasp.dojopop.live/the-membrane` |
| Deploy / push | [`deploy/grasp/README.md`](deploy/grasp/README.md) |

GitHub remains `origin` for day-to-day work; `grasp` (nostr) is the decentralized backup remote after `ngit init`.

## Status

Phase 0 foundation: schemas, Merkle helper, Nostr bus, session-scoped IAC + router CP chain on the gate (HTTP + pluggable model API), sovereign `membrane chat` client, daily OTS rollup CLI. No Winterfell STARK or BCI integration yet.

## License

- Documentation: [CC BY 4.0](LICENSE)
- Future reference code (when added): AGPL-3.0

## Author

Zorie R. Barber
