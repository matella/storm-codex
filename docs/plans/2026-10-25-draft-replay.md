# Draft from a replay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Load a Custom replay, prepare its hidden draft, play it out phase by phase (manual or auto 30 s) on the existing OBS overlay, with a replay queue and fearless.

**Architecture:** The `DraftState` engine (crate `storm-codex-server`, module `draft`) gains a `ReplayState` (queue of loaded games + script) and pure operations `reveal_phase`/`back_phase`. A "draft only" decoding (storm-stats `process_replay`, nothing written to the database) produces a `LoadedGame`. The `/api/draft/replay/*` routes mutate the state, persist (`draft_live`) and broadcast `draft.updated`. The React page `/draft/replay` is the remote control; `/draft/overlay` only gets small touch-ups (Task 5).

**Tech Stack:** Rust (axum, tokio, serde, sqlx), storm-stats, React + Vite + TS + TanStack Query. Consult Context7 (axum, TanStack Query) before using their APIs.

**Spec:** `docs/specs/2026-10-25-draft-replay-design.md` (also read `docs/specs/2026-06-19-draft-simulator-design.md`).

## Global Constraints

- Only **Custom** games are accepted; any other mode → 4xx with a message; **nothing is written to `matches`/`uploads`**.
- Playback unit = **phase** (block of consecutive steps with the same `Order`, `DraftState::current_phase_len`); Standard = 12 phases: ban, ban, ban, ban, 1 pick, 2 picks, 2 picks, ban, ban, 2 picks, 2 picks, 1 pick.
- Auto-play: **30,000 ms per phase** by default (`interval_ms`), adjustable.
- Budget: loading a replay < 500 ms (measured, not estimated).
- Team 0 = Blue, team 1 = Red (to check on a real replay — Task 2).
- Strict clippy, `unwrap()` forbidden outside tests (`unwrap_used = "deny"`), typed `thiserror` errors, conventional commits.
- **Rule no. 0**: each task that changes a contract updates `docs/spec/` in the same commit.
- The draft's `SCHEMA_VERSION` goes to 2; old persisted states (without `replay`) must still load (`#[serde(default)]`).

## Review Focus

- Old replay (build < 66292) or absent/`null` ban: the step is **skipped** ("no ban"), playback does not block (Task 1 + 2).
- Duplicate hero in the script (Custom without a uniqueness rule): the reveal must **not** fail (`Unavailable`) (Task 1).
- Empty body, truncated/corrupted file, non-Custom replay, huge file: clean 4xx, never 500/panic (Task 4).
- Next at the end, Back at the start, Next without a loaded game: 409 without panic or state change (Task 1 + 4).
- Server restart in the middle of auto playback: `playing` goes back to `false`, cursor kept (Task 4).
- Manual Next at the same time as the timer tick: a single phase revealed per action (single write lock) (Task 4).

---

### Task 1: Engine — replay types, per-phase reveal

**Files:**
- Create: `crates/storm-codex-server/src/draft/replay.rs` (types + pure operations)
- Modify: `crates/storm-codex-server/src/draft/mod.rs` (`mod replay; pub use replay::*;`, `replay` field, `SCHEMA_VERSION = 2`)
- Test: unit tests in `replay.rs`

**Interfaces:**
- Produces:
  - `pub struct LoadedGame { pub source: String, pub map: String, pub first_pick: Side, pub script: Vec<Option<String>> /*16, Standard step order*/, pub pick_players: Vec<Option<String>> /*16, aligned*/, pub blue_players: [String;5], pub red_players: [String;5] }` (Serialize/Deserialize/Clone/Debug)
  - `pub struct ReplayState { pub queue: Vec<LoadedGame>, pub current: Option<usize>, pub fearless: bool, pub playing: bool, pub interval_ms: u32 /*default 30_000*/, pub next_at_ms: Option<i64> }` + `Default`
  - `DraftState.replay: ReplayState` (`#[serde(default)]`)
  - `impl DraftState { pub fn reveal_phase(&mut self) -> Result<(), DraftError>; pub fn back_phase(&mut self) -> Result<(), DraftError>; pub fn select_game(&mut self, index: usize) -> Result<(), DraftError>; pub fn public(&self) -> DraftState }`
  - `DraftError` gains `NoGame`; `DraftError::Complete`/`NothingToUndo` reused.

