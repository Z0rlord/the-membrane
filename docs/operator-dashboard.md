# Local operator audit dashboard

This is an observational surface, not another way to operate the gate. The
`membrane-dashboard` binary is a separate Rust/Axum process with no signing keys,
connector credentials, policy editor, or write endpoints. Static assets are bundled;
no CDN, framework build, external fonts or analytics are used.

## Run

Start the gate with your existing operator registry, signed IAC and relay setup:

```sh
cargo run -p membrane-cli -- gate start --registry tools/channel-registry.example.yaml --iac path/to/operator-iac.json
```

The gate starts a dedicated GET-only audit listener on `127.0.0.1:8788`, separate
from the production gate HTTP router. Set `MEMBRANE_AUDIT_LISTEN` to a literal
loopback socket to change it. A non-loopback address or bind failure stops startup.
Do not expose this listener with a reverse proxy.

In another terminal:

```sh
cargo run -p membrane-dashboard
# Open http://127.0.0.1:8790
# Optional, still loopback only:
cargo run -p membrane-dashboard -- --listen 127.0.0.1:8791 --audit-url http://127.0.0.1:8788
```

## What the page means

- **Decision log**: newest 500 authorization decisions in the running gate process,
  allow/deny, stable matched rule code, UTC Unix timestamp displayed in browser local
  time, action class, signing gate identity and verified scope when available.
  The current IAC format does not authenticate a caller-specific agent ID; the page
  does not pretend an untrusted header is one. A denied scope is shown as unverified.
- **Liveness**: idle (no active scope), live, degraded (severed/stale checkpoint),
  or unknown when telemetry cannot be verified. It is not a claim that every scope
  is authorized. Polling every 10 seconds while visible; offline/stale responses clear
  cached status and metrics. Checkpoints can become stale between polls.
- **Loaded policy**: the running registry's channels, model/repository allowlists,
  forbidden exports and checkpoint freshness threshold, without the model API URL.
  Per-request signed IAC constraints still apply and are not editable here.
- **Deny rate**: denied / (allowed + denied) over the retained 500 rows, not an all-time
  or time-window metric. No observations means N/A. Total observed resets on restart.

An allow is recorded immediately before the upstream model/tool call, after the
relevant preflight authorization checks, not inferred from a successful receipt.
An upstream execution or post-action receipt failure does not retroactively turn
that allow into a policy denial. Those failures remain on the existing receipt/SIEM
paths. Invalid JSON handler requests are recorded as request/registry failures.
Other routing, HTTP transport and body-limit rejections are not authorization decisions.

## Security and limits

Both listeners only accept loopback IP bind addresses, reject foreign Host/Origin
headers and send no CORS permission. The dashboard uses only GET requests to the
audit source, refuses redirects, bypasses proxies and bounds response size/time.
Assets have a same-origin CSP, no-store and nosniff headers. DOM content uses
`textContent`, never interpreted HTML. There is no connection to gate write routes.
Local users/processes are inside this trust boundary; loopback is not authentication.
Use OS account isolation for multi-user hosts. Do not forward either port publicly.

This bounded memory log is not durable, cryptographically signed, complete historical
SIEM export, or a substitute for the chained bus receipts. It resets on gate restart.
Telemetry lock failures show unknown and never grant an action. Existing gate policy,
receipt enforcement and SIEM behavior are otherwise unchanged.

## Verify

```sh
cargo test --workspace
cargo clippy -p membrane-dashboard --all-targets
```

For a local visual fixture only, without relay setup or tool execution:

```sh
cargo run -p membrane-gate --example audit-preview
# In another terminal, run membrane-dashboard as above.
```

The example emits three synthetic decisions and an idle policy view. It must not be
used to represent a production gate. Stop the example before starting the operator gate.
