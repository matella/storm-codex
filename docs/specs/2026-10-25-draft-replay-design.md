# Draft from a replay — design

> storm-codex spec, to be approved by the operator before any code (Rule no. 0). Brainstorm of
> 2026-10-25. Builds on the existing draft simulator (`docs/specs/2026-06-19-draft-simulator-design.md`).
> Points marked **[Claude's choice]** are decisions taken without the operator: to be reviewed.

## Goal

The operator casts games that have already been played: there is no live draft. They load a
`.StormReplay` file, that game's draft is **prepared** (map, players, complete sequence of
bans/picks) but **hidden**, then they **play it out by hand** (or with auto-play). The screen seen by
the audience is the existing OBS overlay `/draft/overlay`; the new page is only the remote control.

## Decisions made (with the operator)

1. **Dedicated page** `/draft/replay`: a `.StormReplay` is chosen/dropped there; the draft is prepared at
   step 0 (nothing revealed), then started manually.
2. **Output = existing OBS overlay**, reused (same draft state, same `draft.updated` WS) with the minimal tweaks described below.
3. **In V1 scope**: timed auto-play, queue of several replays (a series),
   and fearless across that series (the picks of previous games become unavailable).

## What the replay provides (already extracted by storm-stats, `process_draft`)

`match.bans` and `match.picks` per team, in each team's order; `picks.first` (first-pick
team); for each player their hero and name. The global order of the 16 steps is
rebuilt from the fixed sequence of the Standard format (same table as the parser's `selections`).
Bans are attribute codes (`Crus`…) to be resolved via `/api/dim/hero-attributes`; picks
are already canonical names. The simulator's hero key = canonical name (unchanged).

Builds < 66292: different ban sequence (2 bans per team in the 1st phase, marked "N/A") —
steps without a hero are **skipped** (see "empty step").

## Design

### Engine (`draft/mod.rs`) — minimal extension

- `DraftState` gains `replay: Option<ReplayScript>` (`#[serde(default)]`, `schema_version` 2):
  `{ source: String, script: Vec<Option<String>>, players: Vec<Option<String>> , playing: bool, interval_ms: u32 /* 30000 by default */ }`
  — `script[i]` = hero of step i (`None` = empty step), `players[i]` = name of the pick's player.
- `reveal_next()`: applies `script[cursor]` (or skips if `None`); reuses `apply`/cursor. The existing `undo`
  and `reset` serve to go back. `assignments` only contains **what has been revealed**:
  the overlay therefore never shows the future. The complete script is only served to the replay page.
- **Empty step**: a `skip()` is added (cursor +1, assignment `None`), displayed as "no ban" as on
  the match page.
- The prepared draft's format is **Standard**; **Fearless** if the queue is in fearless mode.
  `first_pick` / sides come from the replay (team 0 = Blue, team 1 = Red **[Claude's choice, to be checked
  on a real replay]**).

### Decoding

- `POST /api/draft/replay/load` (raw bytes, like `/api/upload-raw`): decodes **only** the draft
  (storm-replay + `process_draft`), **without writing the game to the database** **[Claude's choice]**: loading a
  replay for casting must not pollute the stats. Response: metadata (map, players, build, script).
- **Only custom games (Custom) are accepted** (operator decision): any other mode
  (Quick Match, ARAM, Unranked/Hero League…) is refused with a 4xx and a clear message.
- Budget: < 500 ms for a replay (parse < 150 ms + local network margin).

### Replay queue (series)

- `replay_queue: Vec<LoadedGame>` (picks/bans/players of each game) in the state, persisted like
  the rest (`draft_live`). Routes: add (`load`), remove, reorder, "play game N".
- A finished game → "next game" = the existing `start_next_game()`: in fearless mode, the
  accumulated picks of the previous games move into `series_bans` (ribbon already handled by the overlay).
- The score and team names stay manual (the replay does not contain them) **[Claude's choice]**; the
  names of the 5 players on each side are prefilled from the replay (mapped to their hero's pick).

### Playback

- Playback unit = the **phase** (block of consecutive steps of the same team, see
  `current_phase_len`). Standard sequence in 12 phases: ban, ban, ban, ban, 1 pick, 2 picks,
  2 picks, ban, ban, 2 picks, 2 picks, 1 pick (confirmed by the operator).
- **Manual**: Next button / Space key, Back (undo one phase), Reset. One click = one phase
  revealed **[Claude's choice: the 2 picks of a phase appear together]**.
- **Auto**: Play/Pause, **30 s per phase** (operator decision; adjustable, 30 s by default), server-side
  timer (authoritative, like the state) to survive a refresh; stops at the end of the draft.
  Total duration ≈ 6 min.
- Keyboard shortcuts active even when the page does not have the OBS focus (the replay page is in a
  browser separate from the browser source): nothing to do on the OBS side.

### Routes and events

`POST /api/draft/replay/load`, `/replay/next`, `/replay/back`, `/replay/play` `{playing, interval_ms}` (default 30000),
`/replay/queue/*`; `GET /api/draft/replay` (complete script, reserved for the page). No new event:
`draft.updated` is enough. Open mode (like the existing draft routes).

### Frontend

New route `/draft/replay` (React, existing tokens): drop zone, queue list (current game
highlighted), **non-spoiling** preview of the sequence (numbered steps, heroes hidden
until revealed), Next/Back/Reset/Play controls, fearless toggle, "next game" button.
`/draft/overlay`: **minimal** changes (mockups: `docs/draft-replay-mockup.html`) — "Draft ready — waiting" banner while the cursor is at 0 in replay mode; empty ban slots dotted; timer bar driven by the auto-play interval (instead of the fixed 45 s) and inactive in manual playback. **Player names are only shown with their revealed hero** (the public API `GET /api/draft` hides the name of an unrevealed pick in replay mode **[Claude's choice]**): nothing is spoiled.

## Out of scope

Importing the game into the database from this page, non-Custom replays, editing the script, talents,
multiple operators, auto-detection of team names, dedicated "replay" overlay.

## Acceptance criteria

1. A real draft replay (3 of the operator's replays) loads in < 500 ms; the overlay stays **empty**
   until the first "Next", then fills in the exact order of the real draft (bans and picks
   alternating as on the match page).
2. Back/Reset/auto-play (30 s per phase) work; refreshing the page or the overlay loses nothing.
3. Queue of 3 replays in fearless: in game 2, the picks of game 1 are unavailable; the
   player names are shown on the pick tiles.
4. A non-Custom replay (QM, ARAM, league…) is refused with a clear message; no game is added to the database.
5. Tests: engine (reveal/skip/undo/fearless), decoding on the mini-corpus, global order = the parser's `turn`.

## Docs to update with the code (Rule no. 0)

`docs/spec/05-api.md` (routes), `04-serveur.md` (state/persistence, queue), `08-frontend.md` (page
`/draft/replay`), `07-evenements.md` (unchanged: `draft.updated` reused), `docs/STATUS.md`.
