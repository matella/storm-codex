# Cast control room — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `/cast` page drives a series (teams, map pool, games, lineups, draft, in-game banner) and a single OBS source `/cast/overlay` follows the chosen scene.

**Architecture:** A server module `cast` holds the series state (JSON singleton persisted in `cast_live`, like `draft_live`), a pure engine tested without I/O, `/api/cast/*` routes that persist then broadcast `cast.updated` on the WS. The series is authoritative for teams/score/Bo; `DraftState` keeps its fields and receives a one-way sync. The front adds `/cast` (control room, layout "B") and `/cast/overlay` (scenes); the existing draft is extracted into components reused by the control room.

**Tech Stack:** Rust (axum, tokio, serde, sqlx, thiserror), storm-stats, React + Vite + TS + TanStack Query, vitest. **Query Context7 (axum, TanStack Query, React Router) before using their APIs** (project rule).

**Spec:** `docs/specs/2026-10-25-cast-control-room-design.md` (+ mockups `docs/specs/2026-10-25-cast-mockups/`). Also read `docs/specs/2026-10-25-draft-replay-design.md`.

## Global Constraints

- **Rule no. 0**: any task that changes a contract updates `docs/spec/` in the same commit (05-api, 04-serveur, 06-modele-donnees, 07-evenements, 08-frontend).
- Rust: strict clippy (`-D warnings`), `unwrap()` forbidden outside tests, typed `thiserror` errors, conventional commits. No `unwrap` in integration tests without a local `#![allow(clippy::unwrap_used)]` (see `players/testutil.rs`).
- DB tests: `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex cargo test -p storm-codex-server` (without it the DB tests skip silently — **always provide it**). Front: `cd web && npm test && npm run build`.
- A single OBS source `/cast/overlay`, 1920×1080, transparent background; scene change → overlay updated **< 500 ms** (measured).
- Scenes: `lineups`, `series`, `draft`, `ingame`, `hide`. UI labels in **English**.
- Map statuses: `available` · `banned(by)` · `picked(by)` · `decider`; **no imposed order**; a single `decider`; a game's map is chosen among `picked`/`decider`.
- Played map: "**WON**" in the winning team's colour, **without numbers**; gold "**NEXT**" on the next one.
- In-game tabs: **rectangles** (no angles), inner top corner anchored at **39.2%** (Blue) and **59.65%** (Red) of the width, **y = 9.9%**, height **4.6%**, max width **26%**, name truncated with "…"; top-right badge = `BO{n}` + `GAME {k}` + format (`FEARLESS`/`STANDARD`/`NORMAL`). Anchors = **tunable constants**.
- Lineups: mode A (no finished game) = the 3 most played heroes over the **last 20 games** (all modes), **hidden under 5 games**; **no win rate**; free-text line. Mode B = heroes played in the series, Fearless-locked ones dimmed; auto switch A→B at the 1st finished game, manual override.
- Logos: PNG/WebP/JPEG **≤ 512 KB** in the database, or a URL. Post-game: **Custom-only** replay, **nothing written to `matches`/`uploads`**, compact summary kept in the series state.
- A breakdown value missing from the storm-stats output is **removed from the screen**, never invented; no new metric in storm-stats.
- `SCHEMA_VERSION` of the series state = 1; an older/unknown persisted state must load or fall back to a fresh state, never make startup fail.

## Review Focus

- Team names of 40+ characters or empty: tab truncated without spilling over the HUD; empty → "Blue"/"Red" (Task 5).
- Invalid series operations (map outside the pool, two `decider`s, banned then picked, winner of an unknown game/set twice, starting a game on a map not chosen): 409, no state change (Task 1 + 2).
- Post-game replay not Custom, truncated, or no replay for a game: clean refusal / empty slides, never a 500 or a broken screen (Task 9).
- Player without results, several toons, or < 5 recent games: hero row hidden, card always clean (Task 8).
- Logo too big or of the wrong type, empty body: 413/415/400, nothing stored (Task 3).

---

### Task 1: Series engine (pure)

**Files:**
- Create: `crates/storm-codex-server/src/cast/mod.rs` (types, `CastError`), `crates/storm-codex-server/src/cast/series.rs` (pure operations + tests)
- Modify: `crates/storm-codex-server/src/main.rs` (`mod cast;`)

