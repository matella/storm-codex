# Scouting reports — implementation plan

> Spec: `docs/specs/2026-09-26-rapports-scouting-design.md` (approved on 2026-09-26, part A
> confirmed by the operator: drafts of 3 custom replays conform).
> Steps as checkboxes (`- [x]`).

**Goal:** the operator drops the replays of an opposing team, storm-codex computes quantified
facts, produces an LLM pack (`.md` + `.xlsx`), and re-imports a JSON analysis in an imposed format
whose every claim is backed by the facts.

**Architecture:** server module `crates/storm-codex-server/src/scouting/` — **pure** core (zero
I/O, unit-tested like the draft engine) + a thin `api.rs` layer (sqlx/axum):

| File | Role | Pure? |
|---|---|---|
| `summary.rs` | storm-stats `Output` → compact `GameSummary` (teams, heroes, bans, picks, stats, levels, objectives) | yes |
| `side.rs` | roster candidates, ambiguity, target side (roster ≥ 3 → anchor → manual) | yes |
| `facts.rs` | `GameSummary[]` + sides + roster → `Facts` (sections + flat index `id → value`) | yes |
| `pack.rs` | `Facts` → Markdown (prompt + definitions + facts + response format) | yes |
| `xlsx.rs` | `Facts` → `.xlsx` workbook (rust_xlsxwriter, `save_to_buffer`) | yes |
| `analysis.rs` | LLM text → extracted JSON → validated (tolerant serde) → resolved evidence | yes |
| `api.rs` | routes, persistence, recomputation of the facts on each mutation | no |
| `prompt.md`, `analysis.schema.json` | embedded (`include_str!`) | — |

**Accepted deviation from the spec (documented in the spec in the same commit):** `scouting_games` gets a
`summary JSONB` column in addition to `data`. Recomputing the facts on each replay added only reads
the summaries (a few KB) instead of reloading N complete `match` objects (tens of MB in total for
a drop of 30 files); `data` remains the complete projection (re-process, part F).

**Tech:** axum 0.8, sqlx 0.8, serde; `rust_xlsxwriter` 0.93 (Context7 doc: `Workbook::new`,
`add_worksheet().set_name`, `write_with_format`, `save_to_buffer`); React 18 + TanStack v5 front-end,
vitest (env node).

## Global constraints

- **Isolation**: no existing query on `matches`/`match_players` changes; no write
  from the scouting module into these tables.
- A hero's role: `dim_heroes.role` (the parser's `gameStats.Role` is 0 for all players
  in custom games — checked on the 3 replays), loaded by `api.rs` and passed to the pure core.
- Ban codes → name: `storm_stats::constants::hero_attribute()`.
- Fact rules: rates always `k/n` + %, `low = n < 3`.
- Fact ids: `[a-z0-9_.]`, slug of the names (`Braxis Holdout` → `braxis_holdout`,
  `Anub'arak` → `anubarak`).
- No `unwrap()` outside tests, strict clippy, typed errors; UI in English, comments in
  French (at the time; English since 2026-11-09).
- DB tests: ignored without `DATABASE_URL` (`api_tests` pattern), run against the dev Docker Postgres.
- The operator's 3 replays (third-party games) **are not committed**: unit tests on
  synthetic data + the committed `Industrial District` replay; real check in dev.

## Task 1 — Migration + game summary (part B)

- [x] `migrations/0010_scouting.sql`: the spec's tables + `anchors JSONB`, `target_source TEXT`,
      `summary JSONB NOT NULL`; index `(report_id)`.
- [x] `summary.rs`: `GameSummary { date, map, build, length_s, winner, teams: [[Player;5];2],
      bans: [Vec<Ban{hero, phase}>;2], picks: [Vec<String>;2], first_pick, first_fort,
      first_objective, level10_time: [Option<f64>;2], level_diff: Vec<(start,end,diff)> }` with
      `Player { toon, name, hero, stats }` (K/D/A, HeroDamage, SiegeDamage, Healing, SelfHealing,
      DamageTaken, ExperienceContribution, TimeSpentDead, MercCampCaptures, KillParticipation).
      Tests: committed replay (shape, 10 players, fields present); ban `Crus` → `Johanna`.
- [x] `06-modele-donnees.md` updated; migration applied on the dev Postgres.

## Task 2 — Target side (part B)

- [x] `side.rs`: `candidates(games, anchors)` (≥ 50 % of the games, or of the anchor's games, on the
      anchor's side), `ambiguous` = candidates faced each other in the same game (batch of
      series against the same team → the anchor is required), `detect(teams, roster, anchors,
      manual)` → `Option<(side, Source)>`.
