# HotS draft simulator + overlays — design

> storm-codex spec. Brainstorm approved on 2026-06-19. Light process (batched decisions,
> short spec, no subagent review loop). Target: `writing-plans` then TDD implementation.

## Goal

A Heroes of the Storm draft simulator **driven 100 % manually** by the operator
(all choices of both teams), with **three** draft **formats** and **two live OBS overlays**
(player view + caster view) reflecting the draft state like the in-game draft screen.

No bot/AI: the operator makes every ban and every pick for both sides.

## Decisions made (brainstorm)

1. **Sync**: **server-side authoritative** draft state + WebSocket broadcast. A control
   console drives it; the overlays subscribe live (a true second screen, survives refresh).
2. **Fearless**: **full series tracking** (auto-accumulation of the bans of previous games)
   **+** manual override of any hero's availability.
3. **Caster view**: editable team names **and** score (in addition to the player view).
4. **3 formats from V1**: Standard, Normal, Fearless.

## Key idea: the format = a list of steps (data), not code

The engine is a **generic walker** over an ordered list of steps:

```
Step = { team: Team1 | Team2, action: Ban | Pick }
DraftFormat = { id, label, steps: Step[], prebans: bool, fearless: bool }
```

Each format is just a different `steps[]`. Fixing the exact order = editing a list,
never the engine. The sequences live in an editable constants module.

### Sequences (Standard confirmed by the operator)

Steps written as **order role**: F = first-pick team, S = second. (Decoupled from the
blue/red side — see below.) **Standard** (HotS):

| # | Role | Action | | # | Role | Action |
|---|------|--------|---|---|------|--------|
| 1 | F | Ban | | 9 | F | Pick |
| 2 | S | Ban | | 10 | S | **Ban** (2nd phase) |
| 3 | F | Ban | | 11 | F | **Ban** (2nd phase) |
| 4 | S | Ban | | 12 | S | Pick |
| 5 | F | Pick | | 13 | S | Pick |
| 6 | S | Pick | | 14 | F | Pick |
| 7 | S | Pick | | 15 | F | Pick |
| 8 | F | Pick | | 16 | S | Pick |

→ i.e., with blue first-pick: ban B·R·B·R · pick B · pick R×2 · pick B×2 · ban R · ban B ·
pick R×2 · pick B×2 · pick R. 3 bans + 5 picks per team (6 bans / 10 picks).

**Normal** = Standard **without the ban steps** → pure pick order, no bans.

**Fearless** = Standard sequence, but the "unavailable" set is **pre-filled** with all
the heroes picked in the **previous** games of the current series.

### First-pick ≠ side (independent settings)

As in the HotS lobby: **who has first pick** and **who is blue/red side** are two
separate settings. The side sets the colour/position (overlay); `first_pick` (a side) only sets who
plays the F role. The engine resolves role→side via `first_pick`; the operator sets both
separately in the console.

## Architecture (storm-codex: axum + sqlx + Postgres + existing `/ws` WS)

### Draft engine — pure Rust, TDD, zero I/O

`server/src/draft/engine.rs` (or dedicated crate/module):

- `DraftState { format, map, team_names, score, first_pick, steps, cursor, bans[T1/T2],
  picks[T1/T2], unavailable: Set<HeroKey> }`
- `apply_action(state, hero) -> Result<DraftState, DraftError>`: assigns `hero` to the current
  step (`steps[cursor]`), advances the cursor. Refuses if the hero is unavailable (already picked/banned,
  fearless, or manual override) → `DraftError::Unavailable`. Refuses if the draft is finished.
- `undo(state) -> DraftState`: moves the cursor back, frees the last hero.
- `reset(state) -> DraftState`: goes back to `cursor=0`, keeps format/map/teams/series.
- `toggle_unavailable(state, hero)`: manual override (adds to/removes from `unavailable`).
- `is_complete(state) -> bool`, `current_step(state) -> Option<Step>`.
- Legality: a hero = at most one occurrence across bans+picks+unavailable.

**Tests** (the heart of the TDD): complete walk of the 3 sequences; refusal of an unavailable hero;
undo/redo of each type of step; cumulative pre-filled fearless over 2-3 games; manual
override; edge cases (undo on an empty draft, pick on a finished draft).

### Server state + persistence

- **"Current draft" singleton** in memory (a single operator) + persisted to survive a
  restart. Table `draft_live` (1 row: JSON of the state + `updated_at`).
