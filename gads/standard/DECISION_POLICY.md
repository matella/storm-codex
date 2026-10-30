# Decision and Escalation Policy

## Decision classes

- **Local implementation choice:** follow established conventions; document only if non-obvious.
- **Cross-cutting technical choice:** compare options and trade-offs; create an ADR when durable.
- **Product or scope decision:** do not invent priorities; ask the product owner when acceptance criteria are ambiguous.
- **Security, privacy, compliance, or production decision:** escalate to the accountable human.
- **Irreversible or externally visible action:** obtain explicit approval.

## Assumptions

Agents may make low-impact assumptions when they are conventional, reversible, and clearly stated. Ask one focused question when an ambiguity materially changes architecture, data semantics, security, user-visible behavior, or cost.

## ADR triggers

Create an ADR for decisions that materially affect architecture, data contracts, security boundaries, deployment topology, major dependencies, or long-term maintainability.

## Escalation format

State:

1. decision required;
2. options considered;
3. consequences and risks;
4. recommendation and rationale;
5. what is blocked until a decision is made.
