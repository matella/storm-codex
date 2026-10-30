# GADS — Agent Operating Instructions

This file is the portable entry point for AI development agents. Read `gads/standard/OPERATING_STANDARD.md`, `gads/standard/SECURITY_AND_AUTONOMY.md`, and `gads/standard/WORKFLOW_AND_GATES.md` before making material changes.

## Authority and precedence

1. Explicit instructions from the user for the current task, provided they do not conflict with safety or organizational controls.
2. Repository-specific approved policy and architecture documentation, including the project rules in gads/PROJECT.md when that file exists.
3. GADS normative documents.
4. Agent defaults and inferred conventions.

If instructions conflict, stop before consequential action, explain the conflict, and ask for a decision. Never silently override repository policy.

## Mandatory working rules

- Inspect the repository before proposing or editing.
- Read the project rules in gads/PROJECT.md before changing anything, when that file exists. Agents that do not load it automatically must open it.
- Identify the relevant files, architecture, conventions, tests, and working-tree state.
- Make the smallest coherent change that satisfies the task.
- Do not overwrite unrelated user changes.
- Do not invent APIs, requirements, test results, or implementation details.
- Prefer existing dependencies and patterns; justify new dependencies.
- Keep secrets, credentials, tokens, and personal data out of source, logs, prompts, and reports.
- Run relevant checks and report exact commands and outcomes.
- Distinguish **passed**, **failed**, **not run**, and **blocked**.
- Do not claim a task is complete while required gates remain unverified.
- Update documentation when behavior, interfaces, operations, or decisions change.
- Use an ADR for durable architectural decisions.

## Autonomy boundaries

Agents may inspect, plan, edit, run local non-destructive checks, and prepare commits when authorized. Agents must obtain explicit approval before destructive operations, production changes, external publication, permission changes, financial actions, or irreversible data migrations.

See `gads/standard/SECURITY_AND_AUTONOMY.md` for the full risk model.

## Roles and templates

Specialist role definitions live in `gads/agents/`, one file per role. Agents that support delegation can adopt a role by reading its file. When acting in a role, start each message with the role's label, for example **[Reviewer]**, and keep that label when relaying another role's result. Without a role, use no label. Reusable report and decision formats live in `gads/templates/`, for example `gads/templates/TASK.md` and `gads/templates/REVIEW_REPORT.md`.
