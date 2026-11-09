# Spec — 2D replay viewer (built into Storm Codex)

**Date:** 2026-07-09 · **Status:** design approved (brainstorm), ready for an implementation plan.
**Goal:** rebuild a **top-down 2D abstraction** of a HotS match from the replay data
already decoded, with a scrub bar with **instant seek** (< 50 ms), **built in
as a tab of the match detail** in the existing SPA. A tool to review positioning/decisions,
**not** a replacement for the in-game 3D client.

## Context & starting point
The source spec ("HotS 2D Replay Viewer") was written as if the project only had a "raw Rust
parser". In reality, Storm Codex is a complete product: server (replay archive = source of truth,
on-the-fly decoding + cache, Postgres, WS), React SPA (Nexus Codex design), vendored portraits/maps.
The viewer **integrates** into it instead of rebuilding that infrastructure.

**`storm-replay` already decodes, bit-exactly, all the streams needed**:
- `tracker_events()` → `SUnitPositionsEvent`, `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitRevivedEvent`,
  `SStatGameEvent`.
- `game_events()` / `visit_game_events()` → `SCmdEvent` / `SCmdUpdateTargetPointEvent` (with `TargetPoint`).
- `details()` / `attributes()` → map, players, mode.

The "Phase 1" work is therefore **not** decoding: it is an **extraction/projection layer**
that consumes the already-decoded events and produces the viewer's model.

## Ground truths (measured on a real replay — Silver City ARAM)
1. **Coordinate calibration = almost free.** The replay carries the map extent via
   `SStatGameEvent{ GameStart, MapSizeX, MapSizeY }` (e.g. 1,015,808 × 884,736). All coordinates
   are normalised within that box (tracker coords already in "tile" space ~0–250; game events'
   `TargetPoint` in the same space after dividing the fixed point ×4096). **So heroes can be placed
   correctly on a minimap with replay data alone** — no per-map bounds table,
   no manual alignment. The `jamiephan/HeroesOfTheStorm_Gamedata` gamedata is only used to provide
   the **minimap image** per map (+ an optional small crop if `MapSize` includes an unplayable border).
2. **Hero movement density = abundant.** ~20,831 `SCmdUpdateTargetPointEvent` + 15,847 `SCmdEvent`
   (each with `_userid` + a world `TargetPoint`) versus only 38 `SUnitPositionsEvent`. We anchor
   on the exact positions (rare) and densify with the command targets (frequent).
3. **No HP/mana data in the replay.** Enumeration of all tracker event types of a real
   match: `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitPositionsEvent`, `SStatGameEvent`, `SUpgradeEvent`,
   `SUnitOwnerChangeEvent`, `SUnitRevivedEvent`, `SUnitTypeChangeEvent`, `SScoreResultEvent`. **None**
   carries continuous health/mana per unit. HotS re-simulates the vitals — which is a **permanent non-goal**
   ("no engine re-simulation"). → **US-13 (HP/mana bars) dropped** (joins fog of war as
   "Won't have"). Instead: **alive/dead state per hero** (Born/Died/Revived, solid data) +
   **death markers**. `SUnitRevivedEvent` exists → softens the "revives glitch" warning.

## Locked decisions
1. **Built into Storm Codex** (not a standalone tool). Web = the existing SPA; exchange format = JSON
   over HTTP; generation **on demand + cache**, never materialised in bulk in Postgres.
2. **Seek 100 % client-side.** The server emits **the whole match timeline at once**; the scrub is
   a search + interpolation in browser memory. Payload budget: the ~37k command events do not
   all become samples — only movement `TargetPoint`s are kept (not every cast),
   near-stationary points are deduplicated and the normalised floats quantised (~3 decimals), which
   keeps the JSON in the order of a few hundred KB. →
   **the source spec's "snapshots every N s" become unnecessary in V1** (only useful for server
   seek or huge payloads — not applicable here).
3. **Data-driven calibration** (via `MapSize`), minimap images from `jamiephan`, fallback
   to the already-vendored map images / veiled gradient.
4. **Hero identity reused from Postgres**: the replay provides the unit→player link; the canonical
   name / portrait / universe colour come from the **existing match projection** (already corrected
   for the ARAM shuffle via `dim_talents`). The viewer never has to re-resolve the hero's identity.

## MVP-1 scope (thin vertical slice — de-risks calibration first)
Correct minimap background · 10 hero dots (fill = **team colour**; the portrait ring =
`universeColor`) + portraits · scrub bar with instant
client-side seek · **alive/dead dimming + death markers** (replaces HP/mana) · **no
play/pause animation yet**. Goal: prove coordinate calibration + instant seek on
real matches, before investing in animation/polish.

**Fast-follows** (after MVP-1 is validated): animated play/pause playback 0.5×–8× (remaining Phase 2);
alive/destroyed structures (data already in the tracker events — almost free); clickable kill feed;
talent/level indicators; then Phase 4 (per-map objectives) case by case.

## Architecture
- **New crate `storm-replay-viewer`** (workspace member), depends on `storm-replay` only.
  Consumes the decoded events → produces the viewer model. Rationale: `storm-stats` (faithful
  hots-parser port, under a green parity diff) is **not** polluted by positional extraction. Three clear
  roles: `storm-replay` decodes · `storm-stats` does stats parity · `storm-replay-viewer` projects the
  positions.
