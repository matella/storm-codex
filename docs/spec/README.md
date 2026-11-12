# Living spec — Storm Codex

The project's **living** documentation: it describes the **current** state of the contracts (API,
data, events, invariants), not the history. It is the reference to read before changing a
behaviour, and to **update in the same commit** as the change.

## Maintenance rule (what keeps the spec "living")

> **Any commit that changes a contract documented here updates the matching section.**
> An added/changed endpoint → `05-api.md`. A migration → `06-modele-donnees.md`. A WS/Redis
> event → `07-evenements.md`. An SPA page/route → `08-frontend.md`. An env variable or
> a deployment step → `04-serveur.md` / `09-operations.md`.
> The code remains the source of truth for details; the living spec documents the **contract and
> the why** — if the two diverge, it is a doc bug to fix in the commit that reveals it.

## Documentation map (who does what)

| Location | Nature | Lifecycle |
|---|---|---|
| `docs/spec/` (**here**) | **current** contracts & invariants | living — maintained with every change |
| `docs/STATUS.md` | progress log, next step | living — updated at end of session |
| `docs/specs/` (dated) | design decisions approved by the operator | frozen — do not re-decide without them |
| `docs/plans/` | executed milestone plans | archives |
| `docs/research/` | evidence: benches, parity reports, references | archives (referenced here) |
| `docs/runbooks/` | step-by-step operator procedures | maintained as needed |
| `crates/*/README.md` | showcase + public contract of each crate (crates.io publication) | living |

## Contents

1. [Architecture & invariants](01-architecture.md) — components, 3 data tiers, perf budgets
2. [storm-replay](02-storm-replay.md) — replay decoder contract
3. [storm-stats](03-storm-stats.md) — stats contract, hots-parser parity, tolerances
4. [Server](04-serveur.md) — upload→parse→projection pipeline, env config, reference data
5. [API](05-api.md) — REST + WebSocket reference
6. [Data model](06-modele-donnees.md) — Postgres schema (migrations)
7. [Events](07-evenements.md) — WS `/ws` and Jarvis contract (Redis)
8. [Frontend](08-frontend.md) — SPA pages, OBS overlays
9. [Operations](09-operations.md) — dev, box deployment, CI, backups, pitfalls
10. [**Development workflow**](10-developpement.md) — **read before any change**:
    spec-first cycle, doc/proof checklist per type of change, definition of "done"

## Repo map

| Path | Contents | Status |
|---|---|---|
| `crates/storm-replay` · `storm-stats` | public libraries (decoding, stats) | active — publishable on crates.io |
| `crates/storm-codex-server` | server (API + front-end + overlays) + migrations | active |
| `web-rs/` | Rust front-end (Leptos → wasm, Trunk): app + OBS sources, built into `web-rs/dist` | active |
| `tools/` | `protocol_gen.py` (protocol tables), `crosscheck_streams.py` (decoding parity), `parity-harness/` (stats parity vs hots-parser) | tooling — required for evidence |
| `corpus/` | real test replays (`spike50` bench, `stats` parity) — **not committed** (gitignored, NAS/box) | local data |
| `spike/` | milestone 0 (go/no-go decoding spike, .NET/Python benches) | archive — do not touch |
| `scripts/` | `backup.sh` (pg_dump + retention) | active |
| `.github/workflows/` | `ci.yml` (clippy + Rust/web tests, service Postgres) · `publish.yml` (Docker image → ghcr.io on every `main` push) | active |
| `docs/` | see documentation map above | — |
