# Cast control room — usability (overlay URL, flow, putting on air)

> storm-codex spec, **approved by the operator on 2026-10-31 (recommended choices: C = explicit button, D = server clock)**. Language: English confirmed by the operator on 2026-10-31 (point F). Rule no. 0.
> Follows `2026-10-25-cast-control-room-design.md` (control room delivered, PR #5 merged into `main`).
> Points marked **[Claude's choice]** are proposals to review; those marked **[to decide]**
> question a decision already made and require an explicit answer from the operator.

## Goal

A streamer discovering the control room must be able, **without searching for anything**, to: get the OBS source
URL, know in which order to act, see what is actually on air, and understand an error
message. Today the overlay URL only appears on the Draft tab (in the cover of the
Draft console); the Series, Lineups and In-game tabs do not show it.

## Findings (review of 2026-10-31, `main` code)

1. **Hidden overlay URL**: `web/src/cast/DraftWorkspace.tsx` (l.74 and l.89) is the only place
   that shows `/cast/overlay?skin=nexus`. `/cast` (the `Layout`'s "Cast" link) shows it nowhere.
   Step 3 of the first-launch wizard (`components/Onboarding.tsx`) lists old sources
   (`/queue`, `/ticker`, `/widget`, `/now-playing`) and **not** `/cast/overlay`.
2. **Preview ≠ OBS output**: `cast/SeriesWorkspace.tsx` and `castState.ts` (l.242–249) admit it:
   in automatic rotation, the preview and the OBS source each have their own timer. Criterion 2 of the
   original spec ("the control room preview = what OBS sees") is therefore not met in auto mode.
3. **On air on click**: a click on a scene tab sends the scene on air
   (`POST /api/cast/scene`, R26). No confirmation, and the local working tab may differ from
   the scene on air without it being obvious.
4. **Raw errors**: `Server refused: <code>` shows `NotInPool`, `SeriesInactive`,
   `MapAlreadyPlayed`, `side_mismatch: the replay's winner contradicts…`.
5. **No flow**: nothing indicates the order (create the series → teams → pool → games → scenes).
   "New series" is disabled while the pool is empty, without saying why; "Save teams" is
   separate from "New series", which still sends the unsaved teams.
6. **Jargon**: "BO 5 · GAME 2 · FEARLESS · score from game winners", Lineup modes "Auto / A / B",
   "linked / unlinked", "awaiting game data", "Swap sides" (explanation only in a tooltip),
   "Custom replay" required for the post-game (only in a short instruction).
7. **Duplicate Draft console**: the "Draft simulator · operator console" cover (large title)
   shows inside the control room, with a style that does not look like the rest of `/cast`.

## Proposed decisions

### A. Permanent "OBS source" block (findings 1, 7)

- In the **header** of `/cast`, always visible whatever the scene: the full URL
  (`{origin}/cast/overlay?skin=…`), a **Copy** button, a skin selector
  (`nexus · glass · tactical · mono`, default `nexus`), and the instruction "Browser source · 1920×1080 ·
  transparent background". **[Claude's choice]**
- The URL updates with the chosen skin; the skin is remembered in the browser only
  (display preference, not shared state). **[Claude's choice]**
- Remove the "operator console" cover from the Draft tab; keep only the
  "Draft from a replay" link in the Draft tab. **[Claude's choice]**
- Step 3 of the first-launch wizard: add `/cast/overlay` to the list, and present it as
  the control room's main source (the others remain optional). **[Claude's choice]**

### B. Guided flow (finding 5)

- A **step bar** at the top of the rail: *1 Series* (Bo, format, pool) → *2 Teams & players* →
  *3 Games* → *4 Live*. Each step shows its state (done / to do) from the server
  state, with no checkbox to maintain by hand. **[Claude's choice]**
- When no series is active: the rail opens on a short welcome text ("Create a series
  to get started") and on the only possible action. **[Claude's choice]**
- "New series" states why it is disabled ("Tick at least one map in the pool"). **[Claude's choice]**
- A modified, unsaved team is flagged in "New series" ("unsaved teams will be
  sent" or "Save the teams first"); no silent implicit save. **[Claude's choice]**

### C. Explicit putting on air (finding 3) — **[to decide]**

R26 chose "click = on air". This spec proposes to reconsider it:

- **Option 1 (recommended by Claude)**: a click on a tab opens the scene **in the
  workspace only**; a clearly visible **"Put on air"** button (and "Enter") broadcasts it.
  The tab on air stays marked *ON AIR*, and the working tab that is not broadcast is flagged.
- **Option 2**: keep the direct click, but ask for confirmation when the scene being left is
  *In-game* with a game in progress.

Operator's choice required: option 1 or option 2? (The keyboard and ARIA roles remain those
described by the R26 spec in both cases.)

### D. The preview shows what OBS shows (finding 2) — **[Claude's choice]**

- Recommended: **automatic rotation is driven by the server** (the server carries the slide
  index and the timestamp of its last change; the overlay and the preview compute the current moment
  from these two values). Preview and OBS then show the same slide, without depending on two
  local timers.
- Less invasive alternative: in auto mode, the preview shows **the slide received from the server** and the
  control room no longer shows "rotating n slides". Less faithful, but no server change.
- The choice affects `sync`/`cast.updated`; it must be decided before the plan.

### E. Understandable errors (finding 4) — **[Claude's choice]**

- A `code → plain message` table (e.g. `NotInPool` → "This map is not in the series'
  pool.", `SeriesInactive` → "No series in progress.", `MapAlreadyPlayed` → "This map has already been
  played.") in `castState.ts`, next to `replayErrorText` which already exists.
- Technical code kept as a fallback (tooltip or discreet line), for support. The banner stays
  "never swallowed" as today.

### F. Labels and language (finding 6)

- Labels replaced by short wordings: "Lineup mode" → "Team cards: before the
  series / between games / automatic". Badge: "BO5 · Game 2 · Fearless" without "score from
  game winners" (moved to a tooltip).
- "Swap sides": label "Swap sides" with a visible instruction under the checkbox when a replay
  is refused with `side_mismatch`.
- "Custom replay": visible mention in the Series tab when no game has a replay.
- **[to decide]** Interface language: the original spec chose English labels
  ("app convention"). A French-speaking streamer would benefit from a French interface. Operator
  decision required; this document does not take it.

## Out of scope

- New overlay feature, new skin, new scene.
- Driving OBS (OBS WebSocket) — still excluded by the original spec.
- Complete visual redesign of the control room (only the OBS block, the flow and the labels change).
- Data contract change other than the one described in D (if D is retained).

## Acceptance criteria

1. From `/cast`, without navigating, the OBS source URL is visible and copyable in one click, in
   **all** scene tabs, with the chosen skin.
2. From the first-launch wizard, `/cast/overlay` is listed and leads to the control room.
3. A user who has never used the control room can, by following only the step bar and the
   displayed messages, create a series, fill it in and put it on air (manual flow test
   recorded in `docs/STATUS.md`).
4. Depending on the option chosen in C: no scene is broadcast without an explicit action (option 1), or a
   confirmation is asked when a game is in progress (option 2).
5. Depending on D: in auto mode, the preview and the OBS source show the same slide at the same moment (measured
   on two tabs, gap ≤ 1 s), or the control room no longer shows a promise of alignment (alternative).
6. Each error code listed in E shows the plain message; an unknown code shows the raw code.
7. No regression: the existing tests (`web` and server) stay green; `replayErrorText` and the
   R19–R26 guarantees are unchanged, unless decided otherwise in writing.

## Docs to update with the code (Rule no. 0)

- `docs/spec/08-frontend.md`: `/cast` (OBS header, step bar, putting on air), `/cast/overlay`
  (if D changes how it works), labels.
- `docs/spec/05-api.md` and `07-evenements.md` only if D is retained (server slide clock).
- `docs/STATUS.md`: update the control room's status (the "not merged" section is obsolete since PR #5)
  and the result of the flow test (criterion 3).

## Questions for the operator to decide

1. Putting on air: option 1 (explicit button, recommended) or option 2 (confirmation) — **[C]**.
2. Auto rotation: server clock (recommended) or a preview that follows the server without a shared clock — **[D]**.
3. Interface language: French or English — **[F]**.
4. Should the OBS block also be visible on mobile/small screens, or only at the control room station?
