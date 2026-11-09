# 2D viewer — Fast-follows (Phases 2–5) — Implementation plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (fresh subagent per batch + two-stage review). Steps use checkbox (`- [ ]`).

**Goal:** Complete the 2D viewer beyond MVP-1: animated playback, structures, kill-feed, ability/talent/level indicators, per-map objectives, minion toggle, clip export — everything the spec covers beyond V1.

**Base:** MVP-1 shipped + deployed (crate `storm-replay-viewer`, endpoint `/api/matches/{id}/replay2d`, front tab `web/src/components/Replay2D.tsx` + `web/src/replay2d.ts`). These batches **extend** those three units following the same patterns.

**Spec :** `docs/specs/2026-07-09-visionneuse-2d-replay-design.md` (Phases 2–5, US-11, US-16→US-27).

**Ground truths (measured on a real replay — see recon 2026-07-09):**
- Structures: born `KingsCore`(core), `TownTownHallL{2,3}`(fort/keep), `TownCannonTowerL{2,3}`(tower), `TownWallRadial*`(wall), `TownGate*`(gate), `m_controlPlayerId` **11=team0 / 12=team1**; destruction = the tag's `SUnitDiedEvent`; `SStatGameEvent{TownStructureInit}`.
- Talents: `SHeroTalentTreeSelectedEvent {_userid.m_userId, m_index, _gameloop}` (pick, order = tier); `SStatGameEvent{TalentChosen}` and `{LevelUp}` (timestamped). Names via `dim_talents` (populated on the box).
- Abilities: `SCmdEvent` with a non-null `m_abil` (`m_abilLink`), key `_userid.m_userId`.
- Kill-feed: `deaths` (already extracted) + `SStatGameEvent{PlayerDeath, JungleCampCapture}` + structure deaths.
- Minions/camps: `SUnitPositionsEvent` also carries non-hero units (damage-gated, sparse).

**⚠️ Branch:** `git switch -c feat/replay2d-fastfollows` (not on `main`). Each batch commit + review before the next one.

**`VIEWER_VERSION` bump reminder**: as soon as a batch changes the model's **JSON schema** (new fields), increment `VIEWER_VERSION` in `crates/storm-replay-viewer/src/model.rs` (invalidates the disk cache `{id}-replay2d-v{N}.json`) and regenerate the golden (`UPDATE_GOLDEN=1`).

**Build order = decreasing value/effort, each batch shippable independently.**

---

## Chunk 1 — Animated playback (Phase 2, US-11) · FRONT ONLY

**Responsibility:** play/pause + speed 0.5×–8× driving the existing `t`. No server data.

**Files:** Create `web/src/usePlayback.ts` (testable pure hook); Modify `web/src/components/Replay2D.tsx` (controls); Test `web/src/usePlayback.test.ts`.

- [ ] **Step 1 — hook test (fails)**: `usePlayback` exposes `{playing, speed, toggle, setSpeed}` and, via an injectable "tick", advances `t` by `speed × dt` seconds, stops at `durationSec` (clamp + auto-pause). Write a unit test of the **pure advance function** `advance(t, dt, speed, duration) → {t, playing}` (not the React hook): `advance(10, 1, 2, 100) → {t:12}`; `advance(99.5, 1, 8, 100) → {t:100, playing:false}` (clamp + pause at the end).
- [ ] **Step 2**: `npm run test` → FAIL.
- [ ] **Step 3 — implement** `web/src/usePlayback.ts`: pure `advance()` + a hook `usePlayback(duration, {onTick})` that uses `requestAnimationFrame` (real delta via a timestamp passed as an argument to the callback to stay testable; the hook measures the delta itself via `performance.now`). NB: `performance.now` is allowed in the browser (this is not the Rust sandbox). The hook does NOT own `t`: it calls `onTick(dt)` and the component applies `advance`.
- [ ] **Step 4**: `npm run test` → PASS.
- [ ] **Step 5 — wire into Replay2D.tsx**: ▶/⏸ button, speed selector (0.5, 1, 2, 4, 8), wired to the existing scrub (`setT(t => advance(...).t)`). Manual scrubbing pauses. `aria-label` on the controls. **Loop stop**: when `advance` returns `playing:false` (end reached), the component must put the hook back into pause (call `toggle`/`setPlaying(false)`) so the `requestAnimationFrame` loop stops spinning idle after the end of the clip.
- [ ] **Step 6**: `npm run build` → `✓ built`. Commit `feat(web): animated playback play/pause + speed (US-11)`.

