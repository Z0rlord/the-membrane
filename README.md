# The Membrane

Research architecture for a **cognitive boundary** — a nervous-system firewall against AI routing, invasive BCI read/write paths, and non-invasive thought inference.

The Membrane does not read minds. It attests **which channels may cross the boundary** (local LLM, BCI telemetry, cloud inference), whether they match prior commitments, and **fails closed** when attestation breaks.

## Documents

| File | Description |
|------|-------------|
| [docs/whitepaper.md](docs/whitepaper.md) | Full specification (v0.9.3) |
| [docs/appendix-open-research.md](docs/appendix-open-research.md) | Open-source BCI stacks, security research, and Phase 0 prototype path |

## Core idea

```text
 Endogenous cognition          THE MEMBRANE          Exogenous channels
 (nervous system)              (fail-closed gate)    (AI routers, BCI, sensors)
        │                              │                      │
        └────────── only attested ──────┴────── traffic ───────┘
```

**Threats:** AI routing (copilots/agents ingesting context), invasive neural channels (implants/BCIs), non-invasive inference (EEG, gaze, behavioral phenotyping).

**Mechanism:** zk-STARK Chain Proofs + TEE attestation + personal web-of-trust witnesses. No valid CP → sever the channel.

## Phase 0 (no Neuralink required)

OpenBCI or Muse → [Lab Streaming Layer](https://github.com/sccn/labstreaminglayer) → local TEE prover → optional **local** LLM session gate → NOSTR CP bus → fail closed on stale/missing attestation.

See [appendix-open-research.md](docs/appendix-open-research.md) for libraries and papers.

## Status

Research specification only. No reference implementation in this repo yet.

## License

- Documentation: [CC BY 4.0](LICENSE)
- Future reference code (when added): AGPL-3.0

## Author

Zorie R. Barber
