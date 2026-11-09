# Development workflow — development is driven from the docs

This file is the **working contract** for every contributor, human or AI. The golden rule:

> **No behaviour change without documentation in the same commit.**
> A diff that changes a contract (endpoint, schema, event, page, env, perf budget) without touching
> `docs/spec/` is an **incomplete change** — it does not get merged, it gets completed.

## Language

Docs, code comments, log and error messages, and commit messages are written in **English**.
Existing file names (some are French slugs, e.g. `04-serveur.md`) stay as they are so links keep
working. Already-applied SQL migrations are never edited, so their French comments remain.

## The cycle (spec-first)

1. **Sync up**: `git fetch origin` (the local clone is often behind — development happens from
   several machines), read `docs/STATUS.md` (where things stand), then the section(s) of
   `docs/spec/` touched by the planned change.
2. **Specify before coding**:
   - Change within the existing frame (new stat, new UI block, read endpoint…) →
     update the relevant `docs/spec/` section (or write it) **first or together with** the code,
     same commit.
   - **Design decision** (new capability, architecture change, scope, major UX) → dated spec in
     `docs/specs/YYYY-MM-DD-….md` + **operator approval before implementing**. Locked decisions
     (01-architecture) are not reopened without the operator.
3. **Code** within the specified frame — conventions: Rust 2021+, strict clippy, typed errors,
   no `unwrap()` outside tests; strict TS front-end; conventional commits; up-to-date library docs
   via the Context7 MCP (do not code axum/sqlx/TanStack… APIs from memory).
4. **Prove** (never just claim): tests + verification suited to the type of change
   (table below). A UI feature is verified in the browser on real data.
   Commands: `cargo test --workspace` (DB integration tests are skipped without
   `DATABASE_URL` — run them against the dev Docker Postgres; CI always runs them);
   `npm test --prefix web` (vitest, pure helpers of `api.ts`); `npm run build --prefix web`.
5. **Document the rest**: `docs/STATUS.md` (end of session); AI memory where relevant.
6. **Commit + push** (the operator has authorised pushing directly to `main` after each finished step).

## Checklist per type of change (what to update, what to prove)

| Change | Doc to update (same commit) | Minimum proof |
|---|---|---|
| REST/WS endpoint (add/change/remove) | `05-api.md` (+ `07-evenements.md` if event) | test or verified curl |
| SQL migration | `06-modele-donnees.md` | migration applied on the dev Postgres |
| Shape of the projection (`project.rs`, storm-stats output) | `06-modele-donnees.md` + bump `PARSER_VERSION` + `04-serveur.md` if pipeline | reprocess verified idempotent |
| Stats logic (`storm-stats/src/process/`) | `03-storm-stats.md` (any divergence → tolerances.json) | **parity diff green again** (`tools/parity-harness/`) |
| Decoding (`storm-replay`) | `02-storm-replay.md` | heroprotocol crosscheck + mini-corpus |
| Front-end page/route/overlay | `08-frontend.md` | `npm test` + `npm run build` green + browser check on real data |
| Pure helper in `web/src/api.ts` | — (if the UI contract does not change) | vitest test in `api.test.ts` |
| Env variable, compose, Dockerfile, CI | `04-serveur.md` / `09-operations.md` | build/deployment tested |
| WS or Jarvis event | `07-evenements.md` | event observed (WS or Redis) |
| Perf affected (parse, API, backfill) | budget re-measured in `01-architecture.md` | matching bench |
| New draft format, setting, reference data | `04-serveur.md` + relevant section | engine test (draft = TDD, zero I/O) |

Cases with no doc to touch: pure refactor without contract change, typo, dependency bump without
API change — the commit says so explicitly ("no contract changed").

## Definition of "done"

- Measurable acceptance criterion verified (not "it should work").
- Tests green (`cargo test`, front-end build); parity/bench where relevant.
- `cargo fmt --all` applied (CI fails on `cargo fmt --check`).
- `docs/spec/` up to date; `docs/STATUS.md` up to date at end of session.
- Pushed to `main`; if the user asks, deployed on the box **and verified over HTTP**
  (bundle hash changed, `/api/health` 200).

## What a contributor (AI or human) does NOT do without the operator

- Reopen a locked decision (01-architecture) or re-decide an approved dated spec.
- Break a perf budget or introduce an undocumented parity divergence.
- `cargo publish`, creating public repos, switching over/decommissioning services on the box.
- Touch neighbouring services on the box (Jarvis, HotsPatchNotes, Node overlay) beyond reading.
- Modify an already-applied migration (always a new migration). CI rejects it
  (`scripts/check-migrations.sh`). If a **dev** database applied an intermediate version
  of a migration still in a PR, repair it (schema + checksum in `_sqlx_migrations`)
  rather than rewriting history.
