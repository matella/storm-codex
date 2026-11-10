# Milestone 3 — server + Postgres + backfill: acceptance criteria

**Date:** 2026-06-12 · **Status: DONE** (dev against a local Docker Postgres; box target at
deployment). Plan: `docs/plans/2026-06-12-jalon3-serveur-db-backfill.md`.
Code: `crates/storm-codex-server/`. client-rs adapted: `Hots-Overlay` repo (separate commit).

## Criteria (spec) and results

| Criterion | Target | Measured | Verdict |
|---|---|---|---|
| Archive archived and attempted | 100 % | 2781/2781 unique files (0 pending) | ✅ |
| Parsed | ≥ 99 % | **99.4 %** of parsable replays (2722/2739) | ✅ |
| Failures listed and classified (Admin) | yes | 59, all classified and legitimate | ✅ |
| 3-year backfill (~3,000 replays) | < 5 min | **1.8 min** (109 s) | ✅ |
| End of game → page up to date | < 5 s | **1.4 s** (WS, E2E test) | ✅ |
| Read API p95 | < 100 ms | **52 ms** (median 30 ms) | ✅ |
| PG write + WS push | < 200 ms | included in the 1.4 s end-to-end | ✅ |

## Real backfill (complete local archive)
Concurrent headless uploader (16 workers) to the release server + Docker Postgres — the client-rs
GUI being impractical to drive in CI; the client-rs code is adapted and compiled separately.

- **2823 files** → 2781 unique (42 content duplicates, `409`), all archived.
- **2722 parsed** (97.9 % raw), **59 classified failures**:
  - `too_old` 27 — pre-2015 alpha builds (the spec allows them to fail);
  - `unsupported_mode` 14 — brawls (`GameMode.Brawl`, outside V1 scope);
  - `computer_player` 1 — game with AI (out of scope);
  - `parse_failed` (-2) 17 — borderline replays (PlayerInit without handle…) that **hots-parser also rejects
    with the same status** (checked) → faithful parity, not a bug.
- Excluding the rejections by design (42), the parse rate of actually parsable replays
  is **99.4 %**.
- **All recent ARAM maps** (Silver City, Lost Cavern, Braxis Outpost, Industrial
  District) are parsed and projected (thanks to the milestone 2.5 extension) — ~30 % of the archive
  that would have been lost under strict parity.

## Bug found and fixed (what the criterion is for)
The first backfill revealed **1129 `projection` failures = Postgres deadlock**: 16 concurrent
workers doing UPSERTs on the `players` table, and since the archive owner is
present in **every** game, the transactions acquired row locks in different
orders → deadlock cycle. Fixes:
1. `players` UPSERT **outside the match transaction**, in standalone statements sorted by
   `toon_handle` (lock held ~1 ms instead of the whole transaction; best-effort).
2. **Retry on deadlock/serialisation** (40P01/40001) in `project_match`.

Effect: 1129 failures → 0, and duration **1190 s → 109 s** (×11). The "real backfill" criterion did
its job: it surfaced a real concurrency error invisible to unit tests.

## Architecture delivered (reminder)
axum 0.8 + sqlx 0.8 + tokio. 3 tiers: raw archive (source of truth) · Postgres (JSONB projection
+ promoted columns, `parser_version`, idempotent re-process) · `…/raw` dump + LRU cache.
Upload (token, archive first, parse pool = number of cores, ≤ 2 s / 202 semantics), WebSocket
`match.parsed`, REST (matches/match/player/heroes), admin (tokens/reprocess/health). Two-level
fingerprints (content + game, overlay compat).

## Limits / follow-ups
- Dev against Docker Postgres; **box deployment** (rsync + docker compose) at
  integration time (milestone 5 switchover). The `dim_heroes`/`dim_talents` reference data remains to be populated
  from the HotsPatchNotes API (milestone 4). Materialised views of hot aggregates: to add if the
  p95 degrades at larger volume (52 ms now, wide margin).
- The 17 -2 rejections and the 42 rejections by design can be retried via `POST /api/admin/reprocess`
  when storm-stats evolves (driven by `parser_version`).