- [x] Tests: ≥ 3 from the roster; anchor fallback; manual kept; anchor on both sides → `None`;
      BO3 series against the same team without an anchor → ambiguous; with an anchor → roster = the anchor's 5.

## Task 3 — Facts (part C)

- [x] `facts.rs`: sections `overview`, `players` (`p1…pN`, core then substitutes, heroes, roles,
      averages /min), `maps` (record + 95 % Wilson, picks, bans made/received), `draft` (first
      pick rate, first-phase / middle bans, bans received, the team's first pick, role of the first pick),
      `flow` (first to 10, first fort, first objective, average level gap at 10/15/20 min,
      duration brackets, comebacks/collapses at ±2 levels), `games` (one row per game) +
      `index: BTreeMap<id, {label, text, k?, n, low}>`.
- [x] Tests: records, Wilson (reference values), exclusion of games without a side, substitutes,
      `n` everywhere, unique and well-formed ids.

## Task 4 — API + drop (parts B/C)

- [x] `api.rs`: the spec's routes (+ `PATCH /api/scouting/{id}/replays/{gid}` for the manual
      side); drop = archive `ARCHIVE_DIR/scouting/<sha256>` → parse (`parse_sem` pool,
      `spawn_blocking`) → summary → insert → recompute; every mutation → `recompute(report_id)`
      (`facts`, `facts_version + 1`, `target_team` of non-manual games).
- [x] `POST /api/admin/reprocess` covers `scouting_games` (stale `parser_version`) then recomputes.
- [x] Integration tests: cycle creation → drop → facts → deletion; **isolation**:
      `/api/heroes`, `/api/maps` identical before/after a drop; duplicate → `duplicate` status.
- [x] `05-api.md`, `04-serveur.md`.

## Task 5 — Pack (part D)

- [x] `prompt.md` (English), `analysis.schema.json`; `pack.rs`: prompt + definitions + table
      `pN` → name + facts as Markdown tables (id column) + schema + example + `report_id` /
      `facts_version`.
- [x] `xlsx.rs`: sheets README, Overview, Players, Heroes, Maps, Draft, Flow, Games.
- [x] Routes `pack.md` / `pack.xlsx` (Content-Disposition). Tests: the Markdown contains every id
      of the index; the workbook is generated (non-empty bytes, ZIP signature).

## Task 6 — Analysis import (part E)

- [x] `analysis.rs`: extraction (```json block, otherwise the first balanced object, ignoring the
      braces inside strings), validation (`format_version == 1`, `report_id`), tolerant serde
      structures, `resolve(analysis, index)` → evidence `{id, text}` + `unsupported`.
- [x] `PUT/DELETE /api/scouting/{id}/analysis`; `GET /api/scouting/{id}` returns the analysis with
      evidence resolved against the **current** facts + derived status (`empty/ready/analyzed/stale`).
- [x] Tests: fenced, surrounding prose, broken JSON → error and nothing overwritten, wrong `report_id` → refused,
      unknown id → `unsupported`, old `facts_version` → warning.

## Task 7 — Front-end (part E)

- [x] `api.ts`: types + hooks `useScoutingList`, `useScoutingReport`; pure helpers
      `web/src/scouting.ts` (`fmtRate`, `statusLabel`, `claimEvidence`) + tests vitest.
- [x] `pages/Scouting.tsx` (list + creation); `pages/ScoutingReport.tsx` (header with editable title,
      pack/import actions, Overview / Players / Draft / Maps / Games / Roster tabs, evidence
      chips, sequential drop with per-file status, import dialog with preview and
      overwrite confirmation). Nav tab "Scouting".
- [x] `08-frontend.md`. `npm test` + `npm run build` green.

## Task 8 — Real check + measurement

- [x] Dev server + front-end: report with the operator's 3 replays, one anchor (a player of the scouted team) → roster of
      the 5 expected, 3 games on the target side, facts consistent with the confirmed drafts.
- [x] Pack copied → JSON response (written by Claude in the session from the real pack, the LLM
      step being manual) → import → evidence shown; re-import → overwrite.
- [x] `GET /api/scouting/{id}`: p95 measured (20 requests) → `01-architecture.md`.
- [x] `STATUS.md`, commit, push.

Acceptance criteria D (Claude **and** ChatGPT on the real pack) and B (roster confirmed on a real,
larger opposing batch): **require the operator** — listed as still due in `STATUS.md`.
