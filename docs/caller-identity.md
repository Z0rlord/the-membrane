# Caller-bound IACs and identity grants

The operator still signs the IAC using the existing Schnorr/canonical-JSON mechanism.
`caller_pubkey` is now part of that signed view. The caller holds a separate key and
proves possession on every production chat or tool request. An IAC is not a bearer
credential: copying it is insufficient to act.

## Operator setup

1. Put the caller's lowercase, 64-hex public key in `identities` in the channel registry.
   Give exact scopes, channels, models, tools and repositories. No wildcard matching.
   Missing lists grant nothing; an absent identity or `revoked: true` denies.
2. Issue with the operator key: `membrane iac issue --caller-pubkey <caller-key>
   --scope-id pilot-scope --model <model> --tool github.comment --out iac.json`.
   Relay and signing-key options remain as before. Do not give the operator secret to an agent.
3. Start the gate with the registry and signed IAC as before. The CLI start path
   rereads identity grants on each production authorization and again immediately
   before dispatch. Edit the file atomically (temporary file then rename).
4. Invoke using the caller key: `membrane tools invoke --gate-pubkey <operator-key>
   --iac iac.json --tool github.comment --model <model> --owner <owner> --repo <repo>
   --issue-number 1 --body <comment>`. Supply the caller secret through the existing
   `NOSTR_NSEC` environment or signing-key option, not a checked-in file.

Existing sovereign chat is the self-caller case (operator and caller keys equal).
It binds new session IACs and sends proofs automatically. Its generated scope still
needs an explicit registry grant. Distinct agents use operator-issued IACs, not
self-issued authority; the tool CLI supports separate operator/caller keys.

## Request proof

Fetch `/health` for `caller_audience` (operator key plus a fresh process challenge).
The `X-Membrane-Caller-Proof` JSON has `timestamp`, `nonce`, `signature`. Sign the
SHA-256 digest of the canonical JSON produced by `membrane_core::caller::CallerProof`:
domain `membrane/caller-proof/v1`, audience, POST, exact route, typed request body,
IAC signable hash, timestamp and random 64-hex nonce. The Rust helper is the reference.
The gate uses the deserialized request shape, including serde defaults/null fields.
Use the typed `ChatRequest` or `ToolInvokeRequest` when signing.

Proofs cannot be future-dated and expire after 60 seconds. Nonces are consumed after
signature verification, even if policy later denies. A mutex-protected cache denies
replay and fails closed at 10,000 retained entries. A restart rotates the audience,
so a captured old proof does not revive. Use one gate process per audience; this is
not a shared multi-replica replay store. Use TLS outside localhost. Proofs do not
hide request content and do not protect a compromised caller private key.

## Authorization and revocation

Permission is the intersection of the operator-signed IAC, loaded global policy,
current registry identity grant and existing liveness/export/context checks. No
identity grant expands the IAC or bypasses an existing clause. Registry reread
errors deny; an unavailable replay cache denies. Production has no identity-free
fallback, including when a default IAC is configured. Old unbound IACs remain
parseable for historical/demo data but production requests deny until reissued.
The demonstration remains simulation-only and does not exercise caller auth.

Set `identities.<key>.revoked: true` or remove the key. The next authorization sees
that change without restarting. Revocation is not retroactive: an already-dispatched
connector call cannot be recalled, and a file edit racing the final check can win
just after it. There is no dashboard write endpoint. Library embedders must use
`with_identity_registry_path` for hot reload; without it grants are immutable for
that Gate instance. Global connector/config changes still require restart.

## Audit and advisor

`Decision.agent` remains the gate signer for compatibility. New optional
`authenticated_identity` is the verified caller key on allows and on denials after
proof verification (including unknown/revoked identity). Authentication failures
leave it absent, never record an attacker-claimed key as authenticated.
`/audit` exports the same field. Old snapshots still parse. The advisor groups by
caller and can recommend exact-key revocation for review; it never auto-grants or
un-revokes from a denial and does not generate global-widening patches for
identity-attributed denials. This bounded observational log is not a receipt store.

## Tests

`cargo test --workspace`: caller/operator separation, wrong caller signature,
tampered caller binding, wrong body/route/audience, stale/future/unbound proof,
replay, unknown/revoked identity, permission intersection, live registry edits and
broken/missing registry, plus HTTP handler audit attribution and advisor grouping.
The real-money/live GitHub connector test stays ignored unless explicitly enabled.
