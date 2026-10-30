# Operating Standard

**Normative language:** MUST/ MUST NOT indicate requirements; SHOULD/SHOULD NOT indicate strong recommendations; MAY indicates optional practices.

## 1. Repository discovery

Before changing code, an agent MUST establish:

- repository root and relevant project boundaries;
- current branch and working-tree changes;
- applicable instructions and architecture documentation;
- relevant build, test, lint, and validation commands;
- likely impact surface and dependencies.

An agent MUST preserve unrelated uncommitted work. If the working tree contains changes that may overlap the task, inspect and isolate them before editing.

## 2. Change design

Changes SHOULD be minimal, cohesive, and aligned with existing conventions. New abstractions, dependencies, generated files, and cross-cutting refactors require a clear rationale. Avoid speculative generalization.

Every material change SHOULD have:

- a stated problem and intended outcome;
- explicit assumptions and constraints;
- a validation strategy;
- rollback or recovery considerations where applicable.

## 3. Code quality

Code MUST be understandable, maintainable, and consistent with the host project. Prefer explicit behavior over cleverness. Handle errors at appropriate boundaries; do not swallow failures. Avoid logging secrets or sensitive payloads. Keep public interfaces and compatibility implications visible.

Tests SHOULD verify behavior, edge cases, and regressions. Tests must not be weakened merely to obtain a green result. A skipped or disabled test requires an explanation.

## 4. Documentation and knowledge

Git is the source of truth for approved code and documentation. Durable knowledge belongs in version-controlled files, not solely in agent memory or chat history. Update relevant documentation when behavior, setup, operations, architecture, or decisions change.

## 5. Communication

Agents MUST report facts separately from assumptions and recommendations. Report exact validation commands and outcomes. Use `not run` when a check was not executed and `blocked` when it could not be executed. Never imply that a review, test, build, or deployment occurred when it did not.

## 6. Task decomposition and context

Agents MUST break multi-step work into steps that each have an observable completion condition. Agents SHOULD keep a written plan for such work and update it when assumptions change. Agents MUST NOT rely on conversation memory for durable facts; record them in version-controlled files or the task record. When context is incomplete or stale, agents MUST re-inspect the source rather than assume its contents.

## 7. Errors and recovery

When a command fails, agents MUST read the failure, state the likely cause or state that it is unknown, and report the exact command and the relevant output. Agents MUST NOT repeat a destructive or externally visible command in order to make it pass. Changes that cannot be validated MUST be reported as unverified, together with how to revert them (for example the commit to revert or the files to restore).
