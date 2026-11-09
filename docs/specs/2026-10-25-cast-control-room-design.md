# Cast control room — design

> storm-codex spec, **to be approved by the operator before any code** (Rule no. 0). Brainstorm of
> 2026-10-25 with the operator. Builds on the draft simulator (`2026-06-19-draft-simulator-design.md`)
> and draft from a replay (`2026-10-25-draft-replay-design.md`, merged into `main`).
> Points marked **[Claude's choice]** are decisions taken without the operator: to be reviewed.
> Mockups: `docs/specs/2026-10-25-cast-mockups/` (HTML fragments, to open in a browser;
> the names, scores and portraits in them are fictitious; the `ingame.webp` background capture only keeps
> the top HUD band sharp — everything else, minimap and player names included, is blurred).

## Goal

Give the caster **a single administration page** to drive everything the audience sees of a
series (Bo3/Bo5): map veto, lineups, hero draft, in-game banner, results between
games. Today only the hero draft exists (`/draft`, `/draft/replay`, overlay
`/draft/overlay`); maps there are only decorative and nothing covers the rest of a broadcast.

Inspiration: Soothsayer (ebshimizu, MIT) — a caster control panel + OBS overlays.
**Complete rewrite** in the Storm Codex stack and its design system: no code reused.

## Decisions made (with the operator)

1. **A `/cast` control room**, layout "B": permanent left column (series: teams, lineups, Bo,
   Fearless, score, maps), scene tabs + **live preview** of what is on air in the
   centre, workspace of the selected scene below (mockup `control-room-layout.html`).
2. **Five scenes**: *Lineups*, *Series*, *Draft*, *In-game*, *Hide*. Changing scene changes what
   OBS shows, without touching anything in OBS.
3. **Two data sources, same display**: live, the caster enters everything by hand (maps,
   hero bans/picks, Fearless); from a replay, the draft fills itself from the replay (already delivered),
   and so do the game results.
4. **Map veto entered, not driven**: map bans/picks happen elsewhere (captains); the
   caster **reports them in any order**. No sequence imposed. At the start of the
   series, they choose **which maps are in the pool** (≈ 10 of the game's maps).
5. **Standard HotS format** for the hero draft (existing Standard / Normal / Fearless).
6. **Style**: Nexus Codex — the draft overlay's "nexus" skin for full-screen scenes, the app's
   "dossier" look for the control room. The 4 existing skins remain available.

## Out of scope / anti-goals

- Automatic reading of the draft **from a game in progress** (the caster enters it; the replay is the only automatic mode).
- Driving OBS itself (changing OBS scenes, OBS WebSocket): the control room only talks to our overlay.
- Win rates or statistics from our database displayed as "tournament level": misleading
  (games of all modes), **therefore excluded**. The stats displayed are either entered, or taken from
  the game currently being presented.
- Simultaneous multiple operators, sponsors/tickers, chat, talents, draft bot.
- Covering the game's native interface: the in-game overlay must hide nothing of the HUD.
- **Spoilers**: in replay mode, nothing from the future is sent to the overlay (rule already established).

## Series model (new)

A **series** is the central entity; the control room drives one at a time (persistent singleton, like
`draft_live`). Contents:

- **Teams** (Blue / Red): name (required), logo (optional, uploaded image or URL), 5 players:
  BattleTag/name, **optional free line** (role, slogan — entered by the caster), link to the
  player in our database if the name is resolved. An optional "captain" flag **[Claude's choice —
  to remove if useless]**.
- **Format**: Bo (1/3/5/7), draft mode (Standard / Normal / Fearless).
- **Series score**: wins per team (derived from finished games, correctable by hand).
- **Map pool**: list chosen at the start. Each map carries a status:
  `available` · `banned by X` · `picked by X` · `decider`. No order imposed between entries.
  The decider is **marked by hand** (or proposed when only one map is left **[Claude's choice]**).
- **Games**: number, map played, winner, takedown score, duration, reference to the replay if loaded.
  The map of game N is **chosen among the "picked"/decider maps when the game is started**
  **[Claude's choice]** (the play order is not necessarily the pick order).
- **Current scene** + rotation state of the *Series* scene's slides.

The **hero draft** stays carried by the existing engine (`DraftState`). Invariant to respect:
**team names, players, score and Bo have only one source of truth** (the series) — the draft
reads them or is synced to it; never two editable places. The details are left to the plan.

## Scenes

### Lineups (team cards) — mockup `team-cards.html`

Full screen, nexus skin. Two cards (Blue left, Red right): logo, name, record if entered, 5 players.
**Two modes of the same card**:
- **A — before the series** (no game played): per player, their **3 most-played heroes over their
  last 20 games** in our database, **hidden** if they have fewer than ~5 recent games **[Claude's
  choice: thresholds to review]**; plus the entered free line. **No win rate** (operator decision).
- **B — after ≥ 1 game**: the portraits become **the heroes actually played in this series**,
  in order, those locked by Fearless dimmed.
Automatic switch A→B at the first finished game; **manual override** possible in both directions.

### Series (between games) — mockups `styled-mockups.html`, `postgame-breakdown.html`

- **Map strip** (always visible): all the pool's maps; for each one, who
  banned/picked it, **"WON" in the winning team's colour** (no numbers on the map),
  gold **"NEXT"** on the next one, `decider`, banned ones struck through.
- **End-of-game slideshow** under the strip, fed by the **decoded replay of the game that
  has just been played**: "post-game breakdown" screen (WIN/LOSS header, takedown score, duration,
  map; comparison column; per-player damage panel with tabs; experience advantage
  curve). Scrolling **automatic** (adjustable duration) or **by hand** from the control room.
- LoL → HotS mapping: gold → experience advantage; turrets → forts/keeps; drakes/barons
  → mercenary camps and map objectives.
- Data (already in storm-stats): hero damage, healing, siege damage, takedowns/deaths/assists,
  team level, forts/keeps, camps, map objectives, periodic XP, bans. **Without a loaded replay**,
  only the map strip and the score are shown (the caster enters the winner) **[Claude's choice]**.

### Draft

The existing draft (manual live + replay, Fearless, replay queue, timers) **moves into the
control room**: same functions, hero picker and Blue/Red columns in the workspace. At functional
parity, `/draft` and `/draft/replay` redirect to `/cast`; `/draft/overlay` keeps
working as long as the old OBS source exists **[Claude's choice]**.

### In-game — mockup `ingame-bar-v6.html` (final: see "rectangle" below)

Transparent background placed over the game capture. **Does not cover the native HUD** (portrait band at
the top, central panel team level / structures / clock, minimap bottom right, replay panel).
- **Two rectangular tabs** under the portrait bands, on either side of the game's central
  panel: team name + **series score**. The inner top corner of each tab is
  **anchored** (≈ 39.2 % and 59.65 % of the width, y ≈ 9.9 % of the height for 1920×1080; measured on
  the operator's captures); the name **grows outwards** up to ≈ 26 % of the width,
  then is truncated with "…". **Straight edges, square corners** (operator decision: no angles
  imitating the game's `\` `/`).
- **Badge at the top right**: `BO5`, game number, and `FEARLESS` (or `STANDARD`/`NORMAL`)
  **[the map played could be added to it — to decide]**.
- These positions depend on the game's interface at 1920×1080; the **anchors are adjustable
  constants** (fine offset from the control room or in CSS) to absorb a UI change.

### Hide

Completely transparent overlay.

## OBS overlay

**A single browser source** `/cast/overlay` (1920×1080, transparent background) that **follows the current
scene** over WebSocket (`cast.updated`); short transition (fade ≈ 300 ms **[Claude's choice]**).
Refreshing the page or the source loses no state.

## `/cast` control room

- Left column: **Series** (teams + score, Bo, mode), **Lineups** (5 players per team, free
  line, logo), **Maps** (pool, statuses, games).
- Scene bar with an "on air" indicator, live preview (the real overlay, scaled).
- Workspace per scene: *Lineups* (forced A/B mode), *Series* (slideshow: current slide,
  auto/manual, duration), *Draft* (existing), *In-game* (score steppers, badge correction),
  *Hide*.
- Labels in English (app convention); open mode like the current draft routes, with the
  same guard as the rest if the operator asks for it **[Claude's choice]**.

## Technical contracts (sketch — the plan specifies them)

- Server: new `cast` module (persisted JSON series state, singleton), routes `/api/cast/*`
  (read, series, teams, pool/maps, games, scene, slides), WS event `cast.updated` (stateless signal,
  like `live.updated`). Reuses the existing draft engine and replay decoding.
- Lineups: recent-heroes queries per player on our database (budget: < 100 ms for 10 players).
- Post-game: an endpoint that serves the overlay the summary of a game (from the decoded replay or the
  projection if the game is already in the database); **a replay loaded for casting is not written to the database**
  (rule already established for draft from a replay).
- Front-end: routes `/cast` (control room) and `/cast/overlay`; overlay components shared with
  `DraftOverlay` rather than duplicated.

## Acceptance criteria

1. From `/cast`, a series is created (2 teams, 5 players each, Bo5, Fearless, pool of 10 maps),
   map bans and picks are reported in any order, and the *Series* scene reflects them.
2. **Scene change → overlay updated in < 500 ms** (measured, WS); the control room preview = what OBS sees.
3. The team cards switch from A to B after the first game, with Fearless dimming heroes
   already played; manual override OK.
4. A finished game's replay feeds the "post-game breakdown" (all displayed values
   match the match page of the same game); nothing is written to the database.
5. The in-game tab covers no HUD element on a real 1920×1080 capture; with 40-character
   names, the name is truncated without overflowing.
6. The draft (live and replay, Fearless, queue) works from the control room with the same guarantees
   as before (existing tests green); no team data is editable in two places.
7. Refreshing the control room or the overlay loses no state; same for a server restart.

## Docs to update with the code (Rule no. 0)

`docs/spec/05-api.md` (`/api/cast/*` routes), `04-serveur.md` (`cast` module, persistence),
`06-modele-donnees.md` (series table), `07-evenements.md` (`cast.updated`), `08-frontend.md`
(`/cast`, `/cast/overlay`, `/draft*` redirects), `docs/STATUS.md`.

## Clarifications from the review (review of 2026-10-25)

- **Source of truth for teams [Claude's choice]**: the series is **authoritative**; `DraftState` **keeps
  its fields** (`blue`/`red`/`score`/`bo`, no migration of the `draft_live` JSON, old tests and
  `/draft/overlay` intact) and receives a **one-way sync** series → draft on each
  change. As soon as a series is active, editing these fields from the old `/draft` page
  is disabled (read-only); it disappears with the redirect to `/cast`.
- **Players ↔ our database [Claude's choice]**: the caster types a name; the control room uses the existing
  search `GET /api/players/search` (former names included) and offers a **picker** when there are
  several results; no result = "unlinked" player (free line only, no recent heroes).
  Never a first result chosen silently. "Last 20 games" = **all modes combined**
  (it is about heroes played, not level).
- **Post-game and replays [Claude's choice]**: the caster **drops one replay per game** in the control room
  (same path as the draft-from-replay queue, Custom only). A **compact summary of the
  game** (heroes per player, breakdown values, series for the curve) is **kept in the series
  state** — no re-decoding, nothing in the database. The caster can reopen the breakdown of **any
  game** of the series; Fearless and mode B of the team card read these same summaries.
- **Breakdown data**: **parity with the match page** is the criterion; any displayed field
  must exist in storm-stats' current output for a replay (hero damage, healing, siege
  damage, takedowns/deaths/assists, team level, forts/keeps, camps, map objectives, periodic
  XP). **No new metric** in storm-stats for this feature: a missing value
  is removed from the screen, not invented. The plan checks field by field before freezing the screen.
- **Logos [Claude's choice]**: upload of an image (PNG/WebP/JPEG, **≤ 512 KB**) stored in the database
  and served by the server, **or** a URL. No file storage on the box.
- **Access [Claude's choice]**: open reads (the OBS overlay has no credentials); **writes to
  `/api/cast/*` open like the draft routes** as long as the server is only reachable via
  Tailscale/LAN. If the operator exposes it, add the same guard as `/api/admin`.
- **Map pool**: the list comes from the existing reference data (maps + visuals already served for
  the draft overlay and `/maps`); the caster ticks the pool's maps at the start of the series.

## Left to the plan (suggested breakdown, non-binding)

1. Series model + control room (shell, scenes, preview) + in-game overlay + badge.
2. Map pool, veto, games, *Series* scene (strip).
3. Team cards (modes A/B).
4. Post-game breakdown and slideshow.
5. Migration of the draft into the control room and redirects.
