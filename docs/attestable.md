# Attestable

**The fail-closed control point for AI agents with production access.**

Attestable is the commercial product surface of the Membrane gate: an enforcement and evidence layer for enterprises that grant AI agents write access to production systems.

---

## Product overview

Enterprises are handing AI agents real keys: agents that merge code, edit tickets, change infrastructure, and touch customer data. When an agent does something wrong, teams are left with scattered logs and vendor chat histories—and no way to prove which model, which context, which policy, and which person or parent agent actually authorized the action.

Attestable is the enforcement and evidence layer that sits directly in front of your agents. Every model call and every tool action must carry a live, signed authorization that names the allowed model, the allowed tools, and the scope of the task. Each action is written to a tamper-evident, hash-linked receipt chain. If the authorization is missing, expired, or out of scope—or if the model or tool is swapped mid-task—Attestable blocks the action and can sever the agent instantly.

Your security team gets a clear timeline of what each agent did, why it was allowed, and where it was stopped. When an incident happens, you reconstruct any action in minutes and export a signed evidence pack for auditors and counsel.

Attestable enforces and proves the traffic that runs through it. It is deployed inside your environment as the required path for in-scope agents, so approved actions are provable and unauthorized ones never reach production.

---

## Problem

Companies are granting AI agents write access to production systems, but existing logs are editable, incomplete, and can't prove an action was authorized by an unexpired policy—so incident reconstruction is slow and unreliable.

## Solution

A fail-closed gateway that requires a live, signed authorization for every model and tool call, records each action in a tamper-evident receipt chain, and blocks or severs the agent the moment the chain breaks.

## Why now

Agent frameworks with real write scopes (code, infra, CRM, ticketing) are shipping into production faster than the controls to govern them, and the first serious agent-caused incidents are landing on security teams' desks.

## Who pays

Heads of Security and Platform Engineering at growth-stage and enterprise software companies whose internal agents can already change production—teams with existing zero-trust and data-protection budgets.

---

## Differentiators

1. **It enforces, it doesn't just watch.** Observability tools explain what an agent did after the fact. Attestable is an inline control point that refuses unauthorized actions before they reach production.

2. **Authorization is bound to the action, not the prompt.** Every action carries a signed policy naming the exact model, tools, and scope, linked into a tamper-evident receipt chain. A silent model or tool swap breaks the chain and is blocked.

3. **Incident reconstruction in minutes, with exportable evidence.** Because approvals and actions are hash-linked, security can trace any action to its authorizing policy and issuer, and hand auditors a signed evidence pack—no dependence on a vendor's mutable logs.

---

## 60-second pitch

> Companies are giving AI agents real production access—agents that merge code, change infrastructure, and touch customer data. The problem is that when an agent does the wrong thing, all you have is logs. Logs that are editable, incomplete, and can't prove the action was actually authorized.
>
> Attestable is the control point that sits in front of your agents. Every model call and every tool action has to carry a live, signed authorization—this model, these tools, this task, for this long. Every action gets written to a tamper-evident receipt chain. If the authorization is expired, out of scope, or the model gets swapped mid-task, we block it. We can sever the agent on the spot.
>
> Your security team gets a clean timeline of what each agent did and why it was allowed, and when something goes wrong they reconstruct it in minutes and export signed evidence for auditors.
>
> We enforce and prove everything that runs through the gateway. Approved actions are provable; unauthorized ones never hit production. We're working with security teams whose agents already have the keys.

---

## Demo narrative

1. **Grant scope.** An operator issues a 15-minute authorization for a support agent: model X, tools limited to *comment on tickets* and *post to Slack*, bound to one task.
2. **Approved action.** The agent posts a ticket comment. The console shows a green, linked receipt: policy → model → tool, all matching.
3. **Blocked swap.** The agent tries to use a different model, or reach for *merge to main*—outside the authorization. Attestable blocks it; a red receipt shows exactly why.
4. **Expiry / sever.** The authorization expires (or security hits "sever"). The next tool call fails closed; an alert lands in the incident channel.
5. **Reconstruct.** Security opens the timeline, clicks any action, and sees the model, context scope, policy, and issuer behind it—no log spelunking.
6. **Export evidence.** One click produces a signed evidence pack; verify the hash chain offline in seconds.

---

## Language

### Prefer

control point; fail-closed; enforcement and evidence layer; signed authorization; policy scope; tamper-evident receipt chain; block; sever; incident reconstruction; evidence pack; production write access; agents with the keys; provable actions.

### Avoid

- Overclaims: "prevents all misuse," "guarantees compliance," "proves the agent's intent/reasoning," "proves nothing was deleted," "detects everything the agent does." (Coverage is limited to gateway-routed traffic.)
- Category confusion: "observability," "monitoring," "AI safety," "guardrails," "alignment"—these blur the fail-closed enforcement position.
- Vague hype: "revolutionary," "trustless," "unhackable," "military-grade."

### Honest scope

Attestable enforces and proves the traffic routed through it. It does not make claims about an agent's hidden reasoning, data deletion, activity that bypasses the gateway, or regulatory compliance on its own.

---

## Initial use case & ICP

**Use case:** Internal, tool-using agents that can mutate production—for example, an SRE or support agent authorized to comment on incidents, post to Slack, and open or change tickets—where security must prove, per action, which model and policy authorized it and be able to stop it instantly.

**Ideal customer profile:** Head of Security or Platform Engineering at a Series B–D B2B software company (about 50–500 engineers) that has already deployed agents with write scopes to systems like GitHub, Jira/ServiceNow, Slack, or cloud infra, and has an existing zero-trust or data-protection budget. Not chatbot pilots; teams whose agents already hold the keys.