**Interfaces:**
- Produces (reuses `draft::{Side, Format}`):
  - `pub enum Scene { Lineups, Series, Draft, Ingame, Hide }` (serde `snake_case`, default `Hide`)
  - `pub enum MapStatus { Available, Banned { by: Side }, Picked { by: Side }, Decider }` (serde tagged `status`)
  - `pub struct PoolMap { pub name: String, #[serde(flatten)] pub status: MapStatus }`
  - `pub struct Player { pub name: String, pub tagline: String, pub toon: Option<String> }` (default empty)
  - `pub struct Team { pub name: String, pub logo: Option<String>, pub record: String, pub players: [Player; 5], pub captain: Option<u8> }`
  - `pub struct GameRecord { pub number: u8, pub map: String, pub winner: Option<Side>, pub takedowns: [u32; 2], pub duration_secs: Option<u32>, pub summary: Option<GameSummary> }` (`GameSummary` defined in Task 9; meanwhile an empty `pub struct GameSummary {}` `#[derive(Default)]`)
  - `pub enum LineupMode { Auto, A, B }`; `pub struct SlideState { pub game: Option<u8>, pub index: u8, pub auto: bool, pub interval_ms: u32 /*default 12_000*/ }`
  - `pub struct Series { schema_version: i32, active: bool, blue: Team, red: Team, bo: u8, format: Format, score_override: Option<[u8;2]>, pool: Vec<PoolMap>, games: Vec<GameRecord>, scene: Scene, lineup_mode: LineupMode, slide: SlideState }` + `Series::new()`
  - `pub enum CastError { UnknownMap, NotInPool, DuplicateDecider, MapNotChosen, UnknownGame, WinnerAlreadySet, SeriesInactive }`
  - `impl Series { pub fn set_pool(&mut self, names: &[String]); pub fn mark_map(&mut self, name: &str, status: MapStatus) -> Result<(), CastError>; pub fn start_game(&mut self, map: &str) -> Result<u8, CastError>; pub fn set_winner(&mut self, number: u8, side: Side) -> Result<(), CastError>; pub fn score(&self) -> [u8; 2]; pub fn next_map(&self) -> Option<&str>; pub fn lineup_mode_effective(&self) -> LineupMode /* A or B */; pub fn display_name(&self, side: Side) -> &str /* "Blue"/"Red" if empty */ }`

- [ ] **Step 1: Tests (red)** in `series.rs`: `set_pool_keeps_status_of_retained_maps_and_drops_others`, `mark_map_rejects_map_outside_pool` (`NotInPool`), `second_decider_is_rejected` (`DuplicateDecider`, state unchanged), `marking_banned_then_picked_replaces_status` (last status wins), `start_game_requires_picked_or_decider` (`MapNotChosen` for `Available`/`Banned`), `start_game_numbers_games_from_one_and_rejects_replaying_a_map` (the same map twice → `MapNotChosen`), `set_winner_unknown_game` (`UnknownGame`), `set_winner_twice` (`WinnerAlreadySet`), `score_counts_winners_unless_overridden`, `next_map_is_first_picked_or_decider_without_a_game` (pool order), `lineup_mode_auto_switches_to_b_after_first_finished_game`, `lineup_mode_forced_wins_over_auto`, `display_name_falls_back_to_blue_red_when_blank`, `series_roundtrips_json_and_loads_default_for_missing_fields` (partial JSON `{}` → defaults, no error).
- [ ] **Step 2:** `cargo test -p storm-codex-server cast::` → rouge.
- [ ] **Step 3: Implement** the types + methods above (`#[serde(default)]` everywhere for schema tolerance; `Series::new()` = `active: false`, `bo: 5`, `format: Fearless`, scene `Hide`). `mark_map` accepts `Decider` only if no other map is one.
- [ ] **Step 4:** tests PASS; `cargo clippy -p storm-codex-server --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(cast): pure series engine — pool, games, score, lineup modes`

### Task 2: Persistence, `/api/cast/*` routes, WS

**Files:**
- Create: `crates/storm-codex-server/migrations/0012_cast_live.sql`, `crates/storm-codex-server/src/cast/store.rs`, `crates/storm-codex-server/src/cast/api.rs`
- Modify: `main.rs` (field `cast: Arc<RwLock<cast::Series>>` in `AppState`, loaded at startup with a fresh-state fallback, routes), `docs/spec/{04-serveur,05-api,06-modele-donnees,07-evenements}.md`
- Test: integration tests in `cast/api.rs` (model: `draft/replay_api.rs`, `players/testutil.rs`)

