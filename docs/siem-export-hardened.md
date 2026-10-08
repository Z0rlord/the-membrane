# Hardened SIEM export

The gate can ship every receipt and denial to your security stack over a
bounded, durable, TLS-pinned channel. This is telemetry out. It never changes an
authorization decision, except that you may choose to make the gate refuse new
work when it cannot record its audit trail (`fail_closed`).

Targets: Splunk HEC, Datadog logs intake, and a generic HTTPS endpoint that
receives newline-delimited JSON. The older env-var webhook shipper
(`MEMBRANE_SIEM_WEBHOOK_*`, see [siem-export.md](siem-export.md)) stays for local
demos. It is fail-open, unpinned and unspooled. The gate refuses to start if
both it and the hardened export are configured.

## Behaviour

- **Absent config disables export.** The gate starts as before.
- **Invalid config stops startup.** That covers parse errors, unknown keys, out of
  range limits, non-HTTPS URLs, unreadable trust anchors, a missing or empty token
  variable and a spool that fails verification. There is no silent fallback.
- **Bounded.** The spool holds at most `max_buffered_events` unacknowledged events
  (plus one reserved gap marker). Batches are capped by count and bytes. There is
  one delivery task and no per-event task.
- **Durable and ordered.** Each event is appended and fsynced to an on-disk spool
  before the caller moves on, in the order the gate produced it. Delivery is in
  that order and at-least-once. After a restart unacknowledged events are resent.
- **Retry with backoff.** Network errors, timeouts, 5xx and 429 retry with
  exponential backoff, jitter and a cap (`backoff_max_secs`). A rejection such as
  401 or 403 is also retried at the capped rate and never discarded, because it
  usually means a bad token that an operator can fix. HTTP 413 halves the batch.
- **No redirects.** Redirect responses are failures.

## Failure modes

`failure_mode` is required. There is no default.

| Mode | When the SIEM cannot keep up |
| --- | --- |
| `fail_closed` | Chat and tool requests are refused with HTTP 503 before any upstream effect. Triggers: buffer within 10% (at least 10 slots) of full, oldest unacknowledged event older than `fail_closed_lag_secs`, a spool write failure, or an open overflow gap. Admission returns when the backlog drains. |
| `degrade` | The gate keeps authorizing. When the buffer is full the newest events are not queued. A `gap` envelope with `phase: started` is spooled at once, and a `gap` envelope with `phase: ended` and the dropped count is spooled when space frees. `/health` reports `degraded`. |

`fail_closed` does not stop an event that is already in flight. Receipts are
produced first and queued after, and the headroom above exists so queuing succeeds
for requests already admitted. Admission is checked after the IAC validates and
before any upstream call. The operator-IAC exchange and the loopback audit
listener are not gated.

## Configure

Create an admin-owned YAML file and point the gate at it:

```yaml
failure_mode: fail_closed
target:
  kind: splunk_hec
  url: https://splunk.example.com:8088/services/collector/event
  token_env: MEMBRANE_SPLUNK_HEC_TOKEN
  index: security
tls:
  ca_bundle_file: /etc/membrane/splunk-ca.pem
  pinned_cert_sha256:
    - 3b5c...64 hex characters...
max_buffered_events: 10000
```

```sh
export MEMBRANE_SIEM_EXPORT_CONFIG=/etc/membrane/siem-export.yaml
export MEMBRANE_SPLUNK_HEC_TOKEN=...   # from your secret manager
membrane gate start ...
```

Other targets:

```yaml
target: {kind: datadog_logs, url: "https://http-intake.logs.datadoghq.com/api/v2/logs",
         token_env: MEMBRANE_DD_API_KEY, service: membrane, tags: "env:prod"}
target: {kind: webhook, url: "https://siem.example.com/ingest",
         token_env: MEMBRANE_SIEM_TOKEN}   # sent as "Authorization: Bearer ..."
```

For `webhook`, `token_env` is optional and `auth_header` names a different header
that carries the bare token.

| Key | Default | Range |
| --- | --- | --- |
| `max_buffered_events` | 10000 | 100..1000000 |
| `batch_max_events` | 100 | 1..1000, not above the buffer |
| `batch_max_bytes` | 1 MiB | 4 KiB..4 MiB (approximate; includes per-event overhead) |
| `flush_interval_secs` | 5 | 1..300 |
| `request_timeout_secs` | 10 | 1..60 |
| `backoff_initial_ms` / `backoff_max_secs` | 500 / 60 | 50..60000 / 1..3600 |
| `fail_closed_lag_secs` | 300 | 10..86400 |
| `spool_dir` | `$MEMBRANE_OPERATION_DIR/siem-export` | - |