- **Server endpoint** `GET /api/match/{id}/replay2d` → JSON. Reuses the
  fetch-raw-archive + decoding path (the one behind `…/raw`) and the existing on-demand + cache model.
  Disk cache keyed by `(replay hash, viewer_version)`: the computation costs a parse (~130 ms), so the first
  opening computes, re-openings are instant (satisfies US-6). No new data "tier".
- **Front-end**: **"Replay 2D" tab in the existing match detail page** (no separate route).
  Rendered on a **`<canvas>`** (the right substrate for animation + minions/camps to come; avoids a
  rewrite). Reuses Nexus Codex tokens, vendored portraits, the `mapImage()` pattern (+ fallback).

## Data flow
match detail → "Replay 2D" tab → `fetch /api/match/{id}/replay2d` → the client holds the whole
model → the scrub bar drives a pure `seek(t)` function → canvas rendering.

## JSON model (emitted by the server; **coordinates pre-normalised to `[0,1]`**)
```
{
  meta:   { mapName, mapSize:{x,y}, durationSec, loopOffset:-610, viewerVersion },
  players:[ { playerId, userId, name, hero, team, universeColor } ],   // from the Postgres projection
  heroes: [ { playerId,
              samples:[ {t, x, y, exact} ],   // t=sec (offset-corrected), exact=true if SUnitPositionsEvent
              life:   [ {from, to} ] } ],      // "alive" intervals from Born/Died/Revived
  deaths: [ {t, x, y, victimPlayerId, killerPlayerId} ],
  warnings:[ "objective data unavailable: <map>" ]   // V1 stub, US-7 later
}
```

## Extraction rules
- **Time base (US-3)**: `t = (gameloop − 610) / 16`, applied once at the boundary.
- **Normalisation**: `mapSize` from `SStatGameEvent{GameStart}`; `x' = worldX / mapSizeX` (the game events'
  `TargetPoint`s are first divided by their fixed point ×4096 to land in the same
  space as the tracker coords). A single place owns the transformation.
- **Hero positions**: anchored on the exact `SUnitPositionsEvent` (`exact:true`); densified via
  `SCmdUpdateTargetPointEvent`/`SCmdEvent`.`TargetPoint` keyed by `_userid` (`exact:false`). The client
  interpolates linearly between samples (blink/dash artefact accepted by the source spec).
- **Unit→player link**: `SUnitBornEvent.m_controlPlayerId` + unit tags to track each player's hero
  unit; game events use `_userid.m_userId`. The hero's **name** does NOT come from the replay
  (immune to the shuffle) but from the Postgres projection.
- **Life state**: Born (spawn) → Died → Revived intervals. Dead hero = dimmed; death marker
  visible ~4 s of scrub around each death `t`.
- **Deaths (`deaths[]`)**: from the tracked hero unit's `SUnitDiedEvent` — `x,y` = `m_x`/`m_y`
  (normalised `[0,1]`), offset-corrected `t`, `victimPlayerId` = player owning the dead unit (via the
  unit→player link), `killerPlayerId` = `m_killerPlayerId`. `m_killerPlayerId = 0` or missing (environmental
  death / by a structure) → `killerPlayerId: null`.

## Client seek
Pure `seek(t)`: for each hero, binary search in `samples`, lerp between the two surrounding `t`,
alive/dead resolution by interval. Zero round trips → guarantees < 50 ms.
**Interpolation vs dead interval**: do not lerp across a dead→respawn gap (the respawn is at the
base, far from the place of death → the dot would slide across the map while dimmed).
V1 rule: during a dead interval, freeze the dot on the last alive position (no lerp),
then jump to the respawn position when back alive.

## Edge cases / errors
- Map without a minimap image → existing veiled gradient fallback (deterministic, silent); heroes
  still correctly placed via normalised coords.
- Replay not decodable / not archived → the tab shows "replay unavailable" (reuses the
  archive/reprocess status already exposed).
- Overscan of `MapSize`'s unplayable border → optional per-map crop constant; MVP-1 without it,
  judged by eye on real matches.

## Tests
- **Extraction** (crate): unit tests on the committed mini-corpus — offset maths, `[0,1]`
  normalisation bounds, segmentation of life intervals; a **golden-JSON** snapshot for a known replay
  (exclude `viewerVersion` from the comparison, or deliberately regenerate the golden on each version
  bump, so that the test does not become noisy with each extraction refinement).
- **Client**: unit test of `seek(t)`.
- **End-to-end**: visual check via the preview tools on a real backfilled match.

## Out of scope (reminder of the permanent non-goals)
3D rendering · fog of war / per-player vision · exact spell hitbox/geometry · live multiplayer ·
full engine re-simulation · **continuous HP/mana (data absent from the replay)**.

## Breakdown into batches (build order)
1. **`storm-replay-viewer` crate**: extraction → JSON model (meta, players via Postgres at the
   server level, heroes samples + life, deaths); unit tests + golden-JSON.
2. **`/api/match/{id}/replay2d` endpoint**: on-demand decoding path + `(hash, version)` disk cache.
3. **"Replay 2D" front-end tab**: fetch the model, minimap background (jamiephan + fallback), 10 dots +
   portraits, scrub bar, client `seek(t)`, alive/dead dimming + death markers.
4. **Visual check** on a real match; minimap alignment/crop if needed. → MVP-1 delivered.

Each following batch (animation, structures, kill feed, per-map objectives) can be delivered independently
without reopening the architecture.
