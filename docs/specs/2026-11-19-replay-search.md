# Replay search: structured filters on the match archive

Date: 2026-11-19 · Status: **approved** by the operator in chat (2026-11-19)

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

## Operator decisions (2026-11-19)

The open questions were resolved as follows:

- Filters use existing stat columns only (no siege damage, damage taken or camps for now).
- Total count in an `X-Total-Count` response header; the body stays a plain array.
- The UI hero filter means "my hero" by default (the any-player filter stays available).

## Open questions (as proposed)

1. Are the 8 typed stats enough, or do you want siege damage / damage taken / camps too (would use the
   in-memory insights corpus instead of SQL, or a new typed-column migration plus a re-process)?
2. Total count via an `X-Total-Count` header (proposed, no shape change) — or a new `{items, total}` endpoint?
3. Should `hero` in the UI default to "my hero" (more intuitive) and move "any player" to the advanced panel?

## Implementation notes (2026-11-19, branch `feat/replay-search`)

- **Measurement.** The box DB copy was not used (no access from the implementation worktree). Instead a
  synthetic archive of 3,500 games, 10 players each (operator in every game), whose `matches.data` /
  `match_players.data` are the storm-stats output of a committed fixture replay (~6.9 kB TOASTed per player, as on
  the box). Release build, warm, 40 sequential requests per query; numbers in `docs/spec/01-architecture.md`.
  - Before (old query, 200-game page): p95 ~100 ms unfiltered, 157 ms with `result=loss&mvp=true`, 1,118 ms at
    `offset=3000` (the players JSON was built for every skipped row).
  - After: 82–96 ms for a 200-game page with any filter combination, 21 ms for `limit=50`, deep offsets as
    cheap as the first page, CSV 5–16 ms.
  - **The 50 ms target is missed for 200-game pages.** The floor is the page expansion: the `award` of each of
    the 10 players is read from `match_players.data`, detoasting ~7 kB per player (~0.35 ms per game). No index
    can avoid that read. Options for the operator: a typed `award` column (new migration + backfill, projection
    change), a smaller UI page (100 games: p95 43 ms unfiltered, 45–67 ms for most filters, measured while the shared host was busy), or accepting ~85–95 ms (under the 100 ms contract).
- **Migration 0018** (`match_players_mvp_idx`, partial index on MVP rows): added because the measurement of the
  existing MVP filter missed the contract (157 ms, it detoasted the operator row's JSON for every game). The filter
  is now an `EXISTS` on the same `(match_id, toon_handle)` row: 81 ms with the 200-game page.
- **Query shape.** The page is chosen on match IDs first, then expanded; the perspective row used by filters is
  one `DISTINCT ON (match_id)` pass without JSON (the per-game lateral of the old query cost ~15 ms over 3,500
  games); the count runs concurrently. Same-`played_at` ties are now ordered by match ID (stable paging).
- **CSV parity.** The CSV now also applies `player` and accepts `offset` (it ignored `player` before), so it
  returns exactly the list's games for the same query.
- **Validation.** Numeric bounds are parsed as numbers, clamped to ≥ 0 (stats ≤ 10¹², length ≤ 600 min);
  non-numeric / non-finite values, unknown stats and `with` = `against` (same toon) answer 400 `bad_filter`
  with `param`. Empty values count as absent. `min` > `max` is not rejected (empty result).
- **UI.** The URL uses the API parameter names (`my_hero`, `with`, `min_deaths`…). The main hero select is now
  `my_hero`; `hero` (any player) is in More filters, so existing `/matches?hero=` links (Hero page, Maps) keep their
  meaning and show as a chip. Compare has no "together/against" link between A and B (the archive's perspective is
  always the operator): each player card links to "your games with / against" that player.
- **Noise.** The host was shared with other builds: a rerun of the 200-game page at load average ~6.8 (8 cores) gave
  p95 up to 121 ms unfiltered and 98–114 ms with filters. A quiet-host run on the box DB copy is still needed.
- **Bundle.** App 1,009,761 bytes Brotli (+13,977 vs 995,784): above the current 1 MB cap, within the 1.25 MB cap of
  PR #53 (must merge first).

### Paging and validation follow-up

- Page and count now use one snapshot and one materialized set of filtered IDs; empty and out-of-range pages
  still include the count. The UI keys paging to filters, live imports, reference revisions and operator changes.
  It ignores stale successes and errors, counts consumed server rows independently of visible IDs, and can retry
  a failed offset or reload after no progress. Offset paging does not freeze the archive across HTTP requests.
- JSON and CSV reject repeated bounds and unknown `min_*`/`max_*` keys even when blank. A single blank known
  bound is still absent. Chips and Load more are native buttons.

The **200-row / <50 ms p95 contract is unchanged and the target remains open**. See the current latency
measurement in `docs/spec/01-architecture.md`. In an interleaved `psql` probe on the same synthetic archive,
materializing the count with JSONB was ~79 ms p95 for the first page (old page query alone ~77 ms); expanding
into JSON via `json_build_object` was ~65 ms, direct JSON record aggregation ~63 ms, and set-based player
aggregation ~63 ms. The direct record shape ships; its HTTP endpoint also avoids a Rust JSON round trip.
A second paired SQL-only probe measured 63.2 ms with the awards and 8.5 ms with `NULL::text AS award` (20 warm
samples each). The latter is a diagnostic, not a candidate response. No migration was added or edited.

Next decision: retain 200 and explicitly accept the measured latency, or approve a separate change to measure
materializing awards while preserving the 200-row response. Reducing the page size also changes the approved
contract and would require approval and a new worst-case measurement (as a diagnostic, 100 rows measured
44.1 ms for teammate + death bound and 47.9 ms for all typed bounds). A box DB copy was not used in this run.