---

## Chunk 2 — Structures (Phase 3, US-16) · CRATE + FRONT

**Responsibility:** state of the structures (core/fort/keep/tower/wall/gate) alive→destroyed while scrubbing.

**Files:** Modify `crates/storm-replay-viewer/src/{model.rs,extract.rs}`; Modify `web/src/replay2d.ts` (type) + `web/src/components/Replay2D.tsx` (rendering); crate test `tests/extract.rs`.

- [ ] **Step 1 — model**: add to `ViewerModel` a field `structures: Vec<Structure>` where
  `Structure { team: i64, kind: String, x: f64, y: f64, destroyed_at: Option<f64> }` (camelCase, `destroyedAt`). **Bump `VIEWER_VERSION` → 2.**
- [ ] **Step 2 — crate test (fails)**: `structures_present_and_classified`: ≥1 structure `kind=="core"` per team (0 and 1), all coords ∈ [0,1], `destroyed_at` either `None` or ∈ [0, durationSec]. (On silver-city ARAM there are 2 `KingsCore`.)
- [ ] **Step 3**: `cargo test -p storm-replay-viewer` → FAIL.
- [ ] **Step 4 — extraction**: in `build`, walk `SUnitBornEvent` where `m_controlPlayerId ∈ {11,12}` **and** `m_unitTypeName` matches a structure prefix. Classify `kind`:
  ```
  KingsCore|*Core*        → "core"
  TownTownHall*           → "fort"      // (fort/keep: keep "fort" in V1, refine later)
  TownCannonTower*|*Tower*→ "tower"
  TownWall*               → "wall"
  TownGate*               → "gate"
  TownMoonwell*           → "well"
  otherwise               → "other" (ignore other for rendering if too noisy)
  ```
  `team = if m_controlPlayerId==11 {0} else {1}`. Position `q3(norm_tile(m_x,m_y))`. `destroyed_at`: look for a `SUnitDiedEvent` with the same `(m_unitTagIndex,m_unitTagRecycle)` → `loop_to_sec(t)`, otherwise `None`. Sort by (team, kind). Ignore ephemeral units (`WitchDoctorZombieWallUnit`, `*BunkerDrop*`, summons) — restrict to the `Town`/`KingsCore`/`HallOfStorms` prefix.
- [ ] **Step 5**: regenerate the golden (`UPDATE_GOLDEN=1`), `cargo test -p storm-replay-viewer` + `--workspace` PASS, clippy clean.
- [ ] **Step 6 — front**: `Structure` type in `replay2d.ts`; in the draw loop, draw each structure at `(x,(1-y))`: icon/diamond by `kind` (team colour), **greyed + ✕** if `destroyedAt != null && t >= destroyedAt`. Core = bigger. Draw UNDER the heroes (structures first).
- [ ] **Step 7**: `npm run build` ✓. Commit `feat(viewer): structures alive/destroyed (US-16)` (crate+front).

---

## Chunk 3 — Kill-feed cliquable (Phase 3, US-20) · CRATE + FRONT

**Responsibility:** event log (takedowns, structure deaths, camp captures) next to the map; click → seek.

**Files:** Modify `crates/storm-replay-viewer/src/{model.rs,extract.rs}`; `web/src/replay2d.ts` + `Replay2D.tsx`; crate test.

- [ ] **Step 1 — model**: `events: Vec<FeedEvent>` where **the crate emits STRUCTURED fields, not readable text** (the front builds the label with the real names):
  `FeedEvent { t: f64, kind: String, team: Option<i64>, victim_player_id: Option<i64>, killer_player_id: Option<i64>, structure_kind: Option<String> }` (camelCase in JSON). `kind ∈ {"takedown","structure","camp","objective"}`. `structure_kind` reuses the Chunk 2 enum (`core/fort/tower/wall/gate/well/other`). **Bump `VIEWER_VERSION` → 3.**