**Interfaces:**
- Produces: table `cast_live (id INT PK DEFAULT 1 CHECK (id=1), state JSONB NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now())`; `store::load(&PgPool) -> Option<Series>`, `store::save(&PgPool, &Series) -> Result<(), sqlx::Error>`; `async fn persist(&AppState)` (save + `events.send({"type":"cast.updated"})`).
- Routes (JSON): `GET /api/cast` → `Series`; `POST /api/cast/series` `{blue:{name}, red:{name}, bo, format, pool:[name]}` (creates/restarts: `active=true`, games/score reset); `PUT /api/cast/teams` `{blue: Team, red: Team}`; `POST /api/cast/pool/map` `{name, status}`; `POST /api/cast/games/start` `{map}` → `{number}`; `POST /api/cast/games/{n}/winner` `{side, takedowns?, duration_secs?}`; `POST /api/cast/score` `{blue, red}|null`; `POST /api/cast/scene` `{scene}`; `POST /api/cast/lineup-mode` `{mode}`; `POST /api/cast/slide` `{game?, index?, auto?, interval_ms?}`. `CastError` errors → `409 {"error": "<Variant>"}` with no state change. Mutations: a single write-lock, then `persist`.

- [ ] **Step 1: Tests (red)**: `get_cast_returns_default_inactive_series`, `post_series_activates_and_resets_games`, `invalid_map_marking_returns_409_and_leaves_state` (map outside the pool), `scene_change_broadcasts_cast_updated` (a subscriber to `events` receives `{"type":"cast.updated"}`), `state_survives_restart` (save → identical `store::load`), `load_of_garbage_json_returns_none` (an unreadable `{"x":1}` row → `None` is **not** required: a partial `{}` loads defaults; non-object JSON → `None`), `scene_switch_to_overlay_poll_under_500ms` (POST scene then GET: < 500 ms, measured with `Instant`).
- [ ] **Step 2:** `cargo test -p storm-codex-server cast::` → rouge (migration/routes absentes).
- [ ] **Step 3: Implement** the migration, store, handlers, register the routes **before** the generic routes, load `cast` into `AppState` (`store::load(&db).await.unwrap_or_else(Series::new)`). Open mode like `/api/draft`.
- [ ] **Step 4:** tests PASS + clippy; update the 4 spec docs (routes, table, `cast.updated` event without state, module).
- [ ] **Step 5: Commit** `feat(cast): /api/cast routes, cast_live persistence, cast.updated event`

### Task 3: Series → draft sync, logos

**Files:**
- Create: `crates/storm-codex-server/src/cast/sync.rs`, `crates/storm-codex-server/migrations/0013_cast_logo.sql`
- Modify: `cast/api.rs` (sync call after each teams/score/Bo/format mutation; logo routes), `draft/api.rs` (`teams`/`score`/`config` refuse with `409 {"error":"series_active"}` when `cast.active`), `main.rs` (routes), `docs/spec/{05-api,06-modele-donnees}.md`
- Test: in `sync.rs` and `cast/api.rs`

**Interfaces:**
- Consumes: `Series`, `DraftState` (fields `blue`, `red` = `TeamInfo{name, players:[String;5]}`, `score:[u8;2]`, `bo`, `format`).
- Produces: `pub fn sync_into_draft(series: &Series, draft: &mut DraftState)` (copies names, player names, score = `series.score()`, `bo`, **without** touching cursor/assignments/replay queue); table `cast_logo (side TEXT PRIMARY KEY CHECK (side IN ('blue','red')), mime TEXT NOT NULL, bytes BYTEA NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now())`; routes `POST /api/cast/logo/{side}` (body = bytes, `Content-Type` header) and `GET /api/cast/logo/{side}`; after upload `Team.logo = Some("/api/cast/logo/{side}?v={updated_at_ms}")`.

