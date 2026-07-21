# Membrane landing demo

Browser dashboard for The Membrane fail-closed narrative, backed by real Membrane gate IAC checks and CP receipt chaining.

## One command

```bash
cargo run -p membrane-cli -- landing-demo
```

Open **http://127.0.0.1:8790/**.

Optional listen address: `cargo run -p membrane-cli -- landing-demo --listen 127.0.0.1:8790`

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
6. **Evidence** — export JSON pack and verify the receipt chain (pass/fail)

## Honest scope

The Membrane enforces and proves traffic routed through the gateway. This demo does not claim hidden reasoning, deletion, off-gateway coverage, or compliance guarantees.

## Tests

```bash
cargo test -p membrane-gate demo
cargo test -p membrane-core
```
