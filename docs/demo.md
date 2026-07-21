# Membrane demo dashboard

Browser dashboard for The Membrane fail-closed narrative, backed by real Membrane gate IAC checks and CP receipt chaining.

This is the **primary public demo path** — no secrets, relay setup, or external
tool access. For the operator full stack (relay + gate + your signed IAC), see
[README § Full stack (operators)](../README.md#full-stack-operators). Repo
overview: [README § Local demo dashboard](../README.md#local-demo-dashboard).

## Public sandbox

Open **https://membrane-demo.dojopop.live**.

The hosted dashboard runs the real gate authorization and receipt-chain logic,
but all Jira, Slack, and GitHub actions are local simulations. It has no
production credentials or production network access. State is isolated by an
opaque browser session, expires after 30 minutes, and is also cleared on
restart. Do not enter secrets, customer data, or other sensitive information.

## Run locally with one command

```bash
cargo run -p membrane-cli -- demo
```

Open **http://127.0.0.1:8790/**.

Optional listen address: `cargo run -p membrane-cli -- demo --listen 127.0.0.1:8790`

If the `membrane` binary is installed, the equivalent command is `membrane demo`.

## What it is

- Ephemeral local signing keys (no `NOSTR_NSEC`, no Doppler required)
- In-memory attestation bus (`memory://`) — no relay or paid APIs
- Demo-only HTTP routes under `/demo/api/*` (not mounted by `membrane gate start`)
- Tool calls (`jira.comment`, `slack.post`, `github.merge`) are **simulated** and labeled as such

## Six-step demo flow

1. **Issue authorization** — 15-minute signed IAC for `support-agent-v1` with tools `jira.comment` and `slack.post`
2. **Allowed action** — run `jira.comment`; green chained receipt (CP hash linked to parent)
3. **Blocked tool** — attempt `github.merge`; hard block with reason
4. **Blocked model swap** — same tool with `unrestricted-agent-v9`; hard block
5. **Sever** — sever session, then retry `slack.post`; fails closed
6. **Evidence** — export JSON evidence or SIEM-ready OCSF-inspired/JSON Lines records, then verify the receipt chain (pass/fail)

Demo SIEM API:

```bash
curl -fsS 'http://127.0.0.1:8790/demo/api/siem?format=ocsf'
curl -fsS 'http://127.0.0.1:8790/demo/api/siem?format=jsonl'
```

Both remain simulation-only and omit action bodies and credentials. See
[siem-export.md](siem-export.md) for operator ingestion patterns.

## Technical IAC smoke test

The older relay-backed IAC smoke test remains available to operators as `membrane iac-smoke`. It requires a relay and your signing key and is not the public product demo.

## Honest scope

The Membrane enforces and proves traffic routed through the gateway. This demo does not claim hidden reasoning, deletion, off-gateway coverage, or compliance guarantees.

## Tests

```bash
cargo test -p membrane-cli cli_tests
cargo test -p membrane-gate demo
cargo test -p membrane-core
```
