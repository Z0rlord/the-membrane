# SIEM / SOC export

The Membrane emits authorization telemetry for security systems to consume. It
does not replace a SIEM, SOC, or SOAR, and these exports do not imply an
integration or partnership with any particular vendor.

## Formats

- `jsonl` — newline-delimited `membrane.action_receipt` records for syslog
  collectors, file tailers, object storage, and HTTP bulk ingestion.
- `ocsf` — an OCSF-inspired JSON projection with familiar `actor`, `session`,
  `api`, `policy`, `status`, and `metadata.product` fields. It intentionally
  omits OCSF class/activity numeric IDs until the mapping is certified against
  a pinned OCSF release.

Both formats include timestamps, public agent/subject identifiers,
session/scope identifiers when known, allowlisted model/tool names, policy and
receipt digests, parent receipt digests, outcomes, and block/degradation
reasons. They omit prompts, action bodies, private keys, credentials, and
signatures.

## CLI export

Export the last day of signed attestation-bus receipts and alerts:

```bash
membrane evidence export \
  --relay "$MEMBRANE_RELAY_URL" \
  --since-secs 86400 \
  --format jsonl \
  --out membrane-siem.jsonl
```

Use `--format ocsf --out membrane-siem.ocsf.json` for the OCSF-inspired pack.
Use `--subject-pubkey <hex>` to restrict a shared bus export to one public
subject identifier. Reading the bus requires no signing key.

The bus export covers published authorization, allowed-action CP,
blocked-action, sever, and stale/degraded events. Coverage remains limited to
traffic routed through the Membrane gate.

## Demo API

The simulation-only dashboard exposes the same normalized records:

```bash
curl -fsS 'http://127.0.0.1:8790/demo/api/siem?format=ocsf'
curl -fsS 'http://127.0.0.1:8790/demo/api/siem?format=jsonl'
```

The hosted sandbox uses the same routes at
`https://membrane-demo.dojopop.live`. Every demo record has
`"simulation": true`.

## Ingestion patterns

For file-based ingestion, run the CLI on a schedule, write to a temporary file,
then atomically rename it into the directory watched by the SIEM forwarder. A
collector can also tail JSON Lines and forward each line over syslog or HTTPS.

For HTTP ingestion today, POST the generated file with the authentication and
retry mechanism supplied by the operator's collector:

```bash
curl --fail-with-body \
  -H 'Content-Type: application/x-ndjson' \
  --data-binary @membrane-siem.jsonl \
  "$SIEM_INGEST_URL"
```

Keep collector tokens in the operator's secret manager. The Membrane CLI does
not yet ship a live webhook worker, delivery queue, or vendor-specific
authentication adapter.

## Deferred

- Certified OCSF class, category, activity, and severity identifiers
- A durable live webhook shipper with retries, backoff, and dead-letter storage
- CEF formatting and RFC 5424 framing
- Vendor-specific field packs and authentication adapters
