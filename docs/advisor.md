# Recommendation advisor

`membrane-advisor` turns gate denials into a reviewable policy patch. It is
deterministic (no model, no network, no randomness), read-only (it prints text, it
never opens the registry for writing), and advisory (a human applies and merges the
patch through normal code review).

## Use

```sh
curl -s http://127.0.0.1:8788/audit | \
  cargo run -q -p membrane-advisor -- --registry tools/channel-registry.example.yaml
# Only the diff, for `git apply`:
... --format patch
# Ignore one-off denials:
... --min-denials 3
```

Run from the repo root with a repo-relative registry path so the diff headers apply
with `git apply`. `--snapshot FILE` reads a saved snapshot instead of stdin.

## What it recommends

| Rule | Output |
| --- | --- |
| `repository_allowlist` | Patch adding the one exact `owner/name` to `github_repo_allowlist` |
| `model_allowlist` (not in registry) | Patch adding the one exact model id to `model_allowlist`; the signed IAC must also list it |
| `model_allowlist` (already in registry) | Advice: the IAC is what denies; re-issue it |
| `tool_allowlist`, `iac_validity`, `channel_allowlist`, `export_restriction`, `context_bound` | Advice: these live in the signed IAC, which a text patch cannot change |
| `iac_signature` | Investigate. Never a policy change |
| `session_stale`, `session_degraded`, connector faults | Operate. Never raise `delta_t_secs` to hide it |

Every recommendation is conditional: if the denied call was not intended, change
nothing, the denial is the gate working.

## Safety rules

- **Narrowest change.** One exact entry per patch. No wildcards, ever.
- **Hostile input.** The denied model, tool and repository names are caller-chosen
  strings. A name enters a patch only if it is a plain identifier or `owner/name`;
  anything else (wildcards, newlines, spaces, `..`) gets advice and no patch. The
  edited registry is re-parsed and compared: only the intended list may change.
- **Unsupported shapes are refused.** Inline lists (`[a, b]`) are not rewritten.
- **Review-by comment.** Added entries carry a `review by <date>` comment
  (snapshot time plus 7 days). The registry format has no expiry, so this is a
  reminder, not enforcement. Real time-boxing needs an IAC `valid_until`.
- **Per-identity grants are not available yet.** The IAC does not authenticate a
  caller-specific agent id (see `operator-dashboard.md`), so the advisor cannot name
  "identity X". Per-identity and deny-spike recommendations wait on that.

## Gate change

`Decision` in the audit snapshot gains an optional `subject`: the model, tool or
repository a denial named, control characters dropped and capped at 128 characters.
It is observational only and never an authorization input. Older snapshots without
it still parse; the advisor then says the log has no subject.
