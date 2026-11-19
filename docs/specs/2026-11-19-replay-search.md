# Replay search: structured filters on the match archive

Date: 2026-11-19 · Status: **proposed** (awaiting operator approval) · Branch `docs/specs-session-card-search`

## Goal

Find games like "where I died more than 8 times on Braxis Holdout", "with player X on my team", "as Jaina in
ranked last month, lost, under 15 minutes" from the Matches page. Structured filters only (no free-text or
natural-language parsing), local, same on the box and in the standalone app.

## Current state

- **Server** `GET /api/matches` and `GET /api/matches.csv` (`crates/storm-codex-server/src/read.rs`,
  `MatchFilter`, `list_matches` / `matches_csv`; contract `docs/spec/05-api.md`): `map`, `mode`, `hero` (**any
  player** in the game), `player` (toon present in the game, either team), operator perspective `account`,
  `result` (`win`/`loss`), `mvp`, `from`/`to` (ISO, `to` excluded), `limit` (default 50, max 200; CSV max
  50,000) / `offset`. Response = a bare JSON array of match summaries with `perspective_toon` and 10 players
  (toon, name, hero, team, win, kills, deaths, takedowns, award). One SQL query; perspective row via a
  lateral join (verified toons, then `operator_names`).
- **UI** `/matches` (`web-rs/app/src/pages/matches.rs`, `core::dashboard::MatchFilter`): filters in the URL
  (back button safe): mode pills, W+L / Wins / Losses, MVP, map and hero search-selects, account select (when
  > 1), from/to dates, reset, CSV/JSON export. Shows at most the latest 200, no paging, no `player` filter.
- **Gaps**: no "my hero" (the hero filter matches any of the 10 players), no teammate/opponent filter in the UI
  and no same/enemy team distinction, no stat thresholds, no duration bounds, no way past 200 results.
- **Data** (`migrations/0001_init.sql`): typed columns on `match_players`: `kills` (SoloKill), `takedowns`,
  `deaths`, `hero_damage`, `healing`, `experience`, plus `apm` (0016); `matches.length` (s). Indexes:
  `matches(map)`, `(mode)`, `(played_at DESC)`, `match_players(toon_handle)`, `(hero)`, `(match_id)`, and the
  `UNIQUE (match_id, toon_handle)` index. Other stats (siege damage, damage taken, camps…) live only in
  `match_players.data` JSON, whose per-request scan was measured at 12 s for 3,480 games (local insights spec).

## Design

Extend the existing `MatchFilter` (`read.rs`). The new parameters apply to `/api/matches` and
`/api/matches.csv` only (the Heroes/Maps aggregates that share the struct ignore them). All are optional and
AND-ed; omitted = today's behaviour.

| Parameter | Meaning |
|---|---|
| `my_hero` | hero played by the perspective row (existing `hero` keeps "any player") |
| `with` / `against` | toon on the perspective player's team / on the other team (needs a perspective) |
| `min_<s>` / `max_<s>` | inclusive bounds on the perspective row, `s` ∈ `kills`, `deaths`, `takedowns`, `assists` (= takedowns − kills), `hero_damage`, `healing`, `xp`, `apm` |
| `min_len` / `max_len` | game length in minutes (inclusive, on `matches.length`) |

- "Died more than 8 times on Braxis" = `map=Braxis Holdout&min_deaths=9`. The UI shows "≥" explicitly.
- Stats are restricted to the **typed columns**: they are filtered in SQL without touching JSON. Siege damage,
  damage taken, camps, CC time are out of this spec (see questions).
- Stat bounds, `my_hero`, `with`, `against` need a perspective row (any operator account, or `account`);
  without a configured operator they answer 400 `perspective_required`. Invalid numbers → 400 `bad_filter`
  with the parameter name; unknown stat names are rejected, not ignored.
- **Count and paging**: the response stays a bare array (the Session page and core helpers consume it); the
  total comes in a `X-Total-Count` header (same WHERE, `count(*)`). The UI pages with `offset` ("Load more").
- Filter parsing/validation lives in `read/filter.rs` with unit tests; the SQL grows the WHERE clause only.

## Indexes

No migration. The perspective lateral join and the `with`/`against` lookups go through the existing
`UNIQUE (match_id, toon_handle)` index and `match_players(toon_handle)`; stat bounds are evaluated on the one
perspective row per game. A partial or composite index (e.g. `match_players (toon_handle, match_id)
INCLUDE (team, deaths, …)`) is added in a new migration **only if** the measurement below misses the target;
existing migrations are never edited.

## API / UI

- API: the table above, on `/api/matches`, `/api/matches.csv`; contract text in `docs/spec/05-api.md`.
- UI `/matches`: a "More filters" disclosure under the current filters: My hero (search-select), Played with /
  Played against (player pickers reusing `/api/players/search`), one stat row "stat [select] ≥ [n] ≤ [n]"
  (one stat at a time in the UI; the API accepts several), length min/max. All in the URL like today; active
  advanced filters appear as removable chips. Header shows "N games" from `X-Total-Count`; "Load more" pages
  by 200. Entry points: Player page and Compare "games together/against" link to `/matches?with=` /
  `?against=`.

## Perf & budget impact

- Target **p95 < 50 ms** for `/api/matches` with any combination of filters on the box DB copy (3,480 games,
  release build), including the count; contract API p95 < 100 ms. Today's query is unmeasured: measure it
  first as the baseline, then the worst combinations (`with` + stat bound + no map/mode).
- App bundle: filter panel + parsing, estimated +8–12 kB Brotli (to measure; needs the cap raise to 1.25 MB).

## Tests / acceptance

- Unit: filter parsing (bounds, inclusive edges, assists derivation, unknown stat, `with` = `against` both set).
- Endpoint tests on fixture replays: each new parameter alone, combined, without operator (400), CSV parity
  with the list for the same filters, `X-Total-Count` vs `offset` paging.
- Browser: the Braxis example and a with/against example on a box DB copy, URL round-trip via back button.
- Latency recorded in `docs/spec/01-architecture.md`; contracts in `05-api.md` and `08-frontend.md`.

## Out of scope

Free-text or natural-language queries, saved searches, JSON-only stats (siege, damage taken, camps, CC),
filters on talents, draft order, time-windowed events (damage per window is not in the replay), team
composition queries (e.g. "against double support"), stat bounds on players other than "me".

## Open questions for the operator

1. Are the 8 typed stats enough, or do you want siege damage / damage taken / camps too (would use the
   in-memory insights corpus instead of SQL, or a new typed-column migration plus a re-process)?
2. Total count via an `X-Total-Count` header (proposed, no shape change) — or a new `{items, total}` endpoint?
3. Should `hero` in the UI default to "my hero" (more intuitive) and move "any player" to the advanced panel?