- [ ] **Step 2 — test (fails)**: `feed_events_sorted_nonempty`: `events` non-empty, sorted by increasing `t`, each `t ∈ [0,durationSec]`, at least one `kind=="takedown"`.
- [ ] **Step 3**: FAIL.
- [ ] **Step 4 — extraction**: build `events` with the structured fields defined in Step 1 (no text on the crate side).
  - **takedowns**: one `FeedEvent{kind:"takedown", t, victim_player_id, killer_player_id, team=victim's team}` per hero death (reuse the logic/data of `deaths`).
  - **structure**: `FeedEvent{kind:"structure", t=destroyed_at, team, structure_kind}` for each Chunk 2 `Structure` whose `destroyed_at != None`.
  - **camp**: `SStatGameEvent{JungleCampCapture}` → `FeedEvent{kind:"camp", t, team}` (team from `m_intData`/player→team if present, otherwise `team=None`).
  - Sort by increasing `t`.
- [ ] **Step 5**: golden + tests + clippy PASS.
- [ ] **Step 6 — front**: list panel on the right (under the legend or as a tab): each event = icon (💀/🏰/🏕️) + label (names via `players[]`/`heroIcon`) + `mm:ss` timecode, team colour, **click → setT(event.t)** + pause. Highlight the event closest to `t`. Auto-scroll to the current event. Clickable rows = `<button>` (or `role="button"` + an `aria-label` describing the event and its timecode) for keyboard accessibility.
- [ ] **Step 7**: `npm run build` ✓. Commit `feat(viewer): clickable kill-feed event log (US-20)`.

---

## Chunk 4 — Ability indicators (Phase 3, US-18) · CRATE + FRONT

**Responsibility:** flash on a hero when they cast an ability.

**Files:** crate `{model,extract}`; `replay2d.ts` + `Replay2D.tsx`; crate test.

- [ ] **Step 1 — model**: add to `HeroTrack` a field `casts: Vec<f64>` (cast instants, sorted). **Bump `VIEWER_VERSION` → 4.**
- [ ] **Step 2 — test (fails)**: `casts_present`: sum of `casts` over all heroes > 100; each instant ∈ [0,durationSec]; sorted.
- [ ] **Step 3**: FAIL.
- [ ] **Step 4 — extraction**: in the `visit_game_events` pass, for an `SCmdEvent` with a non-null `m_abil`, `t=loop_to_sec(_gameloop)`, `p=user_to_player[_userid.m_userId]` → push `t` into `casts[p]`. **Temporal dedup**: ignore a cast less than ~0.3 s after the same player's previous one (re-cast spam). Do NOT try to identify which ability (out of scope — just "cast an ability"). Sort.
- [ ] **Step 5**: golden + tests + clippy.
- [ ] **Step 6 — front**: pure helper `castFlash(casts, t, window=0.6) → 0..1` (intensity of the closest flash, 0 if none) + test. In the draw loop, if intensity>0, draw a pulsing glowing ring around the marker. Useful during animated playback.
- [ ] **Step 7**: `npm run build` ✓. Commit `feat(viewer): ability-cast flash indicators (US-18)`.

---

## Chunk 5 — Talents/niveaux + bande talents (Phase 3 US-19 + Phase 5 US-27) · CRATE + FRONT

**Responsibility:** talent pick marker per hero + **tier strip** under the scrub.

**Files:** crate `{model,extract}`; `replay2d.ts` + `Replay2D.tsx` (+ possibly `web/src/components/TalentStrip2D.tsx`); crate test.