- [ ] **Step 1: Tests (red)** in `replay.rs`: `reveal_phase_reveals_whole_two_pick_phase` (script of 16 distinct heroes `h0..h15`; 4 reveals → cursor 4; the 5th → 5; the 6th → cursor 7 and `assignments[5..7]` filled), `twelve_reveals_complete_standard` (12 calls → `is_complete()`, 13th → `Err(Complete)`), `back_phase_undoes_whole_phase` (after 6 reveals, `back_phase` → cursor 5, `assignments[5..7]` = None), `back_phase_at_start_errors` (`NothingToUndo`), `reveal_without_game_errors` (`NoGame`), `none_step_is_skipped` (script with `None` at steps 2,3: revealing these phases advances the cursor, assignment `None`), `duplicate_hero_does_not_fail` (script with `h0` at steps 0 and 5: 6 reveals OK), `select_game_fearless_seeds_series_bans` (queue of 2 games, `fearless=true`, `select_game(1)` → `series_bans` == picks (non-`None`) of game 0, cursor 0, format `Fearless`, teams/players/map/first_pick of game 1), `select_game_non_fearless_has_no_series_bans`, `public_hides_scripts` (`public().replay.queue[i].script` empty, `pick_players` empty), `public_hides_names_of_unrevealed_picks` (in replay mode, after only 1 ban phase, `public().blue.players` and `.red.players` are all empty; after the reveal of the 1st Blue pick, only `blue.players[0]` is filled in).
- [ ] **Step 2:** `cargo test -p storm-codex-server draft::replay` → compile/test failures expected.
- [ ] **Step 3: Implement** types and methods. `reveal_phase`: `len = current_phase_len()`, for each of the `len` steps write `script[cursor]` directly into `assignments` (without going through `apply`, so without a uniqueness check) and advance; `back_phase`: go back to the start of the previous phase block (compute the phase bounds from `steps`, not from the cursor alone). `select_game`: `*self = DraftState::new(format, g.first_pick, g.map)` keeping `score`, `bo`, `manual_unavailable`, `replay`, team names; fill `blue.players`/`red.players`; `series_bans` = union of the picks of `queue[..index]` if `fearless`. `public()`: clone with scripts/`pick_players` emptied, and `blue.players[k]`/`red.players[k]` emptied as long as the k-th pick of that side (step order) is not revealed — only if `replay.current` is `Some`.
- [ ] **Step 4:** same tests → PASS; `cargo clippy -p storm-codex-server --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(draft): replay engine — per-phase reveal, queue, fearless`

### Task 2: Extracting the draft of a Custom replay

**Files:**
- Modify: `crates/storm-codex-server/src/draft/replay.rs` (addition), `crates/storm-codex-server/src/read.rs` (extract `pub(crate) fn hero_attribute_map() -> HashMap<String, String>` from `dim_hero_attributes`, which calls it)
- Create: `crates/storm-codex-server/src/draft/fixtures/custom_draft_match.json` (reduced `match_` + `players` output of a real Custom replay of the operator, via `fetch-corpus`/local corpus)
- Test: `replay.rs`

**Interfaces:**
- Consumes: `storm_stats::Output { status, match_, players }`, `hero_attribute_map()`.
- Produces: `pub fn loaded_game_from_output(out: &storm_stats::Output, source: &str, attrs: &HashMap<String,String>) -> Result<LoadedGame, ReplayError>`; `#[derive(thiserror::Error)] pub enum ReplayError { NotCustom, NoDraft, Parse(i64) }`.