- [ ] **Step 1: Tests (red)**: `sync_copies_names_players_score_bo`; `sync_leaves_draft_progress_untouched` (cursor, assignments, `replay.queue` identical before/after); `draft_teams_route_returns_409_when_series_active` and `..._200_when_inactive`; `logo_upload_stores_and_serves_same_bytes` (minimal PNG); `logo_over_512kb_is_413`; `logo_wrong_mime_is_415` (`image/gif`); `logo_empty_body_is_400`; no bytes stored in the three error cases.
- [ ] **Step 2:** red. **Step 3: Implement** (`DefaultBodyLimit` allows up to 513 KB to return a clean 413). **Step 4:** PASS + clippy + docs. **Step 5: Commit** `feat(cast): one-way series→draft sync, logos in the database`

### Task 4: Couche API front + utilitaires purs

**Files:**
- Create: `web/src/castApi.ts` (types mirroring `Series`, fetchers, `useCast()`), `web/src/castState.ts` (+ `castState.test.ts`)
- Modify: `web/src/api.ts` (in `useLiveUpdates`, `cast.updated` → `qc.invalidateQueries({ queryKey: ["cast"] })`)

**Interfaces:**
- Produces: `export const fetchCast = () => get<CastSeries>("/api/cast")` and one fetcher per Task 2 route (`castSeries`, `castTeams`, `castMarkMap`, `castStartGame`, `castWinner`, `castScore`, `castScene`, `castLineupMode`, `castSlide`, `castLogoUpload(side, file)`); `useCast(): UseQueryResult<CastSeries>` (key `["cast"]`). `castState.ts`: `mapCardView(m: PoolMap, games: GameRecord[], nextMap: string|null): { tone: "blue"|"red"|"gold"|"muted"|"none"; label: string; struck: boolean }` ("WON" + winner's colour; gold "NEXT"; `banned by X` struck through; `picked by X`; `decider`); `effectiveLineupMode(s: CastSeries): "A"|"B"`; `displayName(t: CastTeam, side: Side): string`.

- [ ] **Step 1: vitest tests (red)** `castState.test.ts`: `mapCardView` — played map won by Blue → `{tone:"blue", label:"WON"}`; next map → `{tone:"gold", label:"NEXT"}`; banned by Red → `struck:true`; `effectiveLineupMode` follows the auto/forced rule; empty `displayName` → "Blue".
- [ ] **Step 2:** `cd web && npx vitest run castState` red. **Step 3: Implement.** **Step 4:** PASS + `npm run build` (types). **Step 5: Commit** `feat(web): cast API layer + pure helpers`

### Task 5: `/cast/overlay` overlay — shell, In-game scene, Hide

**Files:**
- Create: `web/src/pages/CastOverlay.tsx`, `web/src/cast/anchors.ts` (+ `anchors.test.ts`), `web/src/cast/InGameScene.tsx`, `web/src/cast/cast-overlay.css`
- Modify: `web/src/App.tsx` (route outside `Layout`, like `draft/overlay`), `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes: `useCast`, `displayName`, `OverlayFrame`-style transparent background.
- Produces: `export const INGAME_ANCHORS = { blueInnerX: 0.392, redInnerX: 0.5965, topY: 0.099, tabH: 0.046, maxW: 0.26 }`; `export function tabBox(side: "blue"|"red", a = INGAME_ANCHORS): { top: string; height: string; maxWidth: string; left?: string; right?: string }` (percentages with 2 decimals; Blue: `right = (1 - blueInnerX)`, Red: `left = redInnerX`); `CastOverlay` renders the current scene (switch on `scene`), `data-skin` via `?skin=` like `DraftOverlay`, opacity transition ≈ 300 ms; `draft`/`series`/`lineups` scene = empty component until Tasks 7/8/10.

- [ ] **Step 1: Tests (red)** `anchors.test.ts`: `tabBox("blue")` → `{top:"9.90%", height:"4.60%", maxWidth:"26.00%", right:"60.80%"}`; `tabBox("red")` → `left:"59.65%"`; `tabBox` accepts overridden anchors (fine offset).
- [ ] **Step 2:** red. **Step 3: Implement**: `InGameScene` = two rectangular tabs (name `overflow:hidden; text-overflow:ellipsis; white-space:nowrap`, series score, team-colour underline) + top-right badge (`BO{bo}`, `GAME {games.length+1}`, format in capitals); names via `displayName`. Light component test: 40-character name → the element has `text-overflow: ellipsis` (computed style via `getComputedStyle` or class) and does not exceed `maxWidth`.
- [ ] **Step 4:** `npm test && npm run build`; check in the browser (`preview_start`): `POST /api/cast/series` then `POST /api/cast/scene {"scene":"ingame"}`, the overlay changes without reloading; screenshot with the real capture `docs/specs/2026-10-25-cast-mockups/ingame.webp` as background (corners aligned on the red dots). Docs 08-frontend.
- [ ] **Step 5: Commit** `feat(web): /cast/overlay overlay — In-game scene, Hide, tunable anchors`

### Task 6: `/cast` control room — shell and series rail

**Files:**
- Create: `web/src/pages/Cast.tsx`, `web/src/cast/SeriesRail.tsx` (teams, score, Bo, mode, logos, lineup entry), `web/src/cast/PoolPanel.tsx` (pool + veto + games), `web/src/cast/ScenePreview.tsx`, `web/src/cast/cast.css`
- Modify: `web/src/App.tsx` (route `cast` in `Layout`), `web/src/components/Layout.tsx` ("Cast" link), `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes: fetchers/`useCast` (Task 4); list of selectable maps = the `MAPS` constant of `pages/Draft.tsx` (**move** it into `web/src/castState.ts` and import it from both sides — a single list); visuals via `mapImage(map)` from `api.ts` (may return a 404 image → plain fallback background).
- Produces: layout B: left rail (Series / Teams & lineups / Maps), scene tab bar with an "on air" indicator, `ScenePreview` = `<iframe src="/cast/overlay">` 1920×1080 scaled (CSS `transform: scale`); per-scene workspace (`ingame`: score steppers + badge correction; `hide`: nothing; the others: slots filled in Tasks 7–10); `PoolPanel`: pool checkboxes (≈ 10 maps), Ban Blue/Ban Red/Pick Blue/Pick Red/Decider/Reset buttons per map, "Start game" (list of chosen maps), winner per game.

- [ ] **Step 1: vitest tests (red)** on the form helpers extracted as pure functions in `castState.ts`: `togglePool(names: string[], map: string): string[]` (add/remove, stable order), `startableMaps(s: CastSeries): string[]` (`picked`/`decider` maps without a game), `canSetWinner(s, n): boolean`.
- [ ] **Step 2:** red. **Step 3: Implement** the page + panels (English labels, `ds.css`/folder tokens). **Step 4:** `npm test && npm run build`; check in the browser: create a series (2 teams, Bo5, pool of 10), mark bans/picks in any order, start a game, set a winner; the preview and the score follow; refreshing the page loses nothing. Docs 08.
- [ ] **Step 5: Commit** `feat(web): /cast control room — series rail, map pool, scenes and preview`

### Task 7: Series scene — map strip

**Files:**
- Create: `web/src/cast/SeriesScene.tsx`; Modify: `CastOverlay.tsx`, `Cast.tsx` (Series workspace: choice of the displayed game, current slide, auto/manual — wired in Task 9), `cast-overlay.css`

**Interfaces:**
- Consumes: `mapCardView`, `useCast`, `mapImage(map)` visuals (`api.ts`).
- Produces: `SeriesScene` = header (names + series score) + strip of all the pool maps (one card = visual, name, "picked/banned by X", `WON` in team colour, gold `NEXT`, `decider`, banned ones struck through) + slide area (empty until Task 9).

- [ ] **Step 1: Tests (red)** component: pool of 6 maps (1 played won by Blue, 1 played won by Red, 1 next, 1 decider, 2 banned) → the render contains exactly one "NEXT", two "WON" with the classes `tone-blue`/`tone-red`, two `struck` elements, no score number on the cards.
- [ ] **Step 2–4:** red → implement → `npm test && npm run build` + browser check (`series` scene). **Step 5: Commit** `feat(web): Series scene — map strip (WON/NEXT)`

### Task 8: Lineups — recent heroes (server) and A/B team cards

**Files:**
- Create: `crates/storm-codex-server/src/cast/lineup.rs`, `web/src/cast/LineupsScene.tsx`, `web/src/cast/PlayerPicker.tsx`
- Modify: `cast/api.rs` + `main.rs` (route), `Cast.tsx` (Lineups workspace: Auto/A/B mode, picker), `docs/spec/{05-api,08-frontend}.md`
- Test: in `lineup.rs` (DB, model `players/testutil.rs`); vitest for the scene

**Interfaces:**
- Consumes: tables `match_players(match_id, toon_handle, hero)`, `matches(played_at)`, name history (`players::names`), `GET /api/players/search`.
- Produces: `pub async fn recent_heroes(db: &PgPool, toon: &str) -> Result<Vec<HeroCount>, sqlx::Error>` (`HeroCount{hero:String, games:u32}`, ≤ 3, sorted by games then recency, over the last 20 games **all modes**; **empty if < 5 games**); `GET /api/cast/lineup?blue=toon,toon,…&red=…` → `{ blue: [[HeroCount]], red: [[HeroCount]] }` (budget < 100 ms for 10 players, measured); `PlayerPicker` (name field → `players/search` → result list `toon, name, games` → pick; no result = "unlinked" player, never a 1st result taken silently); `LineupsScene` mode A (3 recent hero portraits if present + free-text line) / mode B (heroes played in the series per player from `games[].summary`, Fearless-locked ones dimmed).

- [ ] **Step 1: Tests (red)** server: `recent_heroes_top3_over_last_20_games` (25 games inserted, only the window of the last 20 counts), `recent_heroes_hidden_under_5_games` (4 games → empty), `recent_heroes_counts_all_modes`, `lineup_route_under_100ms_for_10_players`; vitest: `LineupsScene` mode A shows **no** percentage or "win"; mode B dims locked heroes; player without toon → no hero row.
- [ ] **Step 2–4:** red → implement (parameterised query, existing indexes to check with `EXPLAIN` in the budget test) → PASS + clippy + `npm test && npm run build` + browser. **Step 5: Commit** `feat(cast): recent heroes per player + A/B team cards`

### Task 9: Post-game — replay summary, slides, slideshow

**Files:**
- Create: `crates/storm-codex-server/src/cast/summary.rs`, `crates/storm-codex-server/src/cast/fixtures/custom_summary.json` (reduced output of a real Custom replay), `web/src/cast/PostGameSlide.tsx`
- Modify: `cast/mod.rs` (full `GameSummary`), `cast/api.rs` + `main.rs` (route), `web/src/cast/SeriesScene.tsx`, `Cast.tsx` (slideshow controls), `docs/spec/{05-api,06-modele-donnees,08-frontend}.md`

**Interfaces:**
- Consumes: `storm_stats::process_replay`, `storm_stats::Output { match_, players, status }` (usage model: `draft/replay_api.rs::load`).
- Produces: `pub struct GameSummary { pub duration_secs: u32, pub winner: Side, pub takedowns: [u32;2], pub team_level: [u32;2], pub structures: [u32;2], pub merc_camps: [u32;2], pub objectives: [u32;2], pub bans: [Vec<String>;2], pub players: Vec<PlayerSummary>, pub xp_lead: Vec<(u32, i32)> /*(s, Blue lead)*/ }`; `pub struct PlayerSummary { pub side: Side, pub name: String, pub hero: String, pub takedowns: u32, pub deaths: u32, pub assists: u32, pub hero_damage: u64, pub siege_damage: u64, pub healing: u64 }`; `pub fn summary_from_output(out: &storm_stats::Output) -> Result<GameSummary, SummaryError>` (`SummaryError { NotCustom, Parse(i64), Missing(&'static str) }`); `POST /api/cast/games/{n}/replay` (raw bytes, like `draft/replay/load`) → 201 `GameSummary`, 422 `{error: not_custom|parse_failed|…}`; **nothing written to the database**; `winner`/`takedowns`/`duration_secs` of the `GameRecord` filled from the summary.

- [ ] **Step 1 (data check, before any screen code)**: on the fixture and 3 real Custom replays, list in the storm-stats output the JSON paths of each breakdown value (`match.teams[t].level`, `.stats.mercCaptures`, `.stats.structures`, `match.objective`, `match.winner`, `match.length`, `players[].gameStats.{HeroDamage,SiegeDamage,Healing,Deaths,Takedowns/SoloKill,Assists}`, `match.levelAdvTimeline`, `match.XPBreakdown`); **keep only what exists** (XP lead: `XPBreakdown` per team if present, otherwise `levelAdvTimeline`); record the chosen mapping in `docs/spec/03-storm-stats.md` (reading section) and adjust `GameSummary`.
- [ ] **Step 2: Tests (red)** server: `summary_matches_match_sheet_values` (fixture: each summary field = the value read directly in `match_`/`players`), `summary_rejects_non_custom` (`NotCustom`), `summary_rejects_failed_status`, `replay_route_garbage_bytes_is_422_not_500`, `replay_route_empty_body_is_400`, `replay_route_writes_nothing_to_matches` (`matches`/`uploads` count identical before/after), `replay_route_fills_game_record`; vitest: `PostGameSlide` does not show a row whose data is missing (`structures` missing → no "FORTS · KEEPS" row), damage/healing/siege/takedowns tabs.
- [ ] **Step 3: Implement** the extraction, route, slide (mockup `postgame-breakdown.html`), slideshow (auto rotation `interval_ms`, or manual via `POST /api/cast/slide`, open the breakdown of any game via `slide.game`). Without a summary → empty slide area.
- [ ] **Step 4:** `cargo test` (with `DATABASE_URL`) + clippy + `npm test && npm run build`; check in the browser on a real Custom replay: values match `/match/:id` of an equivalent game if available. **Step 5: Commit** `feat(cast): post-game breakdown from replay + slideshow`

### Task 10: Draft in the control room, redirects

**Files:**
- Create: `web/src/cast/DraftWorkspace.tsx` (body extracted from `pages/Draft.tsx`), `web/src/cast/DraftReplayPanel.tsx` (body extracted from `pages/DraftReplay.tsx`), `web/src/cast/DraftScene.tsx` (overlay view extracted from `pages/DraftOverlay.tsx`)
- Modify: `pages/Draft.tsx`, `pages/DraftReplay.tsx`, `pages/DraftOverlay.tsx` (reduced to thin wrappers over the extracted components), `App.tsx` (`/draft` and `/draft/replay` → `<Navigate to="/cast?scene=draft" replace />`; `/draft/overlay` kept), `Cast.tsx`, `CastOverlay.tsx`, `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes: all the existing code (`useDraft`, `draft*`, `draftReplay*`) **unchanged**; only difference: team names, player names, score, Bo are no longer editable in this workspace (displayed, editing = control room rail).
- Produces: `DraftWorkspace` (central picker, Blue/Red columns, format, map, first pick, Undo/Reset, Next game / New series); `DraftReplayPanel` (drop zone, queue, Next/Back/Reset/Play, fearless); the overlay's `draft` scene = `DraftScene` (same rendering as `/draft/overlay`, skins kept).

- [ ] **Step 1: Tests (red)**: the existing tests (`draftOverlay.test.ts`, `draftReplay.test.ts`, Rust draft tests) **stay green without modification**; new vitest: `DraftWorkspace` with an active series exposes no name/score edit field (rendered read-only); the `/draft` route redirects to `/cast?scene=draft`.
- [ ] **Step 2–4:** extract without changing behaviour (move, do not rewrite) → `cargo test` (draft included) + `npm test && npm run build`; check in the browser: a full Standard draft from the control room, then a Custom replay on auto, Fearless over 2 games; the `/cast/overlay` overlay in the `draft` scene reflects every click; `/draft/overlay` still works. **Step 5: Commit** `refactor(web): draft moved into the /cast control room, /draft* redirects`

### Task 11: End-to-end check and STATUS

**Files:** Modify `docs/STATUS.md`; (no code except fixes found)

- [ ] **Step 1:** Full run on the release server + dev Postgres: create a Bo5 Fearless series (pool 10), enter the veto, lineups A→B after game 1, load a Custom replay (post-game), live draft then replay, In-game scene on the real 1920×1080 capture; refresh control room and overlay at each step (no loss), restart the server (state kept).
- [ ] **Step 2:** Measure and record: scene → overlay < 500 ms; `GET /api/cast/lineup` (10 players) < 100 ms; post-game decoding < 500 ms. Check the spec's 7 acceptance criteria one by one, with the proof (command/screenshot) for each.
- [ ] **Step 3:** `cargo test --workspace` (with `DATABASE_URL`), `cargo clippy --all-targets -- -D warnings`, `cd web && npm test && npm run build` all green.
- [ ] **Step 4:** Update `docs/STATUS.md` (header at the top: shipped, measurements, limits, what remains, "not yet deployed on the box"); re-read that each changed contract has its `docs/spec/` section up to date.
- [ ] **Step 5: Commit** `docs(status): cast control room shipped`
