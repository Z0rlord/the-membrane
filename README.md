# The Membrane

Research architecture for a **cognitive boundary** — a nervous-system firewall against AI routing, invasive BCI read/write paths, and non-invasive thought inference.


The Membrane does not read minds. It attests **which channels may cross the boundary** (local LLM, BCI telemetry, cloud inference), whether they match prior commitments, and **fails closed** when attestation breaks.

## Documents

| File | Description |
|------|-------------|
| [docs/whitepaper.md](docs/whitepaper.md) | Full specification (v0.9.14) |
| [docs/appendix-open-research.md](docs/appendix-open-research.md) | Open-source BCI stacks, security research, and Phase 0 prototype path |
| [docs/the-membrane-complete.md](docs/the-membrane-complete.md) | **Single-file edition** (whitepaper + Appendix B) |
| [docs/the-membrane-complete.pdf](docs/the-membrane-complete.pdf) | PDF export with table of contents |

Rebuild MD/PDF: `./scripts/build-paper.sh`

## Core idea

```text
 Endogenous cognition          THE MEMBRANE          Exogenous channels
 (nervous system)              (fail-closed gate)    (AI routers, BCI, sensors)
        │                              │                      │
        └────────── only attested ──────┴────── traffic ───────┘
```

**Threats:** AI routing (copilots/agents ingesting context), invasive neural channels (implants/BCIs), non-invasive inference (EEG, gaze, behavioral phenotyping).

**Mechanism:** zk-STARK Chain Proofs + SHA-256 Merkle commitments + TEE attestation + personal web-of-trust witnesses. Optional daily [OpenTimestamps](https://opentimestamps.org/) rollup for Bitcoin-backed audit time. No valid CP → sever the channel.

## Phase 0 (no invasive implant required)

OpenBCI or Muse → [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) → local TEE prover (channel + bus Merkle roots) → optional **local** LLM session gate → self-hosted **attestation bus** → daily **OTS** rollup on `cp_chain_root` → fail closed on stale/missing attestation.

See [appendix-open-research.md](docs/appendix-open-research.md) for libraries and papers.

## Phase 0 prototype — Nostr attestation bus

**Production relay (Hetzner Nuremberg):**

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
membrane-gate/        IAC fail-closed gate + llama.cpp HTTP proxy
membrane-cli/         `membrane` binary
tools/                channel registry YAML, local relay config
```

### Quick start

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
cargo run -- demo                      # fail-closed without IAC → OK with IAC
cargo run -- gate start                # HTTP gate on :8787 → llama.cpp :8080

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

**llama.cpp:** run `llama-server` with OpenAI-compatible API (default `http://127.0.0.1:8080/v1/chat/completions`). The gate falls back to mock responses if llama.cpp is unreachable.

**Gate HTTP:** `POST /v1/chat/completions` with OpenAI chat body. Pass a **session-scoped** IAC via `X-Membrane-IAC` header (JSON or base64 JSON). Each turn publishes `membrane.cp.router` with a context Merkle root and chains `parent_cp_hash` to the prior CP. Issue session IACs with `membrane iac issue` (binds `parent_cp_hash` to the current chain head). A static `--iac` default is for dev only when the chain is at genesis.

### Nostr mapping (Appendix B)

| MembraneEvent.type | Nostr kind | tag `k` |
|--------------------|------------|---------|
| `membrane.cp.*`, `membrane.iac`, `membrane.anchor.ots` | 31990 | `the-membrane-*` |
| `membrane.alert.degraded` | 31991 | `the-membrane-alert-degraded` |

Common tags: `p` (subject pubkey), `e` (prior event id). Content is canonical `MembraneEvent` JSON (metadata only).

### dojopop relay (legacy note)

Do **not** use `relay.dojopop.live` for Membrane attestation — it allowlists DojoPop kinds only.
Use the dedicated bus above. For integration smoke tests against DojoPop infra, you would need
to add kinds 31990/31991 to that relay separately (not recommended).

## Status

Phase 0 foundation: schemas, Merkle helper, Nostr bus, session-scoped IAC + router CP chain on the gate (HTTP + llama.cpp), daily OTS rollup CLI. No Winterfell STARK or BCI integration yet.

## License

- Documentation: [CC BY 4.0](LICENSE)
- Future reference code (when added): AGPL-3.0

## Author

Zorie R. Barber
