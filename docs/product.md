# The Membrane

**A fail-closed authorization gateway for AI agents with production / operational write access.**

Every model and tool call needs a live, signed, time-bounded scope; each action writes a tamper-evident receipt; broken continuity blocks or severs the agent.

Customers: **sovereigns**, **nation-states**, and **enterprise**. One product — a fail-closed authorization gateway for AI agents with production / operational write access. Self-host the gate; hold your own keys; prove every agent action.

---

## Product overview

Operators are putting AI agents on paths that mutate systems: agents that merge code, edit tickets, change infrastructure, and touch operational data. When an agent does something wrong, teams are left with scattered logs and third-party chat histories—and no way to prove which model, which context, which policy, and which person or parent agent actually authorized the action.

The Membrane is the enforcement and evidence layer that sits directly in front of those agents. Every model call and every tool action must carry a live, signed authorization that names the allowed model, the allowed tools, and the scope of the task. Each action is written to a tamper-evident, hash-linked receipt chain. If the authorization is missing, expired, or out of scope—or if the model or tool is swapped mid-task—the Membrane blocks the action and can sever the agent instantly.

Membrane receipts can be exported as vendor-neutral JSON Lines or OCSF-inspired JSON for an existing SIEM, SOC, or SOAR. The Membrane produces high-integrity authorization telemetry; those systems ingest, correlate, and respond to it. SIEM export is telemetry *out*—not a monitoring product.

The Membrane enforces and proves the traffic that runs through it. Deploy it as the required path for in-scope agents so approved actions are provable and unauthorized ones never reach production.

---

## Who it’s for

One product for three customer classes:

### Sovereigns

Operators who require self-hosted control, own their keys, and keep attested actions with reconstructable evidence — including high-assurance and mil/gov postures that need fail-closed authorization continuity.

### Nation-states

National and government operators who put agents on production or operational write paths and must prove, stop, and reconstruct authorized actions under their own infra and keys.

### Enterprise

Enterprises that grant AI agents production write access and run the same gate in-house: live signed scopes, tamper-evident receipts, block or sever when continuity breaks.

---

## Problem

Organizations are granting AI agents write access to production and operational systems, but existing logs are editable, incomplete, and can't prove an action was authorized by an unexpired policy—so incident reconstruction is slow and unreliable. High-assurance operators cannot outsource that continuity to an opaque vendor plane.

## Solution

A fail-closed gateway that requires a live, signed authorization for every model and tool call, records each action in a tamper-evident receipt chain, and blocks or severs the agent the moment the chain breaks—run under keys and infra the operator controls.

## Differentiation (one line)

**Enforce before production** — observability watches after the fact; prompt filters rewrite text; The Membrane refuses unauthorized actions at the gate.

## Why now

Agent frameworks with real write scopes (code, infra, CRM, ticketing, operational tools) are shipping faster than the controls to govern them, and operators who must prove and stop agent actions need a self-hosted enforcement path—not another log stream.

## Commercial stage

Self-hosted pilots in the operator’s environment, then license and support. No hosted production SaaS.

---

## Differentiators

1. **It enforces, it doesn't just watch.** Observability tools explain what an agent did after the fact. The Membrane is an inline control point that refuses unauthorized actions before they reach production.

2. **Authorization is bound to the action, not the prompt.** Every action carries a signed policy naming the exact model, tools, and scope, linked into a tamper-evident receipt chain. A silent model or tool swap breaks the chain and is blocked.

3. **Incident reconstruction with exportable evidence under your keys.** Because approvals and actions are hash-linked, operators can trace any action to its authorizing policy and issuer, and hand auditors a signed evidence pack—no dependence on a provider's mutable logs or control plane.

---

## 60-second pitch

> Operators are giving AI agents real production and operational access—agents that merge code, change infrastructure, and touch systems that matter. When an agent does the wrong thing, all you have is logs: editable, incomplete, and unable to prove the action was authorized.
>
> The Membrane is a fail-closed authorization gateway in front of those agents. Every model call and every tool action needs a live, signed, time-bounded scope. Every action writes a tamper-evident receipt. If the authorization is expired, out of scope, or continuity breaks, we block—or sever—the agent.
>
> Sovereigns, nation-states, and enterprise self-host the gate, hold their own keys, and keep reconstructable evidence for every agent action that runs through it.
>
> We enforce and prove everything that runs through the gateway. Approved actions are provable; unauthorized ones never hit production.

---

## Demo narrative

1. **Grant scope.** An operator issues a 15-minute authorization for a support agent: model X, tools limited to *comment on tickets* and *post to Slack*, bound to one task.
2. **Approved action.** The agent posts a ticket comment. The console shows a green, linked receipt: policy → model → tool, all matching.
3. **Blocked swap.** The agent tries to use a different model, or reach for *merge to main*—outside the authorization. The Membrane blocks it; a red receipt shows exactly why.
4. **Expiry / sever.** The authorization expires (or security hits "sever"). The next tool call fails closed; an alert lands in the incident channel.
5. **Reconstruct.** Open the timeline, click any action, and see the model, context scope, policy, and issuer behind it—no log spelunking.
6. **Export evidence.** One click produces a signed evidence pack; verify the hash chain offline in seconds.

Local walkthrough: [demo.md](demo.md) (`cargo run -p membrane-cli -- demo`). Hosted sandbox tools are simulated; the production gate can invoke real connectors when configured (see [github-connector.md](github-connector.md)).

---

## Language

### Prefer

authorization gateway; fail-closed; enforcement and evidence; signed authorization; time-bounded scope; tamper-evident receipt chain; block; sever; prove-and-stop; production / operational write access; self-host; hold your own keys; sovereigns; nation-states; enterprise; high-assurance; SIEM telemetry out.

### Avoid

- Overclaims: "prevents all misuse," "guarantees compliance," "proves the agent's intent/reasoning," "proves nothing was deleted," "detects everything the agent does." (Coverage is limited to gateway-routed traffic.)
- False certification or approval claims.
- Category confusion: "observability," "monitoring," "AI safety," "guardrails," "alignment"—these blur the fail-closed enforcement position.
- Framing enterprise as a separate SKU or hosted production SaaS; all three customer classes buy the same self-hosted gate.
- Vague hype: "revolutionary," "trustless," "unhackable," "military-grade."
- OpenAI branding; infra location name-dropping; Attestable as a live product name.

Architecture, attestation-bus research, BCI channels, and zk roadmap belong in the whitepaper and appendix—not the product lede.

---

## Initial use case & ICP

**Use case:** Tool-using agents that can mutate production or operational systems—for example, an SRE or support agent authorized to comment on incidents, post to Slack, and open or change tickets—where operators must prove, per action, which model and policy authorized it and be able to stop it instantly.

**Ideal customer profile:** Sovereigns, nation-states, and enterprise — operators who self-host, hold keys, require attested actions and reconstructable evidence, and fail closed when authorization continuity breaks. Not chatbot pilots; environments whose agents already hold write scopes. High-assurance and mil/gov postures sit under sovereign and nation-state customers.
