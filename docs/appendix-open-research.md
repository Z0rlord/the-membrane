# Appendix B: Open Research & Prototype Stack

Companion to [whitepaper.md](./whitepaper.md) (v0.9.4). Last updated: 2026-06-14.

**Independent research** — not affiliated with any organization named herein. IAC and router session CP schemas: whitepaper §4.2.1–§4.2.2.

## Neuralink: what is public

Neuralink has **no open SDK** and no third-party attestation API for the clinical N1 stack. Useful public material:

| Resource | URL | Use for Membrane |
|----------|-----|------------------|
| ASIC / dev platform paper | [bioRxiv 703801](https://www.biorxiv.org/content/10.1101/703801v4) | Streaming model, electrode counts, UDP multicast — not consumer app stack |
| Patient / device overview | [neuralink.com/patient-information](https://neuralink.com/patient-information/) | Threat model for BLE pairing, firmware updates, phone app as TCB |
| PRIME study | [ClinicalTrials NCT06429735](https://clinicaltrials.gov/study/NCT06429735) | Regulatory context only |

Treat Neuralink as a **future Liveness-2 target**, not a build dependency today.

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
┌─────────────┐     ┌──────────────┐     ┌─────────────────┐
│ OpenBCI /   │ LSL │ Local Membrane│     │ Self-hosted     │
│ Muse EEG    ├────►│ Node (TEE)    ├────►│ NOSTR relay     │
└─────────────┘     │ · channel reg │     │ kind 31990 CPs  │
                    │ · Merkle roots│     └─────────────────┘
┌─────────────┐     │ · CP prover   │
│ Local LLM   │◄───►│ · session gate│────► Fail closed: no CP →
│ (optional)  │     └──────────────┘      kill LLM + BCI decode
└─────────────┘
         ▲
         └── Context hash + model id committed in each CP
             (cloud inference off by default)
```

### Build order

1. **BrainFlow + LSL** — stream EEG features; never publish raw traces to NOSTR.
2. **Channel registry** — YAML list of permitted paths (BCI app, local LLM port, forbidden cloud URLs).
3. **Merkle commitment circuit** — Winterfell Liveness-1 over feature vectors + timestamp (see whitepaper Part 2).
4. **Intent gate** — adapt [iba-neural-guard](https://github.com/Grokipaedia/iba-neural-guard) pattern: no signed scope → no decode→action mapping.
5. **NOSTR CP bus** — kind `31990`, tags `#the-membrane`, hash chain to prior event.
6. **WoT** — K=2 human witnesses sign CP validity out-of-band (Signal/video), not live ritual watching.

### What Phase 0 proves

- Subject can run membrane-gated channels **without default cloud LLM routing**.
- Unauthorized channel open → detectable in attestation chain (if channel touches the bus).
- Subject can **sever** all links (dead-man's key / kill switch).

### What Phase 0 does not prove

- Covert passive surveillance with no attested channel.
- Thought content or consciousness.
- Neuralink-class invasive implant integration.

---

## References to add to implementation issues

- Khan, E. (2026). *Brain Hacking: AI for Safeguarding Against Dangerous AI*. [Preprints.org](https://www.preprints.org/manuscript/202601.0156) — cognitive firewall framing (T3).
- Li et al. BCI cybersecurity survey — [arXiv:2007.09466](https://arxiv.org/abs/2007.09466) — Neuralink BLE threat model.
- Argus wearable BCI security — [arXiv:2201.07711](https://arxiv.org/abs/2201.07711).
