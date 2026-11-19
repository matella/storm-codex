# Session review: one play session against the player's usual baseline

Date: 2026-11-19 · Status: **proposed** (awaiting operator approval) · Branch `docs/specs-session-card-search`

## Goal

After an evening of games, answer "how did that go compared to how I usually play?" on one page: record,
heroes and maps played against the player's own baseline, best and worst game, streaks, time played. Works
the same on the box and in the standalone app, offline, on the operator's own data only.

## Current state

- **Session page** (`/`, `web-rs/app/src/pages/session.rs`, spec `docs/spec/08-frontend.md` route `/`): fetches
  `GET /api/matches?limit=500`, keeps the operator's games client-side (`core::dashboard::my_games`: verified
  toons first, then `operator_names`), and takes **only the latest session** (`core::dashboard::last_session`:
  consecutive games whose `played_at` are < 3 h apart). Shows the W–L record, pips, last game, win rate,
  summed K/D/A, MVP count, heroes played, the session's game cards, then "Your form" (last 30 results,
  last 10, per mode, top 6 heroes over the 500 loaded games).
- **Missing**: no comparison with a baseline (every number is absolute), no per-map view, no best/worst game,
  no streaks, no session wall-clock span, no way to open an older session, and the 500-game window silently
  caps "Your form".
- **Insights corpus** (`crates/storm-codex-server/src/insights/cache.rs`): in-memory extract of every game
  (`Game {id, map, mode, played_at, winner, players: [PlayerRow {toon, name, hero, team, win, stats[12]}]}`,
  the 12 record stats incl. kills, deaths, assists, hero/siege damage, healing, XP, camps), refreshed
  incrementally; warm reads 4–35 ms on 3,480 games. Personal insights (`insights/api.rs::player_insights`)
  already compute per-map / per-hero win rates from it.
- **"Me"**: `standalone::operator_toons` (verified handles) + `app_settings.operator_names`; `/api/matches`
  resolves the perspective row with the same rule (`read.rs::list_matches`, lateral `me`).

## Design

Server-side, read-only, on the insights corpus. No migration, no new dependency.

- **Session boundary**: the operator's games sorted by `played_at`; a new session starts when the gap between
  two consecutive game *starts* exceeds 3 h (same rule as today's `last_session`, so the cover does not
  change meaning). Games without `played_at` are ignored. A session is identified by its first match id.
- **Perspective**: per game, the perspective row is chosen exactly like `/api/matches` (verified toon first,
  then a name in `operator_names`, ties by team then row id). Optional `account` restricts to one account.
- **Baseline**: the operator's games in the **90 days before the session start** (session games excluded),
  same `account` restriction. Reported: win rate, per-game averages of K/D/A, hero damage, siege damage,
  healing, XP, deaths; per hero and per map win rate and games. Rows under 5 baseline games are flagged
  `small_sample`; the whole baseline under 20 games sets `small_sample` at the top (same wording as F7).
- **Per game** (session): hero, map, mode, result, length, K/D/A, award, and the deltas of the 7 stats vs
  the baseline average of **the same hero** when it has ≥ 5 baseline games, otherwise vs the overall average.
- **Best / worst game**: score = number of the 7 stats above that hero baseline minus number below
  (ties: win first, then KDA ratio). Best = max, worst = min; omitted when the session has 1 game.
- **Streaks**: longest win streak and longest loss streak inside the session, plus the current streak at the
  session's end. **Duration**: wall-clock span (first start → last start + last length) and summed game time.
- Pure logic (`insights/session.rs`: splitting, baseline, scoring, streaks) with unit tests; the handler only
  reads the corpus and settings.

## API / UI

- `GET /api/sessions?account=&limit=` → `{sessions:[{id, start, end, games, wins}]}`, newest first,
  `limit` default 20, max 100. `id` = first match id of the session.
- `GET /api/sessions/{id}?account=` → `{id, start, end, span_s, played_s, games:[{match_id, played_at, hero,
  map, mode, win, length, kills, deaths, assists, award, deltas:{stat: value}, baseline:"hero"|"overall"}],
  record:{wins, losses}, streaks:{best_win, worst_loss, current}, best, worst (match ids or null),
  heroes / maps:[{name, games, wins, base_games, base_wins, small_sample}], baseline:{from, to, games, wins,
  averages:{stat: value}, small_sample}, prev, next}` (`prev`/`next` = neighbouring session ids). 404 if `id`
  is not the first game of a session for that perspective; 400 `account` on an unknown account.
- **UI** (Session page, `/`, optional `?s=<id>`): the cover keeps its current content; a new "Review" section
  between the cover and the game cards: record vs baseline win rate, one row per hero and per map with
  session W–L next to the baseline win rate, stat deltas (green above / red below baseline), best and worst
  game cards (reuse `GameCard`), streak and duration tiles. "‹ Previous session / Next session ›" in the
  kicker. "Your form" stays as it is. The page reads the new endpoint
  instead of computing the session client-side; `last_session` stays for the OBS sources.

## Perf & budget impact

- Both endpoints read the warm corpus: target **p95 < 20 ms** on the box DB copy (3,480 games), measured,
  well under API p95 < 100 ms. Cold corpus: same behaviour as records (waits for the background load).
- No new memory (reuses the corpus). App bundle: one section and two wire types, estimated +10–15 kB Brotli
  (to measure; needs the cap raise to 1.25 MB since ~4 kB are left under 1 MB).

## Tests / acceptance

- Unit: session split at exactly 3 h / 3 h + 1 s, games without date, baseline window excludes the session,
  hero vs overall fallback, best/worst ties, streaks, single-game session.
- Endpoint tests (`api_tests::{test_state, call}`) on fixture replays: list, review, prev/next, 404, `account`.
- Browser check of `/` and `/?s=` against a box DB copy; numbers for 2 real sessions cross-checked by hand.
- Measured latency recorded in `docs/spec/01-architecture.md`. Contracts updated: `05-api.md`, `08-frontend.md`.

## Out of scope

Damage or healing per time window (replays store them as end-of-game totals only), comparing with other
players or a global meta, notifications or end-of-session pop-ups, OBS session sources, writing anything.

## Open questions for the operator

1. Baseline = last 90 days before the session, all modes — or same modes as the session only?
2. Best/worst game = "most stats above my hero average" (proposed) — or simply highest / lowest KDA ratio?
3. Keep the 3 h gap between game starts, or make it a setting (e.g. 2–6 h)?
