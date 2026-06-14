# Appendix B: Open Research & Prototype Stack

Companion to [whitepaper.md](./whitepaper.md) (v0.9.11). Last updated: 2026-06-14.

IAC and router session CP schemas: §4.2.1–§4.2.2. Merkle trees: §5.1. Attestation transport and cold anchoring: §11.3–§11.4.

## Clinical cortical implants: what is public

High-bandwidth invasive BCIs have **no open third-party attestation SDK** suitable for Membrane integration today. Useful public material (vendor-neutral):

| Resource | URL | Use for Membrane |
|----------|-----|------------------|
| ASIC / dev platform paper | [bioRxiv 703801](https://www.biorxiv.org/content/10.1101/703801v4) | Streaming model, electrode counts, UDP multicast — representative implant-class architecture |
| BCI cybersecurity survey | [arXiv:2007.09466](https://arxiv.org/abs/2007.09466) | BLE pairing, firmware updates, companion app as TCB |
| Wearable BCI security (Argus) | [arXiv:2201.07711](https://arxiv.org/abs/2201.07711) | Information-flow control patterns for neural data paths |

Treat invasive cortical implants as a **future Liveness-2 target**, not a Phase 0 build dependency.

---

## Merkle trees (required)

All bulk commitments use **SHA-256 Merkle trees** per whitepaper §5.1:

| Tree | Built from | When |
|------|------------|------|
| Channel | `H(0x00 ‖ feature_chunk)` from EEG/IMU/router context chunks | Every CP / Δt |
| Bus | `H(0x01 ‖ canonical MembraneEvent)` | Every bus append |
| Witness | `H(0x03 ‖ witness_pubkey)` | Every CP with WoT |
| CP chain rollup | `H(0x02 ‖ cp_hash)` | Daily cold anchor input |

Implement with a single `merkle_sha256(leaves: bytes[], domain: u8)` helper; sort leaves lexicographically before pairing.

---

## Attestation transport profiles (Phase 0)

The protocol is **transport-agnostic**. Pick a hot bus for every-Δt CPs; add cold anchors for audit timestamps and durability.

| Profile | When | Implementation sketch |
|---------|------|------------------------|
| **Hot (required)** | Every Δt | Self-hosted append-only log + bus Merkle root in each CP |
| **Warm (optional)** | Hourly+ | IPFS or object store for STARK bundles + signed rollup JSON |
| **Cold A (optional)** | Weekly+ | Arweave bundle containing rollup + `.ots` |
| **Cold B (optional)** | Daily+ | L2 calldata commit of `cp_chain_root` |
| **Cold C (optional)** | Daily+ | [OpenTimestamps](https://opentimestamps.org/) on `ots_digest` (§5.1) |

### Cold C: OpenTimestamps rollup (recommended Bitcoin anchor)

```bash
# 1. Export daily rollup (hypothetical CLI)
membrane export-rollup --day 2026-06-14 --out rollup.json
# rollup.json contains cp_chain_root, last_bus_root, subject_pubkey, period_*

# 2. Sign canonical JSON → compute ots_digest = SHA256(json || sig)
membrane sign-rollup rollup.json --out rollup.signed.json

# 3. Stamp (submit to ≥2 calendars)
ots stamp rollup.signed.json
# → rollup.signed.json.ots (pending)

# 4. After Bitcoin confirms (hours later)
ots upgrade rollup.signed.json.ots
ots verify rollup.signed.json.ots

# 5. Optional: announce on hot bus before Bitcoin confirms
# MembraneEvent type membrane.anchor.ots { target: ots_digest, ots_b64, period_end }
```

**Tools:** `pip install opentimestamps-client`; Bitcoin headers via local `bitcoind` (pruned OK) or Esplora.

**Binding rule:** Stamp `SHA256(canonical_bundle || signature)`, not `cp_chain_root` alone — same rationale as [NIP-3B](https://github.com/nostr-protocol/nips) `id+sig` (prevents backdated signature replay).

### Example hot bus: NOSTR relay profile

One practical Phase 0 backend maps `MembraneEvent` to [NOSTR](https://github.com/nostr-protocol/nips) application events:

| `MembraneEvent.type` | NOSTR mapping |
|----------------------|---------------|
| `membrane.cp.liveness` | kind `31990`, tag `["k", "the-membrane-liveness"]` |
| `membrane.cp.router` | kind `31990`, tag `["k", "the-membrane-router"]` |
| `membrane.cp.bci` | kind `31990`, tag `["k", "the-membrane-bci"]` |
| `membrane.iac` | kind `31990`, tag `["k", "the-membrane-iac"]` |
| `membrane.anchor.ots` | kind `31990`, tag `["k", "the-membrane-anchor-ots"]` |
| `membrane.alert.degraded` | kind `31991` |

Common tags: `["e", <prev_event_id>]`, `["p", <subject_pubkey>]`. Run a **self-hosted relay**; treat public relays as optional read replicas only.

Other hot-bus options: MQTT broker, Hypercore feed, SQLite append log + Tailscale sync.

---

## Open acquisition & decode (Phase 0 hardware)

| Project | License | Role |
|---------|---------|------|
| [BrainFlow](https://github.com/brainflow-dev/brainflow) | MIT | Unified SDK for OpenBCI, Muse, etc. — channel registration + feature extraction |
| [OpenBCI](https://openbci.com/) | Open hardware + OSS tooling | Lab bench; [Galea](https://openbci.com/community/introducing-galea-bci-hmd-biosensing/) adds EEG/EMG/PPG/EDA in XR |
| [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) | BSD | Time-synced multimodal bus — use as **membrane data plane** |
| [MetaBCI](https://github.com/TBC-TJU/MetaBCI) | GPL-2.0 | Full decode pipeline; study **decoder drift** when models retrain |
| [PyNoetic](https://github.com/neurodiag/pynoetic-official) | GPL-3.0 | Real-time EEG → classifier pipelines |

### Invasive / bench research (not consumer implants)

| Project | Notes |
|---------|-------|
| [Iris-128](https://github.com/openic-org/iris-128) | Open 128-ch headstage (CERN-OHL) |
| [OpenMEA](https://www.biorxiv.org/content/10.1101/2022.11.11.516234v1) | Benchtop closed-loop electrophysiology |

---

## Security & firewall research (closest to Membrane thesis)

| Project | Type | Membrane mapping |
|---------|------|------------------|
| [iba-neural-guard](https://github.com/Grokipaedia/iba-neural-guard) | Open repo | **Decoded signal ≠ authorization.** Signed intent certificate before any BCI action; blocks capability drift |
| [Argus / LibArgus](https://arxiv.org/abs/2201.07711) | Paper (no public repo) | OS **information flow control** for wearable BCIs; 300+ vulns on Muse/NeuroSky/OpenBCI stacks |
| [NSP — Neural Sensory Protocol](https://qinnovate.com/guardrails/nsp/) | Spec draft | PQC wire protocol for neural frames; constant-rate BLE against timing side channels |
| [NeuroZKP](https://neurozkp.com/) | Project site | ZK proofs over neural signals — prove authorization without raw spike disclosure |

**iba-neural-guard** is the closest open articulation of the policy layer Membrane needs: the thought is not the authorization; the signed certificate is.

---

## LLM ↔ brain coupling (adversary models)

Active research on **closed-loop brain → LLM routing** — where identity drift and sequestration appear first:

| Project | URL | Why it matters |
|---------|-----|----------------|
| NeuroLM | [github.com/935963004/NeuroLM](https://github.com/935963004/NeuroLM) | EEG treated as "foreign language" inside an LLM — native cognition absorbed into model space |
| SYNAPTICON | [github.com/AlbertBarqueDuran/SYNAPTICON](https://github.com/AlbertBarqueDuran/SYNAPTICON) | EEG → text → LLM → output closed loop |
| Brain-LLM Interface | [arXiv:2603.16897](https://arxiv.org/html/2603.16897) | EEG gates LLM refinement at inference time |

| Membrane term | Meaning in this stack |
|---------------|----------------------|
| **Identity drift** | Decoder or LLM updates, session fork, model swap without new Chain Proof |
| **Sequestration** | Cognition lives in external inference; endogenous loop bypassed or unreadable |
| **Firewall job** | Gate which channels may couple; sever on missing/stale CP |

---

## Suggested Phase 0 architecture

```text
┌─────────────┐     ┌──────────────┐     ┌─────────────────────────┐
│ OpenBCI /   │ LSL │ Local Membrane│     │ Self-hosted attestation │
│ Muse EEG    ├────►│ Node (TEE)    ├────►│ bus + bus Merkle root   │
└─────────────┘     │ · channel Merkle     └─────────────────────────┘
┌─────────────┐     │ · CP prover   │              │
│ Local LLM   │◄───►│ · session gate│────► Fail closed: no CP →
│ (optional)  │     └──────────────┘      kill LLM + BCI decode
└─────────────┘              │
                             └── Daily: cp_chain_root rollup → OTS stamp
                                 (optional Arweave/L2 weekly)
```

### Build order

1. **BrainFlow + LSL** — stream EEG features; Merkle-commit chunks locally; never publish raw traces.
2. **Channel registry** — YAML list of permitted paths (BCI app, local LLM port, forbidden cloud URLs).
3. **Merkle + Winterfell Liveness-1** — prove `pub_merkle_root` over feature vectors + timestamp (§Part 2).
4. **Intent gate** — adapt [iba-neural-guard](https://github.com/Grokipaedia/iba-neural-guard): no signed scope → no decode→action mapping.
5. **Attestation bus** — append signed `MembraneEvent`; maintain bus Merkle tree; optional NOSTR backend.
6. **Daily OTS rollup** — sign `RollupBundle`, `ots stamp`, `ots upgrade` when confirmed.
7. **WoT** — K=2 human witnesses sign CP validity out-of-band (Signal/video).

### What Phase 0 proves

- Subject can run membrane-gated channels **without default cloud LLM routing**.
- Unauthorized channel open → detectable in attestation chain (if channel touches the bus).
- Daily `cp_chain_root` + OTS gives third-party **existence time** for the rollup (audit, not liveness).
- Subject can **sever** all links (dead-man's key / kill switch).

### What Phase 0 does not prove

- Covert passive surveillance with no attested channel.
- Thought content or consciousness.
- Invasive cortical implant integration.

---

## References to add to implementation issues

- Khan, E. (2026). *Brain Hacking: AI for Safeguarding Against Dangerous AI*. [Preprints.org](https://www.preprints.org/manuscript/202601.0156) — cognitive firewall framing (T3).
- Li et al. BCI cybersecurity survey — [arXiv:2007.09466](https://arxiv.org/abs/2007.09466) — implant companion-app threat model.
- Argus wearable BCI security — [arXiv:2201.07711](https://arxiv.org/abs/2201.07711).
- OpenTimestamps — https://opentimestamps.org/ — Bitcoin batched timestamps for rollup digests.
