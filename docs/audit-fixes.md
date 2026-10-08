# October enforcement fixes and remaining limits

Tool CP receipts now attest the **request intent before dispatch**, not successful
execution. Context checks cover that request, not the response. A completed local
operation record means the connector returned success; an uncertain record requires
an operator to inspect GitHub. There is no automatic reconciliation or retry.

Mutating tool requests must include `operation_id` (1-128 bytes). Keep that ID across
retries. The caller proof covers it. The gate reserves `(caller, operation_id)` in
`MEMBRANE_OPERATION_DIR` (default `.membrane/tool-operations`) before publication
and dispatch. Reuse always fails closed, including changed bodies and restart.
Do not delete a reservation unless the upstream effect has been reconciled.
Use a persistent volume and one gate instance per journal. Separate instances with
separate storage, new operation IDs, or loss of the volume can still duplicate an
effect. A reservation can remain after a pre-dispatch failure; this sacrifices
availability rather than guessing whether the operation ran.

Each live dispatch checks authenticated operator relay alerts. A relay outage
blocks dispatch; an alert arriving after that check may not stop an in-flight
request. This is not a continuously subscribed cancellation channel. Relay reads
are capped at 5,000 events, so full durable history and truncation-safe recovery
remain unfinished. No live relay end-to-end test was run for these fixes.

Scope nonce, liveness and sever history are tracked independently in process and
restored from available router events at startup. The CP hash chain remains one
serialized operator chain. Full chain-link validation, durable per-caller/scope
storage and complete retained history are not implemented here.

`context_merkle_bound` is a lexical hash bound, not approved-content membership.
`forbidden_exports` checks credential declarations, not content or provider
retention. These controls must not be described as semantic exfiltration prevention.