- [ ] **Step 1 — model**: `HeroTrack.talents: Vec<TalentPick>` where `TalentPick { t: f64, tier: i64, talent_id: Option<String> }`; `levels: Vec<LevelTick>` (at the `ViewerModel` level) where `LevelTick { t, team, level }` (ARAM = XP shared per team). **Bump `VIEWER_VERSION` → 5.**
- [ ] **Step 2 — test (fails)**: `talents_and_levels`: at least one hero has non-empty `talents`, `tier` increasing per hero; `levels` non-empty, `level` increasing per team.
- [ ] **Step 3**: FAIL.
- [ ] **Step 4 — extraction**: talents from `SHeroTalentTreeSelectedEvent` (order of picks per player = tier 1,2,3… → `tier` = pick rank; map to levels 1,4,7,10,13,16,20 for information). **V1 decision: `talent_id = None`** — the crate stays pure (no reference data, see Cross-cutting notes); the FRONT resolves the name via `dim_talents` + the already stored `match_players.talents` build (indexed by tier). Do NOT read `TalentChosen.m_stringData` in V1. Levels: `SStatGameEvent{LevelUp}` (team via `m_intData`/player→team) → `levels`.
- [ ] **Step 5**: golden + tests + clippy.
- [ ] **Step 6 — front marker**: on the marker, a small "⬆" badge or halo when `talents[i].t` is close to `t` (≤ 3 s). Resolve the name via `talentInfo()`/`dim_talents` (already on the front) in a tooltip.
- [ ] **Step 7 — talent strip (US-27)**: under the scrub, a grid of 10 rows (heroes) × tiers; cell filled when the tier is taken at `t` (team colour), empty otherwise; hover = talent name. Team level displayed.
- [ ] **Step 8**: `npm run build` ✓. Commit `feat(viewer): talent/level indicators + talent timeline strip (US-19/US-27)`.

---

## Chunk 6 — Per-map objectives (Phase 4, US-21→US-24) · CRATE (module per map) + FRONT

**Responsibility:** isolated per-map framework (US-24); fill in the traceable objectives, **flag "data unavailable"** for maps with a known gap (US-7/US-22/US-23); Braxis best-effort (US-21).

**Files:** Create `crates/storm-replay-viewer/src/maps/mod.rs` (+ `braxis.rs`, `generic.rs`); Modify `extract.rs` (call), `model.rs`; `replay2d.ts` + `Replay2D.tsx`; crate test.