- [ ] **Step 1:** On a real Custom replay, dump `match_.mode`, `bans`, `picks` (+`first`), and for each player `team`, `hero`, `name`, `turn`; **check** the shape (picks = hero names? bans = `{hero, order, absolute}` with attribute codes?) and the correspondence team 0 = Blue. Reduce into a JSON fixture. If a hypothesis of the plan is wrong, fix the spec before continuing.
- [ ] **Step 2: Tests (red)**: `custom_fixture_script_matches_turns` (the rebuilt script: for each player, `script.iter().position(hero) == turn` from the parser), `bans_resolved_to_canonical_names` (no 4-letter code in the script), `non_custom_rejected` (mode ≠ Custom → `NotCustom`), `old_build_has_none_steps` (fixture modified to build 66000: steps 2 and 3 = `None`), `status_not_one_is_parse_error`, `pick_players_follow_pick_owner` (the nickname in `pick_players[i]` is that of the player whose hero is `script[i]`).
- [ ] **Step 3: Implement** the order of the 16 steps as an exact mirror of `selections` in `process_draft` (`crates/storm-stats/src/process.rs:2343-2380`): First = team `picks.first`; the Custom mode is tested with `storm_stats::constants::rt_int("GameMode","Custom")`. Hero ban `null`/absent or unknown code → `None`. `first_pick` = Blue if team 0.
- [ ] **Step 4:** tests PASS + clippy.
- [ ] **Step 5: Commit** `feat(draft): extract the draft of a Custom replay (step script)`

### Task 3: Routes, auto-play timer, persistence

**Files:**
- Create: `crates/storm-codex-server/src/draft/replay_api.rs`
- Modify: `draft/mod.rs` (`pub mod replay_api;`), `draft/api.rs` (`get_draft` returns `public()`), `main.rs` (routes + `replay_timer: Arc<Mutex<Option<JoinHandle<()>>>>` in `AppState` + reset to `playing=false` when the state is loaded), `docs/spec/05-api.md`, `docs/spec/04-serveur.md`, `docs/spec/07-evenements.md` (note: `draft.updated` reused)
- Test: axum tests in `main.rs` (same style as the existing ones, `tower::ServiceExt::oneshot`)

