# Project Rules — Storm Codex

Project-specific rules. GADS loads them via `CLAUDE.md` (`@gads/PROJECT.md`); `AGENTS.md` points other agents here. Content carried over from the former `CLAUDE.md`.

## Commands (same as CI)

- Rust: `cargo clippy --workspace --all-targets -- -D warnings` then `cargo test --workspace`.
- Front-end (in `web-rs/`): `cargo clippy -- -D warnings` + `cargo test` (native, `core`), `cargo clippy --target wasm32-unknown-unknown -- -D warnings`, `trunk build --release` in `overlay`, `sources` and `app`.

> Loaded at every session. **Development is driven from the documentation** — in order:
> 1. `git fetch origin` (the local clone is often behind — several machines push);
> 2. `docs/STATUS.md` — state + next step (update it at the end of the session);
> 3. `docs/spec/` — **living spec**: read the section of the affected area BEFORE changing anything;
>    `docs/spec/10-developpement.md` is the working contract (spec-first cycle, checklist).
> The programme spec (`docs/specs/2026-06-12-storm-codex-design.md`, + dated follow-ups) is approved
> by the operator: do not re-decide its decisions without them.

## Rule no. 0 — no change without documentation

It is **forbidden** to modify a file that carries a contract (endpoint, migration, projection,
event, page/route, env, CI, perf budget) without updating the matching `docs/spec/` section
**in the same commit**. Mapping and required evidence per type of change:
table in `docs/spec/10-developpement.md`. A code diff without its doc diff is an
**incomplete** change — complete it before committing. Only exception: refactor/typo
strictly without contract change, and the commit message says so explicitly.
Any **design decision** (new capability, scope, architecture) first goes through a
dated spec in `docs/specs/` approved by the operator — the AI does not implement it on its own
initiative.

## Language

Docs, code comments, log and error messages, and commit messages are written in **English**.
The UI is in English too. Existing file names stay as they are (some are French slugs) so links keep
working, and already-applied SQL migrations are never edited, even for comments.

## What the project is
A complete renovation of Stats of the Storm (HotS stats tracker): open-source Rust crates
(`storm-replay` decoding, `storm-stats` analysis), a single `storm-codex-server` server (axum +
Postgres + WS) on the box, a Rust (Leptos, wasm) SPA with the Nexus Codex design system, an OBS stream widget, Jarvis
integration (Redis events) and the existing `client-rs` uploader (Hots-Overlay repo).

## Hard rules
1. **One plan per milestone** (0→6 in the spec), in order; each milestone has a measurable
   acceptance criterion — it is only "done" once the criterion is verified.
2. **Milestone 0 first**: go/no-go spike (decode 3 real replays in Rust, bench 50 files vs
   .NET/Python; accept < 500 ms/replay). If no-go → documented .NET fallback, same architecture.
3. **Stats parity proven, not claimed**: storm-stats is validated by an automatic diff against
   hots-parser (Node) on a reference corpus. No milestone 2 without a green diff.
4. **The spec's perf budgets** = contracts: parse < 150 ms/replay, end of game → page < 5 s,
   API p95 < 100 ms, 3-year backfill < 5 min. Measure, do not estimate.
5. **3 data tiers**: archived raw replay (source of truth); Postgres = complete
   projection (`parser_version` everywhere, idempotent re-process); dump decoded on the fly + LRU
   cache. Never any mass pre-decoding.
6. Events to Jarvis: spine invariants (schema_version, correlation_id/causation_id,
   occurred_at/recorded_at, past-tense `entity.verb`).
7. The hero/talent reference data comes from the HotsPatchNotes API (replicated into `dim_*` at startup) —
   no second sync pipeline.

## Environment
- Box: matella@192.168.129.85 (evening server ~6pm→night); Jarvis Postgres/Redis on it;
  HotsPatchNotes api :5001 / web :5100; Node overlay :8086 (to be decommissioned at milestone 5,
  the Twitch Azure EBS stays). Deployment: rsync + docker compose build (no `git pull` on the box).
- Windows gaming PC: replays in `Documents/Heroes of the Storm/Accounts/*/*/Replays/Multiplayer`
  (~3,000 files, 2–3 GB); uploader = `client-rs` (Hots-Overlay repo).
- Test corpus: NAS/box + `fetch-corpus` script; mini-corpus committed for public CI.

## External repos — what to clone, where, and when
This repo is **self-sufficient for milestones 0→2** (spike, storm-replay, storm-stats): no
local dependency on the other projects. Milestone 0 is ideally done **on the Windows gaming PC**
(the replays are there; the Rust/.NET/Python toolchains are installed there).

| Repo | Where | Needed for |
|---|---|---|
| ebshimizu/hots-parser | public GitHub (npm) | milestone 2 — parity diff harness (clone anywhere) |
| ebshimizu/stats-of-the-storm | public GitHub | functional reference (already summarised in docs/research) |
| Blizzard/heroprotocol | public GitHub | generation of the protocol tables (milestones 0–1) |
| matella Hots-Overlay | GitHub (private) | milestone 3 — adapt `client-rs` (backfill mode, re-pointing) |
| matella HeroesOfTheStormPatchNotes | GitHub (private) | milestone 4 — reference data consumed **via the box's API** (:5001, Tailscale); the clone is only useful to read the code |
| Jarvis | **not on GitHub** (box remote only) | milestone 5 — the connector is written from the Mac/box; nothing to clone on the PC |

Box access from anywhere: Tailscale (subnet route active) → 192.168.129.85 (SSH + APIs),
in the evening when the box is running. Milestones 0–2 do not need the box at all.

## Conventions
Rust 2021+, strict clippy, typed errors (`thiserror`), no `unwrap()` outside tests;
Leptos 0.8 (CSR, wasm, Trunk) front-end in `web-rs/`, Nexus Codex tokens; conventional commits;
secrets only in the box's .env, never committed.

**Up-to-date documentation is mandatory — Context7 MCP.** Before using a library or
framework API (axum, tokio, sqlx, serde, egui, leptos, leptos_router, Trunk, wasm-bindgen…), query the
**Context7** MCP (`resolve-library-id` then `query-docs`) for the current docs — do not rely on the
model's memory (moving versions/APIs: axum and leptos in particular). Applies to
syntax, config, version migrations and library-specific debugging. If Context7
is not connected in the session, say so rather than coding from memory.