Unknown keys are rejected. A key named `token` is an unknown key on purpose:
credentials come only from the environment variable named in `token_env`.

## Transport security

- HTTPS only. URLs with credentials, a query string or a fragment are rejected.
- `ca_bundle_file` is required. It is the **only** trust anchor set. Public roots
  built into the host are not used. For Splunk Cloud or Datadog, supply the CA
  chain that signs the endpoint you use.
- `pinned_cert_sha256` is optional. Each entry is the lowercase hex SHA-256 of a
  leaf certificate's DER encoding. When present, the leaf must match one entry in
  addition to passing chain and hostname validation. A leaf pin breaks on every
  certificate renewal. Add the next certificate's fingerprint before rotating, or
  pin only the CA bundle.
- Compute a fingerprint with
  `openssl x509 -in leaf.pem -outform DER | openssl dgst -sha256`.
- Pinning is covered by tests that perform real TLS handshakes: wrong anchor,
  wrong hostname and wrong pin all fail.

## No secrets in logs

Tokens are held in redacted values and sent in headers marked sensitive. Logs
carry the endpoint host, counts, attempt numbers and an HTTP status or one of
three fixed error strings. They never carry the URL path, headers, token or
event bodies. `/health` exposes `last_error` with the same fixed strings.

## What is exported

Each record is an envelope around the existing digest-only `SiemEvent`
(identifiers, allowlisted model and tool names, policy and receipt digests,
outcome, reason). Prompts, action bodies, keys, credentials and signatures are
not included.

```json
{"seq": 41, "prev": "<hash of seq 40>", "ts": 1791450000, "kind": "event",
 "event": { "...": "SiemEvent" }, "hash": "<hash>"}
```

`hash` is the hex SHA-256 of the JSON array `[seq, prev, ts, kind, event, gap]`
as serialized by the gate. Verify in a script or in the SIEM that `seq` rises by
one, each `prev` equals the previous `hash`, and each `hash` recomputes. A break
means events were lost, reordered or altered between the gate and the SIEM. Each
event still carries `receipt_hash` and `parent_receipt_hash`, so the signed
receipt chain stays checkable on its own.

Delivery is at-least-once. After a crash or lost response the same `seq` can
arrive twice. Deduplicate on `seq` and `hash`.

Splunk receives HEC objects (`time`, `source` default `membrane`, `sourcetype`
default `membrane:receipt`, optional `index` and `host`) whose `event` is the
envelope. Datadog receives a JSON array where the envelope is under `membrane`.
The webhook receives one envelope per line (`application/x-ndjson`).

## Operations

`GET /health` includes `siem_export`: `state` (`ok` or `degraded`), `pending`,
`capacity`, `oldest_pending_age_secs`, `consecutive_failures`, `dropped_total`,
`delivered_total` and `last_error`. Alert on `degraded`, on `pending` approaching
`capacity` and on any `gap` envelope in the SIEM.

The spool (`spool.jsonl`) and `cursor.json` are created with mode 0600 in
`spool_dir`. The spool is verified on startup. A torn tail, a broken chain or a
cursor that does not match stops the gate. There is no automatic repair. Inspect
the files and move them aside deliberately if you accept the loss. Keep the
directory on the same persistent volume as the operation journal.

## Limits

- One gate instance per spool. Unlike the operation journal it has no file lock,
  so do not point two gates at one directory.
- The export chain is its own chain. It proves what the gate queued and the order
  it queued it in. It is not anchored to journal frames, and checksums are not a
  keyed defence against a privileged local user who rewrites the spool and cursor.
- Events are queued after their receipt is published. A crash between the two
  loses the SIEM copy, not the receipt, and nothing marks that gap.
  The `started` gap marker survives a crash during overflow, but the dropped count
  is held in memory and is lost if the process dies before the `ended` marker.
- Under `fail_closed` a request already admitted can still finish while the buffer
  is nearly full. The headroom is sized for that, not for unlimited concurrency.
- A leaf pin needs manual rotation. Tokens are read once at startup.
- Datadog and Splunk acceptance of a request (2xx) is treated as delivered. Splunk
  indexer acknowledgement is not used.
- The demo dashboard and the CLI `evidence export` command still use the older
  unhardened path.
