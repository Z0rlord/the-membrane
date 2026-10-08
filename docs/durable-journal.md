# Durable journal and upstream reconciliation

This layer builds on the October enforcement patches. It does not make GitHub
writes and relay receipts atomic, and it never retries an unresolved write.

## Storage and dispatch

`MEMBRANE_OPERATION_DIR` remains the per-instance persistent volume. A new
`journal.jsonl` stores checksummed, sequence-numbered, hash-linked frames. Every
append flushes the file and directory before returning. An OS file lock on
`instance.lock` excludes another cooperating gate process; a writer mutex
serializes in-process appends. No third-party dependency or lockfile change.

An intent includes caller, scope, operation ID and the exact typed request before
any write. Keep the same operation ID across retries. Completion means the
connector returned success, not independent proof of the effect. Transport errors
and connector failures record `uncertain`. A crash after intent leaves `reserved`.
All these IDs remain reserved, including after reconciliation. Caller/ID keys are
independent of scope, so moving to a new scope cannot duplicate an operation.
Old enforcement-series reservation files also remain blocked and are listed as
legacy records; their missing request metadata cannot be reconstructed safely.

The journal also stores a publication-in-progress marker before publishing a CP,
then its signed event, bus ID and caller binding before dispatch. A known local
validation failure cancels that publication marker; an uncertain relay error or
crash leaves it pending and blocks subsequent dispatch and startup. Resolving such
a marker is deliberately not automated. A partial frame, checksum failure, wrong
operator signature, broken CP parent or scope nonce rollback fails closed. No
partial tail is discarded and no corrupt file is reset to genesis.

## Durable recovery

Signed checkpoints rebuild one serialized operator CP chain. Scope counters,
liveness timestamps, caller ownership and sever history survive restart. A scope
cannot silently move between callers. Authenticated relay events can extend the
local chain, but cannot replace it with a recent window. Known checkpoints are
skipped; an unknown CP must extend the durable head. A fresh volume requires a
verifiable prefix from genesis. Missing prefixes and forks stop startup.

Startup requests relay history without the prior seven-day cutoff, still capped
at 5,000 events. A retained local journal survives an empty/truncated recent window;
a gap before a new checkpoint is rejected. Imported historical router receipts do
not contain verified caller identity, so those old scopes cannot be reused: issue
a new scope anchored to the recovered head. Caller identity is never guessed.

## Operator view

On startup the gate performs GET-only upstream observations for unresolved
operations. It never publishes another effect. On the loopback-only audit listener
(default `127.0.0.1:8788`):

```sh
curl http://127.0.0.1:8788/operations
curl -X POST http://127.0.0.1:8788/operations/reconcile
```

The second command repeats the read-only pass. Browser-origin writes are rejected;
these endpoints are not on the production tool router. The same session mutex
serializes reconciliation against local dispatch. All observations become
`operator_review`, even apparently matching effects. Connector errors become
`unverifiable`, not evidence of absence. Legacy records are named separately.

GitHub comment reconciliation reads at most 100 comments, reports candidate IDs
with an exact body match and marks bounded/paginated history. It does not equate a
matching comment with proof this operation created it. Merge reconciliation reads
the PR's merged flag and merge SHA, not proof of who caused the merge. Neither API
binds an effect to the gate operation ID, so neither observation automatically
resolves, releases or retries the reservation. No marker is added to user content.

GETs have a 15-second timeout, no redirects and a 1 MiB response limit. Responses
are reduced to structured evidence without storing upstream comment bodies or
tokens. The intent itself contains the request body and should be treated as
sensitive. Protect the volume with local permissions, encryption and backups.

## Remaining limits

- Exactly one gate instance per local journal; not a distributed journal or
  cross-instance deduplication service. File locking requires working local OS
  filesystem semantics. Losing/deleting/rolling back the volume loses guarantees.
- Checksums detect torn/corrupt frames; they are not keyed protection against a
  privileged local attacker rewriting history. Signed CPs authenticate receipts,
  not every local outcome or sever frame. No external archive or backup protocol.
- Relay queries are still bounded. A silent relay omission of an unseen sever or
  CP, with no later chain link exposing the gap, cannot be detected. The gate does
  not claim complete external history or cancellation of an in-flight request.
- Publication-interruption and legacy-record repair require operator inspection.
  There is intentionally no destructive repair/delete/retry command in this patch.
- Full request history is retained without compaction or retention limits. Replay
  currently scans the log; large journals will need an indexed checkpoint design.
  Reconciliation scans all unresolved operations sequentially and can delay startup.
- Live GitHub/relay integration, power-loss/filesystem fault injection, fuzzing,
  encryption, retention management and multiprocess stress remain untested.
- Existing lexical context bounds and export declaration checks remain unchanged;
  this work does not add content-level authorization or budget enforcement.