- **Series** (for fearless): table `draft_series { id, format, bo, games: jsonb[], score,
  team_names, created_at }`. A finished game pushes its picks into `games`. "Next game"
  creates a new live draft pre-filled from the history; "new series" resets.
- Versioned: each state carries a `schema_version` (consistent with the repo's conventions).

### HTTP routes

- `GET  /api/draft` → current state.
- `POST /api/draft/config` → `{ format, map, first_pick, team_names, bo }` (resets the draft).
- `POST /api/draft/action` → `{ hero }` (assigns to the current step).
- `POST /api/draft/undo`, `POST /api/draft/reset`.
- `POST /api/draft/unavailable` → `{ hero, value: bool }` (manual override).
- `POST /api/draft/score` → `{ team1, team2 }`.
- `POST /api/draft/series/next` (next game, pre-fills fearless),
  `POST /api/draft/series/new` (new series).

### WebSocket

Extend the existing `/ws` broadcast with a `{ type: "draft.updated" }` event. The pages (console
+ overlay) react by invalidating/refetching `GET /api/draft` (same pattern as `match.parsed`).

## Frontend (React/Vite, react-router) — 2 routes

Reuses `/api/dim/heroes` (roster + `role` + `universe` + `icon`), `/images/heroes/*`,
`/images/battlegrounds/*`, the `Avatar` component, the theme.

The visuals are **frozen** in two mockups delivered with the spec:
- `draft-aesthetics.html` — the overlay (**Timeline** view, 4 swappable skins, Caster view for reference).
- `draft-control-mockup.html` — the control console.

### `/draft` — control console (operator screen)

- **Config bar**: **format** (Standard/Normal/Fearless), **map**, **first-pick** team
  (Blue/Red), **Bo** (1/3/5/7), **score** (steppers). Actions: **Undo**, **Reset**,
  **Next game**, **New series**.
- **Current phase banner**: "Blue's turn — Pick (4th choice)" (team + action +
  index) + the **phase timer** (bar + countdown).
- **Series bans (fearless)**: ribbon of unavailable heroes (already picked in the series) + counter.
- **Blue / Red columns**: **editable team name**, bans, 5 pick slots each with an **editable player
  name field**; the current slot/ban is highlighted (team colour).
- **Central picker**: text search + **role tabs** (All, Tank, Bruiser, Melee/Ranged Assassin,
  Healer, Support) + **hero grid**; unavailable ones greyed out ("unavailable"), hover = highlight.
  Click → `action` (assigns to the current step, advances). Right click/long press → toggle manual override.

### `/draft/overlay` — OBS overlay (1920×1080 browser source, transparent background)

**Timeline view** (the only overlay delivered; the player's perspective = the game's own screen).
Parameter `?skin=nexus|glass|tactical|mono` (default **nexus**). Composition:
- **Header**: map title, **team names** + **blinking dot on the active team**, **score**,
  **phase timer** (draining bar).
- **Series bans** (fearless): ribbon of struck-through mini icons of the unavailable pool.
- **Picks in chronological order**: grid of columns (1 per pick step), **blue at the top /
  red at the bottom** of an invisible line, empty columns = breathing room; **portrait** tiles with
  **hero + player name** written on them; active tile = pulsing gold halo.
- **Game bans**: central band of small struck-through tiles on the invisible line, chronologically.

The 4 skins = sets of CSS variables (colours, radius, clip-path, fonts). Web fonts
(Oswald, JetBrains Mono, Cinzel) loaded in production.

### Settled details

- **Map** = cosmetic (background + title), does not affect the rules.
- **Timer = per phase**: a phase = a block of consecutive actions of the same team (1 ban,
  1 pick, 2 picks…). The countdown runs until the turn passes to the opposing team,
  not per individual pick. Driven by the operator (start/reset) — no hard time constraint.

## Out of V1 scope (YAGNI)

Pick bot/AI, Jarvis suggestions (would go through an Intent later), talents, simultaneous
multiple operators, hard time constraint, public sharing of a draft, dedicated "player view" overlay
(covered by the game's screen).

## Acceptance criteria

1. The engine plays the 3 sequences end to end, refuses unavailable heroes, undo/reset OK
   (tests green).
2. From `/draft`, a complete Standard draft is built by clicking; `/draft/overlay` (Timeline)
   updates **live** (WS) on a 2nd screen/OBS, and `?skin=` changes the look.
3. Fearless: in game 2 of a series, the heroes picked in game 1 are automatically
   unavailable (series bans ribbon); the manual override works.
4. Team names + player names + score editable from the console and reflected in the overlay.
