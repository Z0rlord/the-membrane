# GitHub connector (real tool path)

Operator vertical slice for The Membrane: allowlisted GitHub mutations go through
the production gate (`POST /v1/tools/invoke`) with live IAC, CP receipts, sever,
and SIEM export. This is **not** the public demo simulator.

The hosted sandbox at `membrane-demo.dojopop.live` stays simulation-only and must
not receive operator GitHub tokens.

## Tools

| Tool id | Effect | Typical IAC |
| --- | --- | --- |
| `github.comment` | POST issue/PR comment via GitHub API | allow |
| `github.issue.read` | GET issue/PR metadata (title digest only in receipts) | optional allow |
| `github.merge` | PUT merge PR | **omit** from allowlist to hard-block |

## Required env / config

| Name | Purpose |
| --- | --- |
| `MEMBRANE_GITHUB_TOKEN` or `GITHUB_TOKEN` | Fine-grained or classic token — never commit |
| `github_repo_allowlist` in channel registry | Explicit `owner/name` list; empty denies all |

Suggested token scopes: classic `repo`, or fine-grained **Issues: Read and write**
(and **Pull requests: Read and write** if merging is ever allowlisted). Prefer a
disposable pilot repo.

Receipts store `body_sha256` digests — not comment body plaintext or tokens.

## Six-step pilot recipe

Assume a local relay, `NOSTR_NSEC`, and registry entry:

```yaml
github_repo_allowlist:
  - your-org/disposable-pilot-repo
model_allowlist:
  - sha256:demo-model
```

```bash
export NOSTR_NSEC='…'                    # Doppler / secret manager
export MEMBRANE_RELAY_URL='ws://127.0.0.1:7777'
export MEMBRANE_GITHUB_TOKEN='…'         # never print / commit

# 1) Issue short-lived IAC — comment allowed, merge omitted
membrane iac issue \
  --model sha256:demo-model \
  --ttl-secs 900 \
  --tool github.comment \
  --out session-iac.json

# 2) Start gate (same signing key as IAC issuer)
membrane gate start \
  --registry tools/channel-registry.example.yaml \
  --iac session-iac.json \
  --listen 127.0.0.1:8787

# 3) Allowed: real comment
membrane tools invoke \
  --iac session-iac.json \
  --tool github.comment \
  --model sha256:demo-model \
  --owner your-org \
  --repo disposable-pilot-repo \
  --issue-number 1 \
  --body "Membrane pilot: allowed comment"

# 4) Blocked: merge before any GitHub call
membrane tools invoke \
  --iac session-iac.json \
  --tool github.merge \
  --model sha256:demo-model \
  --owner your-org \
  --repo disposable-pilot-repo \
  --pull-number 1
# → HTTP 403, blocked-action receipt / SIEM event

# 5) Sever → subsequent invokes fail closed
membrane sever --scope-id "$(jq -r .scope_id session-iac.json)"

# 6) Evidence / SIEM
membrane evidence export --format jsonl --since-secs 3600 --out membrane-siem.jsonl
```

Optional curl equivalent for step 3:

```bash
curl -sS http://127.0.0.1:8787/v1/tools/invoke \
  -H 'Content-Type: application/json' \
  -H "X-Membrane-IAC: $(cat session-iac.json)" \
  -d '{"tool":"github.comment","model":"sha256:demo-model","owner":"your-org","repo":"disposable-pilot-repo","issue_number":1,"body":"hello"}'
```

## Simulated vs real

| Path | GitHub / Jira / Slack | IAC / CP / sever |
| --- | --- | --- |
| `membrane demo` / public sandbox | Simulated JSON only | Real checks |
| `membrane gate start` + `/v1/tools/invoke` | Real GitHub API when token + repo allowlist set | Real |

## Tests

```bash
cargo test -p membrane-gate github
cargo test -p membrane-gate tool_invoke_policy
# Optional live call (ignored by default):
# MEMBRANE_GITHUB_INTEGRATION=1 MEMBRANE_GITHUB_OWNER=… MEMBRANE_GITHUB_REPO=… \
#   cargo test -p membrane-gate live_github_integration -- --ignored
```
