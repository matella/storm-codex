# Batch import of a cast series' replays — design

**Status**: approved by the operator (2026-10-31) — decisions 1 to 3 below settled.
**Date**: 2026-10-31
**Area**: cast (`crates/storm-codex-server/src/cast/`, `web/src/cast/`), spec `docs/spec/05-api.md` §Replay of a game.

## Context

Today, each game's Custom replay is uploaded once the game has been started and linked to its series
row (`POST /api/cast/games/{n}/replay`, one file per game). For a series of 3 to 5 games, that is
one round trip per game, after the fact.

## Goal

When creating a series, offer the operator:

- **Manual** (default): current behaviour, one file per game.
- **Batch**: drop all the series' `.StormReplay` files at once; the server proposes the
  game ↔ file assignment; the operator confirms or corrects before anything is written.

## Out of scope

- No folder reading on the server side (the browser sends the files, as today).
- No database writes (`matches`, `uploads`): only the `cast_live` row changes, like the current route.
- No change to the orientation rule (R14/R16) or to the `GameSummary` format.

## Points to check before implementation

1. **Map**: `GameSummary.map` exists (`cast/mod.rs`, field `map`). Used to attach a replay to a pool map.
2. **Date / order**: the summary does not carry the game date so far. To be checked in `storm-stats`
   (`process_replay`); otherwise, the order defaults to upload order, to be confirmed by the operator.
3. **Creating games**: a game only exists after `POST /api/cast/games/start` (map chosen in the pool).
   Batch mode must therefore either create the games from the replays (replay map ∈ chosen pool map),
   or only attach to games already started. **Proposed choice**: create the games (complete view right away).

## Proposed behaviour

1. `POST /api/cast/series` unchanged; the creation screen adds the Manual / Batch choice.
2. Batch mode, after the teams step:
   - `POST /api/cast/series/replays/preview` (multipart body or list of bytes + names): decodes each file
     (same path as `cast/replay.rs::decode`, temporary files deleted), returns for each one
     `{ filename, map, winner, orientation, proposed_game }` without writing anything. `proposed_game` = the matching
     pool map (status `picked`/`decider`), in upload order when there is no date.
   - Per-file errors (`not_custom`, `parse_failed`, `incomplete`) shown without blocking the others.
3. The operator corrects the assignment on screen, then confirms.
4. `POST /api/cast/series/replays/commit`: for each confirmed assignment, creates the game if needed then applies
   `apply_to` (same lock, same `series_changed`/`side_mismatch`). Response: result per file.
   Atomicity: if an assignment fails, the previous ones stay (each game is independent); the result
   says so explicitly.
5. `cast.updated` broadcast as today; draft sync unchanged.

## Constraints carried over from the spec

- Decoding under `parse_sem`, temporary file guaranteed to be deleted (`TempFile` guard).
- Budget: ~180 ms per replay in release → 5 replays < 1 s, to be measured (hard rule 4, no estimate).
- Body limited like the other replay routes; no new implicit limit.

## Expected tests

- Preview: assignment by map; non-Custom file reported without blocking the others; nothing written to the database.
- Commit: game created then filled; `series_changed` if the series changes between preview and commit;
  `side_mismatch` reported per file.
- Front-end: Manual/Batch choice at creation; manual correction of the assignment; Manual unchanged (no regression
  of the existing `SeriesWorkspace` tests).

## Decisions (approved)

1. **Create the games from the replays**: each file creates its game (pool map `picked`/`decider`, not played yet).
2. **Order**: game date if the replay provides it, otherwise upload order.
3. **Partial commit** accepted: each game succeeds or fails on its own, the result is returned per file.

## Amendment of 2026-11-01 (approved by the operator)

Findings from the review of `main` (merge #10) and the operator's decisions. What follows replaces the
corresponding passages above.

1. **Routes delivered**: instead of a batch `preview` and `commit`, two **per-file** routes:
   - `POST /api/cast/replays/inspect`: decodes without writing anything, returns the `GameSummary` (map, date).
   - `POST /api/cast/replays/import?map=…&series_id=…[&swap=1]`: creates the game for the map then fills it,
     in one mutation. A failure only cancels its own file.
2. **`GameSummary` format**: it gains `map` and `played_at` (`null` if missing). Old states load
   with `null`. The sentence "no change to the format" no longer holds.
3. **Sort by date**: done client-side (`replayImportOrder`), from `played_at`; otherwise, upload order.
4. **Series guard**: the import carries the identity of the series read at inspection (`series_id`). If the series has
   changed, `409 series_changed`, nothing is created. The client clears the batch when the series changes.
5. **"Swap sides" option per replay** (operator decision, 2026-11-01): it is offered in the batch,
   and passed to the import (`?swap=1`), as in the per-game route.
6. **Failed replay**: it no longer blocks its map. Changing its map makes it ready to import again.
7. **Error texts**: `series_changed`, `map_mismatch`, `MapNotChosen`, `UnknownMap`, `SeriesInactive`,
   `not_custom`, `parse_failed`, `incomplete` have readable text (`replayErrorText`).
8. **Displayed date**: suffixed with "UTC".