**Interfaces:**
- Consumes: Task 1 (`reveal_phase`, `back_phase`, `select_game`, `public`), Task 2 (`loaded_game_from_output`), `store::save`, `persist`.
- Produces (all mutate under a single `write()`, then `persist` + `draft.updated`):
  - `POST /api/draft/replay/load` — body = bytes of the `.StormReplay` (optional `X-Filename` header, via `upload::filename`); written to a temporary file, `spawn_blocking(process_replay)`, added to `queue`, selects the game if it is the first; 201 + public `ReplayState`; 422 `{error:"not_custom"|"no_draft"|"parse_failed"}`; 400 empty body; 413 beyond the server's existing limit.
  - `DELETE /api/draft/replay/queue/{i}`, `POST /api/draft/replay/queue/move` `{from,to}`, `POST /api/draft/replay/select` `{index}`, `POST /api/draft/replay/fearless` `{value}` (re-selects the current game), `POST /api/draft/replay/next`, `/back`, `/reset`, `POST /api/draft/replay/play` `{playing: bool, interval_ms?: u32}` → 200 state; 409 on `DraftError`.
  - `GET /api/draft/replay` → **complete** `ReplayState` (scripts) for the replay page.
  - Timer: `play(true)` cancels any timer, spawns a task that sleeps `interval_ms`, reveals a phase (same `write()` + `persist`), updates `next_at_ms`, and stops at the end of the draft (`playing=false`); `play(false)`, `reset`, `back`, `select`, `load` cancel the timer. The 1st phase is revealed **after** a full interval (the plan's choice; "Next" gives the 1st immediately).

- [ ] **Step 1: Tests (red)**: `load_rejects_empty_body_400`, `load_rejects_non_custom_422_and_writes_nothing` (body = `silver-city-aram.StormReplay` from `crates/storm-stats/tests/data`; 422 and `SELECT count(*) FROM uploads` unchanged if a test DB is available, otherwise assert on the absence of a projection call), `next_without_game_409`, `next_at_end_409`, `back_at_start_409`, `get_draft_hides_scripts` (`/api/draft` contains no unrevealed hero), `get_replay_returns_full_script`, `play_true_then_false_cancels_timer` (`interval_ms` = 20 ms with `tokio::time::pause`/`advance`: after 1 tick → cursor 1, `play false`, new delay → cursor unchanged), `manual_next_and_tick_never_double_reveal`, `startup_resets_playing` (persisted state with `playing=true` → loaded `playing=false`, cursor kept).
- [ ] **Step 2:** `cargo test -p storm-codex-server draft` → failures expected.
- [ ] **Step 3: Implement** the handlers (typed errors → status); reuse `persist`. Update the 3 spec docs (routes, queue state/persistence, `draft.updated`).
- [ ] **Step 4:** tests PASS; `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(server): /api/draft/replay/* routes — loading, manual and auto playback`

### Task 4: Page `/draft/replay`

**Files:**
- Create: `web/src/pages/DraftReplay.tsx`, `web/src/draftReplay.test.ts` (pure logic: phase labels)
- Modify: `web/src/api.ts` (functions + types `ReplayState`/`LoadedGame` next to `DraftState`), `web/src/App.tsx` (route `draft/replay` next to `draft`), `docs/spec/08-frontend.md`, `docs/STATUS.md`
- Test: `web/src/draftReplay.test.ts`

**Interfaces:**
- Consumes: Task 3 routes; existing `useDraft()`/`["draft"]` invalidation (WS `draft.updated`).
- Produces: `export const draftReplayLoad = (file: File) => …`, `draftReplayNext/Back/Reset`, `draftReplayPlay(playing, intervalMs?)`, `draftReplaySelect(i)`, `draftReplayFearless(v)`, `draftReplayRemove(i)`, `fetchDraftReplay()`; `export function phaseLabels(steps: Step[]): {action: "ban"|"pick"; side: "first"|"second"; count: number}[]` (12 items for Standard).

- [ ] **Step 1: Test (red)** `phaseLabels`: Standard → 12 phases, counts `[1,1,1,1,1,2,2,1,1,2,2,1]`, actions `ban×4, pick×3, ban×2, pick×3`.
- [ ] **Step 2:** `npm test` (vitest, like `session.test.ts`) → failure.
- [ ] **Step 3: Implement** `phaseLabels` then the page (existing theme tokens, same classes as `Draft.tsx`): drop zone (accepts `.StormReplay`, several files → sent in order), queue (current game highlighted, removal, selection, order), fearless toggle, rail of the 12 phases (numbered, heroes hidden until revealed, current phase marked), Next/Back/Reset/Play-Pause buttons + duration field (seconds, 30 by default) + countdown from `next_at_ms`, shortcuts Space (Next), Backspace (Back). Show the readable 422 error message ("not a custom game").
- [ ] **Step 4:** `npm test`, `npm run build`, `npx tsc --noEmit` → green.
- [ ] **Step 5: Browser check** (preview launch.json :8089): load a real Custom, check empty overlay → reveal in order → comparison with the match page; measure the loading (< 500 ms); test a queue of 3 replays in fearless, Back/Reset, auto playback (reduce the interval to 2 s for the test), refresh in the middle of playback, rejection of a QM/ARAM replay. Record the measurements in `docs/STATUS.md`.
- [ ] **Step 6: Commit** `feat(web): /draft/replay page — remote control of the draft from a replay`

### Task 5: OBS overlay touch-ups

**Files:**
- Modify: `web/src/pages/DraftOverlay.tsx`, `web/src/pages/draft-overlay.css`, `web/src/api.ts` (type `DraftState.replay?`), `docs/spec/08-frontend.md`
- Test: `web/src/draftOverlay.test.ts` (extracted pure functions)

**Interfaces:**
- Consumes: `DraftState.replay` public (`current`, `playing`, `interval_ms`, `next_at_ms`).
- Produces: `export function timerFraction(replay: ReplayPublic | undefined, now: number, fallbackSecs: number, secs: number): number` (0 to 1); `export function isReplayReady(d: DraftState): boolean` (`replay.current != null && cursor === 0`).

- [ ] **Step 1: Tests (red)**: `isReplayReady` true at cursor 0 with a current game, false otherwise and false outside replay mode; `timerFraction` = 0 if the replay is not in auto playback, = `(next_at_ms - now) / interval_ms` bounded to [0,1] if auto playback, = `secs/fallbackSecs` outside replay mode.
- [ ] **Step 2:** `npm test` → failure.
- [ ] **Step 3: Implement** the two functions and wire up `Timeline`: `.ready` banner ("Draft ready — waiting", mockup style) under the header when `isReplayReady`; timer via `timerFraction` (refreshed every second); empty bans dotted (`.bn.hidden`/`.empty` like the mockup). No other rendering change outside replay mode.
- [ ] **Step 4:** `npm test`, `npm run build` green; compare visually with `docs/draft-replay-mockup.html` (3 states) during the Task 4 browser check.
- [ ] **Step 5: Commit** `feat(web): draft overlay — ready/replay states (banner, auto playback timer)`

