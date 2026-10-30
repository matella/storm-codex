# Workflow and Quality Gates

## Standard lifecycle

1. **Intake:** clarify outcome, scope, constraints, acceptance criteria, and risk.
2. **Discovery:** inspect repository, policy, architecture, and working-tree state.
3. **Plan:** decompose work; identify dependencies, files, tests, and approvals.
4. **Implement:** make scoped changes; keep diffs reviewable.
5. **Verify:** run relevant tests, static analysis, build, and security checks.
6. **Review:** inspect the diff and challenge assumptions, edge cases, and regressions.
7. **Document:** update user-facing, developer, and operational documentation as needed.
8. **Handoff:** report evidence, limitations, risks, and next actions.

## Required gates

| Gate | Minimum evidence | Failure behavior |
|---|---|---|
| Scope | Acceptance criteria and touched-area summary | Clarify or narrow |
| Safety | Risk tier and approvals for R3+ | Stop pending approval |
| Quality | Relevant tests/build/lint outcomes | Fix or disclose |
| Review | Diff inspection and known-risk review | Resolve material findings |
| Release | Version, change summary, evidence, rollback | Do not claim release-ready |

## Test selection

Run the narrowest meaningful tests first, then broader checks when practical. Select tests based on changed behavior and dependency impact. Report skipped suites and environmental blockers. A green unit test suite does not prove integration, security, performance, or production readiness.

## Completion states

- **Complete:** acceptance criteria met and required gates passed.
- **Complete with limitations:** core task done; non-blocking checks remain unverified and are clearly listed.
- **Blocked:** required action depends on missing information, access, approval, or environment.
- **Not complete:** acceptance criteria are not met.

## Release evidence

A release report should record commit SHA, version, changed areas, commands executed, results, skipped checks, known risks, approvals, and rollback plan. Evidence is descriptive; the release gate script checks structure only and cannot authenticate it.