- [ ] **Step 1 — model**: `objectives: Vec<Objective>` where `Objective { t: f64, kind: String, team: Option<i64>, text: String }` (timestamped objective events) + reuse `warnings: Vec<String>` (already present, US-7). **Bump `VIEWER_VERSION` → 6.**
- [ ] **Step 2 — module per map (US-24)**: `maps::objectives(map_name, &tracker) -> (Vec<Objective>, Vec<String> warnings)`. Each map = a handler; default `generic` (nothing / generic objective events if any). Maps with a gap → `warnings.push("objective data unavailable: <map>")` (Blackheart's Bay, Volskaya Foundry).
- [ ] **Step 3 — test (fails)**: `objectives_framework`: for a Braxis replay (or failing that, silver-city → empty `objectives` + no wrong warning), the function does not panic and respects the invariant (t ∈ [0,dur]). For a map with a gap (build a case), a warning is present. (Use the available replays; if Braxis is missing from the local corpus, test at least the `generic` routing + the warning on a simulated gap-map name via a unit test of `maps::objectives`.)
- [ ] **Step 4**: FAIL.
- [ ] **Step 5 — implement**: `braxis.rs` (US-21): infer wave strength/timing from the deaths of `Ultralisk`/zerg units (`SUnitDiedEvent` zerg unit types) → `Objective{kind:"zerg_wave"}`. `generic.rs`: map the common objective `SStatGameEvent`s if present. Gap maps → warnings. Wire into `build`.
  ⚠️ **Corpus**: no Braxis/Blackheart/Volskaya replay is committed (only silver-city ARAM + Industrial District). Try to get one (`fetch-corpus`/NAS/box read-only) to validate `braxis.rs`; **failing that, mark the wave inference as "not validated on real data / best-effort" in the commit message AND in STATUS.md** — do not present it as tested when only the routing + the warning are.
- [ ] **Step 6**: golden + tests + clippy.
- [ ] **Step 7 — front**: show the `objectives` in the kill-feed (kind `objective`) + an "objective data unavailable" banner when `warnings` is non-empty.
- [ ] **Step 8**: `npm run build` ✓. Commit `feat(viewer): per-map objectives framework + gap warnings + Braxis waves (US-21..24)`.

---

## Chunk 7 — Toggle minions/camps (Phase 5, US-26) · CRATE + FRONT

**Responsibility:** show/hide non-hero units (best-effort, sparse).

**Files:** crate `{model,extract}`; `replay2d.ts` + `Replay2D.tsx`; crate test.

- [ ] **Step 1 — model**: `minions: Vec<MinionSample>` where `MinionSample { t, x, y, team }` (damage-gated non-hero positions). **Bump `VIEWER_VERSION` → 7.** ⚠️ Volume: quantise + aggressive dedup (grid), cap (log if truncated — no silent cap).
- [ ] **Step 2 — test (fails)**: `minions_bounded`: coords ∈ [0,1]; bounded count (< 20000); team ∈ {0,1} or -1 (neutral).
- [ ] **Step 3**: FAIL.
- [ ] **Step 4 — extraction**: from `SUnitPositionsEvent`, for known **non-hero** tags (via `unit_player`: non-`isHero` unit, or owner 11/12), push `MinionSample`s (team from owner: 11/12→0/1, other→-1). Dedup per grid cell (~1/64) + time window. Sort by t. **Hard cap `const MINION_CAP: usize = 15000;`**: if exceeded, truncate **and** `log::warn!("minions truncated: {kept}/{seen}")` — never a silent cap (the Step 2 test checks `< 20000`).
- [ ] **Step 5**: golden + tests + clippy.
- [ ] **Step 6 — front**: "minions/camps" checkbox (default OFF, with `aria-label`); if ON, draw small dull dots (dimmed team colour) at the `sampleAt`-like nearest (no interpolation — too sparse, show the closest sample within a ~5 s window).
- [ ] **Step 7**: `npm run build` ✓. Commit `feat(viewer): minion/camp visibility toggle (US-26)`.

---

## Chunk 8 — Clip export (Phase 5, US-25) · FRONT ONLY

**Responsibility:** export a [start,end] excerpt of the canvas as a video file.

**Files:** Create `web/src/clipExport.ts`; Modify `Replay2D.tsx` (UI); unit test of the bounds logic.

- [ ] **Step 1 — test (fails)**: pure function `clipFrames(start, end, fps) → number` (number of frames) + bounds validation (`start<end`, clamp to duration). `clipFrames(10,15,30)=150`.
- [ ] **Step 2**: FAIL.
- [ ] **Step 3 — implement** `clipExport.ts`: `clipFrames()` + `recordClip(canvas, {start, end, fps, speed, onFrame})` using `canvas.captureStream(fps)` + `MediaRecorder` (webm/vp9, vp8 fallback); internal loop that drives `onFrame(t)` from start→end every `1000/fps` ms and `stop()` at the end → `Blob` → `URL.createObjectURL` → download `match-<id>-<start>-<end>.webm`. NB: MediaRecorder is a standard browser API.
- [ ] **Step 4**: `npm run test` PASS (the pure part; the actual recording = E2E check).
- [ ] **Step 5 — UI**: two "clip start/end" handles (or "set in/out" buttons on the scrub) + an "Export clip" button → `recordClip` driving the existing canvas rendering. Disabled if `start>=end`. Recording indicator. `aria-label` on the in/out/export buttons.
- [ ] **Step 6**: `npm run build` ✓. Commit `feat(web): clip export to webm (US-25)`.

---

## Chunk 9 — Global E2E check + STATUS

- [ ] **Step 1**: `cargo test --workspace` + `cd web && npm run test` + `npm run build` green.
- [ ] **Step 2 — local smoke** (like MVP-1): dev PG + server + upload of a mapped replay (Cursed Hollow pulled read-only from the box if needed); via preview: animated playback, falling structures, clickable kill-feed, ability flashes, talent strip, minion toggle, export of a short clip. Screenshots.
- [ ] **Step 3 — STATUS**: update `docs/STATUS.md` (Phases 2–5 shipped + verified; what possibly remains). Commit.
- [ ] **Step 4**: `superpowers:finishing-a-development-branch` (merge/PR) then (operator option) box redeployment.

---

## Notes transverses
- **Evolving schema**: each batch that adds a field bumps `VIEWER_VERSION` + regenerates the golden. The front ignores fields it does not know yet (backward compatibility).
- **Payload budget**: structures/events/talents/casts are light; **minions** is the only risk → grid dedup + cap + `log`. Objectives are light.
- **Reuse**: hero/talent names via `players[]` + `dim_*` (front) — the crate stays pure geometry/time, with no reference data.
- **Seek perf**: everything stays 100% client-side; the new layers are `t` lookups (binary/window). No round-trip.
- **YAGNI**: no identification of the exact ability (US-18 = generic flash); fort/keep not finely distinguished in V1; objectives = traceable + "unavailable" otherwise (no over-inference).
