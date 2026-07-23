# The Membrane

**A fail-closed authorization gateway for AI agents with production write access.**

Every model and tool call needs a live, signed, time-bounded scope; each action writes a tamper-evident receipt; broken continuity blocks or severs the agent.

One product. Enterprise teams and sovereign operators share the same gate; what differs is who holds the keys and where it runs.

---

## Product overview

Organizations are handing AI agents real keys: agents that merge code, edit tickets, change infrastructure, and touch customer data. When an agent does something wrong, teams are left with scattered logs and third-party chat histories—and no way to prove which model, which context, which policy, and which person or parent agent actually authorized the action.

The Membrane is the enforcement and evidence layer that sits directly in front of your agents. Every model call and every tool action must carry a live, signed authorization that names the allowed model, the allowed tools, and the scope of the task. Each action is written to a tamper-evident, hash-linked receipt chain. If the authorization is missing, expired, or out of scope—or if the model or tool is swapped mid-task—the Membrane blocks the action and can sever the agent instantly.

Membrane receipts can be exported as vendor-neutral JSON Lines or OCSF-inspired JSON for an existing SIEM, SOC, or SOAR. The Membrane produces high-integrity authorization telemetry; those systems ingest, correlate, and respond to it. SIEM export is telemetry *out*—not a monitoring product.

The Membrane enforces and proves the traffic that runs through it. Deploy it as the required path for in-scope agents so approved actions are provable and unauthorized ones never reach production.

---

## Who it’s for

### Enterprise

Security and platform teams who already gave agents keys (GitHub, tickets, infra). They need prove-and-stop—not another log stream. Typical buyers: Heads of Security and Platform Engineering at growth-stage and enterprise software companies with existing zero-trust and data-protection budgets.

### Sovereigns

Operators who refuse opaque vendor control planes. Self-host the gate, hold your own keys, attest locally, optional independent timestamping (e.g. OpenTimestamps). Sovereignty is *deployment and trust posture*, not a second product or a separate SKU.

---

## Problem

Companies are granting AI agents write access to production systems, but existing logs are editable, incomplete, and can't prove an action was authorized by an unexpired policy—so incident reconstruction is slow and unreliable.

## Solution

A fail-closed gateway that requires a live, signed authorization for every model and tool call, records each action in a tamper-evident receipt chain, and blocks or severs the agent the moment the chain breaks.

## Differentiation (one line)

**Enforce before production** — observability watches after the fact; prompt filters rewrite text; The Membrane refuses unauthorized actions at the gate.

## Why now

Agent frameworks with real write scopes (code, infra, CRM, ticketing) are shipping into production faster than the controls to govern them, and the first serious agent-caused incidents are landing on security teams' desks.

## Commercial stage

Self-hosted pilots in the customer’s environment, then license and support. No hosted production SaaS.

---

## Differentiators

1. **It enforces, it doesn't just watch.** Observability tools explain what an agent did after the fact. The Membrane is an inline control point that refuses unauthorized actions before they reach production.

2. **Authorization is bound to the action, not the prompt.** Every action carries a signed policy naming the exact model, tools, and scope, linked into a tamper-evident receipt chain. A silent model or tool swap breaks the chain and is blocked.

3. **Incident reconstruction in minutes, with exportable evidence.** Because approvals and actions are hash-linked, security can trace any action to its authorizing policy and issuer, and hand auditors a signed evidence pack—no dependence on a provider's mutable logs. Sovereign operators get the same chain under keys they control.

---

## 60-second pitch

> Companies are giving AI agents real production access—agents that merge code, change infrastructure, and touch customer data. When an agent does the wrong thing, all you have is logs: editable, incomplete, and unable to prove the action was authorized.
>
> The Membrane is a fail-closed authorization gateway in front of those agents. Every model call and every tool action needs a live, signed, time-bounded scope. Every action writes a tamper-evident receipt. If the authorization is expired, out of scope, or continuity breaks, we block—or sever—the agent.
>
> Security teams get a clean timeline and SIEM-ready telemetry out. Sovereign operators run the same gate self-hosted, with their own keys. One product; prove-and-stop either way.
>
> We enforce and prove everything that runs through the gateway. Approved actions are provable; unauthorized ones never hit production. We're working with teams whose agents already have the keys.

---

## Demo narrative

1. **Grant scope.** An operator issues a 15-minute authorization for a support agent: model X, tools limited to *comment on tickets* and *post to Slack*, bound to one task.
2. **Approved action.** The agent posts a ticket comment. The console shows a green, linked receipt: policy → model → tool, all matching.
3. **Blocked swap.** The agent tries to use a different model, or reach for *merge to main*—outside the authorization. The Membrane blocks it; a red receipt shows exactly why.
4. **Expiry / sever.** The authorization expires (or security hits "sever"). The next tool call fails closed; an alert lands in the incident channel.
5. **Reconstruct.** Security opens the timeline, clicks any action, and sees the model, context scope, policy, and issuer behind it—no log spelunking.
6. **Export evidence.** One click produces a signed evidence pack; verify the hash chain offline in seconds.

Local walkthrough: [demo.md](demo.md) (`cargo run -p membrane-cli -- demo`). Hosted sandbox tools are simulated; the production gate can invoke real connectors when configured (see [github-connector.md](github-connector.md)).

---

## Language

### Prefer

authorization gateway; fail-closed; enforcement and evidence; signed authorization; time-bounded scope; tamper-evident receipt chain; block; sever; prove-and-stop; production write access; agents with the keys; self-hosted; SIEM telemetry out.

### Avoid

- Overclaims: "prevents all misuse," "guarantees compliance," "proves the agent's intent/reasoning," "proves nothing was deleted," "detects everything the agent does." (Coverage is limited to gateway-routed traffic.)
- Category confusion: "observability," "monitoring," "AI safety," "guardrails," "alignment"—these blur the fail-closed enforcement position.
- Framing sovereignty as a separate product or lab-only protocol story in the hero.
- Vague hype: "revolutionary," "trustless," "unhackable," "military-grade."

### Honest scope

The Membrane enforces and proves the traffic routed through it. It does not make claims about an agent's hidden reasoning, data deletion, activity that bypasses the gateway, or regulatory compliance on its own.

Architecture, attestation-bus research, BCI channels, and zk roadmap belong in the whitepaper and appendix—not the product lede.

---

## Initial use case & ICP

**Use case:** Internal, tool-using agents that can mutate production—for example, an SRE or support agent authorized to comment on incidents, post to Slack, and open or change tickets—where security must prove, per action, which model and policy authorized it and be able to stop it instantly.

**Ideal customer profile (enterprise):** Head of Security or Platform Engineering at a Series B–D B2B software company (about 50–500 engineers) that has already deployed agents with write scopes to systems like GitHub, Jira/ServiceNow, Slack, or cloud infra, and has an existing zero-trust or data-protection budget. Not chatbot pilots; teams whose agents already hold the keys.

**Sovereign operator:** Same gate and receipt model, self-hosted, keys and attestation under operator control.
