# API — REST + WebSocket reference

Routes defined in `crates/storm-codex-server/src/main.rs` (source of truth). Auth: only
routes marked 🔒 require `Authorization: Bearer` (personal upload token for uploads;
`ADMIN_TOKEN` for admin — **if set**, otherwise open mode).

## Upload

| Route | Role |
|---|---|
| `POST /api/upload` 🔒 | body = raw bytes of the `.StormReplay`, header `X-Filename` (percent-encoded accepted). Response within ≤ 2 s: `parsed` / `409 duplicate` / `parse_failed`; `202 accepted` if the pool is saturated (result over WS) |
| `POST /api/upload-raw` 🔒 | **strict alias** of the previous one — client-rs compatibility (Hots-Overlay) |

## Reads (`read.rs`)

| Route | Role |
|---|---|
| `GET /api/health` | `{status, parser_version, db}` — 200 or 503 |
| `GET /api/matches` | paginated list; filters `map`, `mode`, `hero`, `player`, `limit`/`offset` |
| `GET /api/matches.csv` | CSV export (same filters; the UI's "JSON export" = `/api/matches` with a high `limit`) |
| `GET /api/matches/{id}` | full detail `{id, fingerprint, parser_version, match, players}` (each player carries **`apm`**, `null` before re-process) — `match` = full storm-stats object (timeline, objectives, `messages`…) |
| `GET /api/matches/{id}/raw?stream=…` | dump decoded on the fly (7 heroprotocol streams) + LRU cache |
| `GET /api/players/search?q=&limit=` | player search: `{hits:[{toon, name, matched_name, matched_old, games, wins, roles[2], last_seen}]}`. Searches the current name **and all former ones** (substring, case-insensitive, literal `_`/`%`); < 2 characters → `hits: []`; `limit` default 20, max 50; order exact > prefix > substring then games. Static route: takes priority over `{toon}` (axum). Measured on the dev corpus (2,723 games, 7,618 players): p95 26 ms (budget < 100 ms) |
| `GET /api/players/{toon}` | player summary + hero pool; **`avg_apm`** = mean of measured APMs (NULLs ignored, `null` if no game measured), `apm_games` = measured games, `recent[].apm` = the game's APM (rounded); `name` = current name (dated history), `names` = all names seen, **most recent first** |
| `GET /api/players/{toon}/identity` | Identity tab: `{toon, name, names:[{name, first_seen, last_seen, games}], profiles:[{name, first_seen, last_seen, games}], suggestions:[{toon, name, level:"high"\|"low", score, reasons:[{kind, text}]}], confirmed:[{toon, name, decided_at}]}`. `suggestions` = **estimate** of accounts belonging to the same person (pure scoring `players/similarity.rs`, preselection bounded to 30 candidates `players/identity.rs`); `profiles` = non-ignored hotkey profile names (`players/profiles.rs`); a **non-generic** name shared by two accounts (reason `profile`, strong signal) is enough on its own for the HIGH level and makes the account a candidate even if it is otherwise excluded; excludes the operator, upload-token owners (except token/profile signal), accounts already met in a game, decided pairs. 404 if the player is unknown. ~400 ms on the dev corpus |
| `GET /api/players/{toon}/chat` | chat habits of an account, **measurements only**: `{games, messages, per_game, all_share, team_share, timing:{early,mid,late}, top_words:[{word,count}], enough}`. Source `matches.data.messages` (type 0, `player` = toon), the account's **500 most recent games** (`games` and `messages` cover that same window; ~65 ms measured on the largest account on the box, 2,128 games); `all_share + team_share` may be < 1 (recipient other than all/team); `top_words` = at most 8 words present in ≥ 3 **distinct messages** (stop words, < 3 and > 20 characters excluded); **never the text**, no toxicity score; `enough` = ≥ 5 games. 404 if the player is unknown |
| `PUT /api/players/{toon}/links/{other}` 🔒 | body `{status:"confirmed"\|"dismissed"}`: "same person" note or dismissed suggestion (**no profile merge**). Normalised pair (A→B ≡ B→A), idempotent; 400 if `toon == other`, 404 if unknown, 422 if the status is invalid |
| `GET /api/heroes` · `GET /api/hero/{hero}` · `GET /api/hero/{hero}/patches` | hero aggregates; cross-reference with patch notes |
| `GET /api/hero-changes` · `GET /api/hero-changes/heroes` | hero sections of the patch notes (buff/nerf) |
| `GET /api/synergies` | hero pairs (winrates with/against) |
| `GET /api/patches` · `GET /api/patches/{id}` | `dim_patches` list; detail (content) |
| `GET /api/maps` | aggregate per map |
| `GET /api/dim/heroes` · `GET /api/dim/talents` | replicated reference data |
| `GET /api/dim/hero-attributes` | 4-letter hero attribute code → canonical parser name (`heroAttribute` from `attr.json`, storm-stats); static, no DB. Resolves draft bans, stored raw (`match.bans[t][i].hero` = `Crus`, `DEAT`…) — `dim_heroes` does not carry the `attributeId` |
| `GET /api/trends` | winrate/duration per build/patch |
| `GET /api/now-playing` | Orpheus proxy (music widget) |
| `GET /api/settings` | application settings (including `operator_names`) |

## Management (`manage.rs`, `admin.rs`) — 🔒 if `ADMIN_TOKEN` is set

| Route | Role |
|---|---|
| `PUT /api/admin/settings` | writes `app_settings` (e.g. `operator_names`) |
| `GET/POST /api/teams` · `PUT/DELETE /api/teams/{id}` | teams (+ `league` field) |
| `GET/POST /api/collections` · `DELETE /api/collections/{id}` | match collections |
| `POST /api/admin/tokens` · `DELETE /api/admin/tokens/{id}` | personal upload tokens (the plaintext is only shown at creation); idempotent revocation (date of the 1st revocation kept), `404` if the id does not exist |
| `GET /api/admin/uploads` | upload health (statuses, failure classes) |
| `POST /api/admin/reprocess` | idempotent re-parse (driven by `parser_version`); also covers `scouting_games`, then recomputes the affected reports |

Every write in this section answers `500 {"error":"db"}` if the database fails (before: `200`
even on failure). Deleting a missing team or collection stays `200` (idempotent).
The admin Bearer is compared by SHA-256 digest, with no early exit.

## Scouting (`scouting/api.rs`) — writes 🔒 if `ADMIN_TOKEN` is set

Spec: `docs/specs/2026-09-26-rapports-scouting-design.md`. Dedicated tables: these routes never
write into `uploads`/`matches`/`match_players`. Every mutation re-triggers the facts computation
(`facts_version + 1`).

| Route | Role |
|---|---|
| `GET /api/scouting` | list (most recent first): title, team, dates, number of games, record, status `empty`/`ready`/`analyzed`/`stale` |
| `POST /api/scouting` 🔒 | create `{title, target_name?}` → `201 {id}` |
| `GET /api/scouting/{id}` | full report: `snapshot` = `{facts, detection{candidates, ambiguous}, roster, roster_auto}`, `games` (teams, target side + `target_source` roster/anchor/manual), `analysis` with evidence resolved against the current facts + `analysis_tally`, `status` |
| `PATCH /api/scouting/{id}` 🔒 | `title`, `target_name`, `roster`, `anchors` (the last two → recompute); returns the report |
| `DELETE /api/scouting/{id}` 🔒 | deletes (games cascaded, archived raw files kept) |
| `POST /api/scouting/{id}/replays` 🔒 | raw bytes + `X-Filename` → `added` (+ `gid`, detected side) / `duplicate` / `parse_failed` (+ `error_class`) |
| `PATCH /api/scouting/{id}/replays/{gid}` 🔒 | `{target_team: 0\|1\|null}` — manual side, `null` = back to automatic |
| `DELETE /api/scouting/{id}/replays/{gid}` 🔒 | removes a game |
| `GET /api/scouting/{id}/pack.md` | Markdown LLM pack (prompt, players `pN`, facts with ids, games, answer format) |
| `GET /api/scouting/{id}/pack.xlsx` | same facts as a workbook (README, Facts, Players, Heroes, Maps, Draft, Games) |
| `PUT /api/scouting/{id}/analysis` 🔒 | body = raw text of the LLM answer; extracts/validates/overwrites → `{warnings, tally}`; `422` if the JSON is missing/invalid, `format_version` ≠ 2 (map-by-map draft plan; v1 is refused) or `report_id` of another report (nothing is overwritten) |
| `DELETE /api/scouting/{id}/analysis` 🔒 | removes the analysis |

## Control room, draft and lobby writes — 🔒 if `ADMIN_TOKEN` is set

Any **non GET/HEAD/OPTIONS** request on `/api/cast`, `/api/cast/*`, `/api/draft`,
`/api/draft/*`, `/api/lobby` or `/api/lobby/*` (except `POST /api/lobby`, ingestion authenticated
by upload token) requires `Authorization: Bearer <ADMIN_TOKEN>` when `ADMIN_TOKEN` is set, otherwise
`401 {"error":"admin token requis"}` (middleware `auth::guard_writes`, before the handler; the error
value is a stable string kept as is). Reads stay open (OBS overlays send no token). In open mode (no
`ADMIN_TOKEN`), nothing changes. The front-end sends the token entered in Admin (`authHeaders()`).

## Draft simulator (`draft/api.rs`)

`GET /api/draft` (state) · `POST /api/draft/config` · `/action` (pick/ban) · `/undo` · `/reset` ·
`/unavailable` (fearless) · `/score` · `/teams` (names) · `/series/next` · `/series/new`.
Each mutation → WS `draft.updated`; the overlay and the console re-fetch.
**Active cast series** (`cast.active`): `/teams` and `/score` answer `409 {"error":"series_active"}` (the series
is the source of truth); `/config` is still accepted but ignores `blue`/`red`/`bo` and forces `format = series.format`
(map and first_pick stay free). Inactive series: behaviour unchanged.
`GET /api/draft` is the **public view**: in replay mode, without the script or player names of unrevealed picks.

**Draft from a replay** (`draft/replay_api.rs`, open mode) — Custom only, nothing written to the database:
`POST /api/draft/replay/load` (body = `.StormReplay` bytes, optional `X-Filename`; decoded under `parse_sem`, temporary file write, decoding and deletion **inside** the `spawn_blocking` with a drop guard — file deleted even on failure, panic or disconnected client, like the cast replay; `201` public queue; `500 {"error":"io"}` if the temporary file cannot be written;
`400` empty body; `422` `{error: not_custom|no_draft|parse_failed}`; `500` `{error: pool|io}`: parsing semaphore closed / temporary file not written) · `GET /api/draft/replay` (queue
**with scripts**, for the remote-control page) · `POST …/replay/next` · `/back` · `/reset` (one **phase** at a time;
`409` with no game / at the start / at the end) · `/select {index}` · `/fearless {value}` (active cast series: `/select`,
`/fearless` and the automatic selection of `/load` follow the **series format**, not `replay.fearless` — M5) ·
`/play {playing, interval_ms?}` (default 30,000 ms per phase; `400` outside 10 ms–1 h; the timer is server-side,
stops at the end of the draft) · `POST …/replay/queue/move {from,to}` · `DELETE …/replay/queue/{i}`.
Spec: `docs/specs/2026-10-25-draft-replay-design.md`.

## Cast control room (`cast/api.rs`) — writes 🔒 if `ADMIN_TOKEN` is set (see above)

`GET /api/cast` (→ `Series`) · `POST /api/cast/series {blue:Team, red:Team, bo, format, pool:[name]}`
(`blue`/`red` = full team — name, record, players, captain, logo — all fields optional, `{name}` alone accepted; a missing `logo` keeps the current logo; creates/restarts in a single call: new `series_id` identity (uuid), `active=true`, games/score/slideshow reset, scene kept; **draft aligned (R20)**: the draft engine takes the series' `format` and `steps`, fresh game (cursor 0) and new series (history and series bans cleared), keeping map, first_pick, manual pre-bans, replay queue (kept, but current game deselected and auto-play stopped, like `/api/draft/config`) and synced teams (`DraftState::align_to_series`); `bo` ∈ {1,3,5,7}
otherwise `400 {"error":"bad_bo"}`, state unchanged; **input bounds**: pool ≤ 32 maps of ≤ 64 characters (`400 bad_pool`), team name/record/player name/toon ≤ 64 characters, free line ≤ 120, logo URL ≤ 512 (`400 bad_team`, on `POST /api/cast/series` and `PUT /api/cast/teams`); **JSON body of `/api/cast/*` routes limited to 64 KiB** (`413` beyond; the logo and the replay have their own limits); pool deduplication is linear) ·
`PUT /api/cast/teams {blue, red}` · `POST /api/cast/pool/map {name, status: available|banned|picked|decider, by?}` ·
`POST /api/cast/games/start {map}` → `{number}` · `POST /api/cast/games/{n}/winner {side, takedowns?: [blue, red], duration_secs?}` (**R21**: a hand-entered winner is **corrected** by a new call — score, lineup mode and draft sync follow; refused with `WinnerAlreadySet` only if the game has a replay summary, which carries its own winner/orientation) · `DELETE /api/cast/games/{n}` (**R21**, game started by mistake: only the **last** one — otherwise `409 NotLastGame` — and with no winner or summary — otherwise `409 GameHasResult`; the map keeps its picked/decider status and becomes startable again, a game pinned in the slideshow is unpinned; draft sync) ·
`POST /api/cast/score {blue, red}` or `null` (back to the computed score) · `POST /api/cast/scene {scene: lineups|series|draft|ingame|hide}` ·
`POST /api/cast/lineup-mode {mode: auto|a|b}` · `POST /api/cast/slide {game?, index?, auto?, interval_ms?}`
(partial; `game: 0` unpins the displayed game → `slide.game` = null, "latest game"; numbers start at 1; **R25**: any other `game` must be the number of an existing game, otherwise `409 UnknownGame`; `index` ≤ 99 otherwise `400 {"error":"bad_index"}` (checked **after** the `409 SeriesInactive` guard: an inactive series answers 409 even with an invalid `index`; the overlay takes it modulo the number of slides); state unchanged on refusal; `interval_ms` bounded to 500 ms–1 h, the control room enters 1–3600 s). Mutations answer with the full `Series` (`games/start`: `{number}`). **Rotation anchor** (2026-10-31): `slide` carries `since_ms` (ms, server clock), set on **every** successful `POST /api/cast/slide`; auto-rotation is computed client-side as `floor((now − since_ms) / interval_ms)` steps from the index, so that the OBS overlay and the control room preview show the same slide (clock skew between machines ≈ a few ms on a LAN).
`CastError` errors → `409 {"error": "<code>"}` — stable codes (`CastError::code()`, independent of the Rust name):
`UnknownMap`, `NotInPool`, `DuplicateDecider`, `MapNotChosen`, `UnknownGame`, `WinnerAlreadySet`, `NotLastGame`,
`GameHasResult`, `MapAlreadyPlayed`, `SeriesInactive`, `side_mismatch`, `series_changed` — state unchanged.
**R24**: if writing the series to the database fails, the mutation answers `500 {"error":"persist_failed"}`; the state
stays applied in memory and `cast.updated` is broadcast (the operator sees the error in the control room banner). Edge case: if the v2 write fails and then a concurrent v1 save succeeds, the database keeps v1 while memory keeps v2; the v2 caller received `500 persist_failed` (consistent with R24; a restart would reload v1).
Writes are versioned: an older snapshot never overwrites a newer one (see 04-serveur).
Every mutation except `/series` and `/scene` returns `409 SeriesInactive` while `active=false`.
`POST /api/cast/logo/{side}` (`blue`|`red`; body = bytes, `Content-Type` png/jpeg/webp; 1 byte–512 KB;
`415 unsupported_mime` type not accepted, `400` empty body, `413` too large, `415 mime_mismatch` if the first bytes are
not the signature of the declared type — PNG `89 50 4E 47`, JPEG `FF D8 FF`, WebP `RIFF....WEBP` — nothing is stored;
`404` unknown side; allowed on an inactive series) → `Series` with
`Team.logo = "/api/cast/logo/{side}?v={updated_at_ms}"` · `GET /api/cast/logo/{side}` → bytes (`404` if none; `X-Content-Type-Options: nosniff`).
**Series → draft sync** (one way) after `/series`, `/teams`, `/games/{n}/winner`, `DELETE /games/{n}`, `/score`: names, player names
(`Player.name`), score (`Series::score()`) and `bo` copied into `DraftState` without touching cursor/assignments/replay
queue/fearless; both states are persisted, WS `draft.updated` + `cast.updated`.
**Recent heroes** (`cast/lineup.rs`, read-only, no mutation or WS): `GET /api/cast/lineup?blue=toon,toon,…&red=…`
→ `{ blue: [[HeroCount]], red: [[HeroCount]] }`, `HeroCount = {hero, games}`; one list per player **in the order received**
(empty toon = unlinked player → `[]`). Per toon: window of the **last 20 games, all modes** (`match_players` ⋈ `matches`,
order `played_at DESC NULLS LAST, id DESC`), the **3 heroes** played most (games, then recency); **`[]` if < 5 games
in the window** (hidden, never partial); a game with a NULL hero counts towards that threshold of 5 (game played) but not in the hero count. A single SQL query (`unnest … LATERAL`, index `match_players_toon_idx`,
checked by `EXPLAIN` in the test: the plan must name that index, sequential scan disabled on the connection; test rows cleaned up even on failure); budget **< 100 ms for 10 players** (test `lineup_route_under_100ms_for_10_players`).
`> 10` toons per team → `400 {"error":"too_many_players"}`; a toon longer than 64 bytes → `400 {"error":"bad_toon"}`; SQL error → `500 {"error":"db"}`.
**Replay of a game** (`cast/replay.rs`): `POST /api/cast/games/{n}/replay` — body = raw bytes of a
`.StormReplay` (like `draft/replay/load`), decoded on the fly under `parse_sem` (temporary file write, decoding and deletion **inside** the
`spawn_blocking`; deletion guaranteed by a drop guard — success, failure, panic, or client disconnected
during decoding; a decoder panic becomes a 422). **Custom only, nothing is written to the database** (neither `matches` nor `uploads`:
only the `cast_live` row). → `201 GameSummary` (see [06-modele-donnees.md](06-modele-donnees.md)); fills
`winner`, `takedowns`, `duration_secs` and `summary` of game `n`, **oriented to the series** (R16, under the
write lock): the replay's players matched to the series' players (linked toon, then exact name) decide whether team 0
of the replay is Blue or Red (`GameSummary::swapped()`); with that evidence the replay is authoritative (replaces an
entered winner, or a previous summary); **without evidence**, team 0 = Blue, but an already-entered winner that
contradicts the replay → `409 {"error":"side_mismatch"}`, nothing changes. **`?swap=1`**: the operator forces the swap.
Evidence = simple majority of the matches (tie = no evidence); a player listed in both teams of the
series is ignored. Persistence + draft sync (score) + WS `cast.updated` like the other mutations; the response is
the oriented summary **plus** `orientation` ∈ `evidence` (evidence from the players) | `assumed` (no evidence: team 0
= Blue) | `forced` (`?swap=1`), and `forced_against_evidence: true` when the forced swap contradicts direct
orientation evidence (accepted, the control room displays it). Errors: `400 {"error":"empty_body"}`; `409 SeriesInactive` / `409 UnknownGame` (game `n` missing) /
`409 side_mismatch` / `409 series_changed` (the `series_id` identity **or the map of game `n`** read before decoding no longer match: a new series was started, or game `n` was removed (R21) and then restarted on another map during decoding — nothing changes);
`422 {"error", "message"}` with `error` ∈ `not_custom` (non-Custom replay), `parse_failed` (unreadable,
truncated bytes, storm-stats status ≠ 1), `incomplete` (winner, duration or takedowns missing from the output) — never 500
on invalid input; `500 {"error":"pool"}` (parsing semaphore closed) or `500 {"error":"io"}` (temporary file not written) are server failures. Decoding budget < 500 ms (measured: full route 178–203 ms in release, local upload
included, conditions undocumented — a later run under load gave 417–444 ms, see `docs/STATUS.md`; `storm-stats-dump --bench` on 6 Custom replays: median 177 ms, max 319 ms).

**Batch import of a series' replays** (`cast/replay.rs`, spec `docs/specs/2026-10-31-cast-batch-replay-import-design.md`):
`POST /api/cast/replays/inspect` — body = raw bytes of a `.StormReplay`; decodes **without writing anything** → `200 GameSummary`
(with `map` = storm-stats map and `played_at` = ISO UTC date, `null` if missing). `400 empty_body`; `422` like the game route.
`POST /api/cast/replays/import?map=<map>&series_id=<id>[&swap=1]` — body = raw bytes; `series_id` = identity of the series read at inspection (required: without it, `400` plain text from the extractor). Creates the game for map `map` (pool `picked`/`decider`,
not played, next number) and fills it from the replay, in **a single mutation** (replay refused → no game created).
`201` = the game (`GameRecord`) + `orientation`, `forced_against_evidence`, `summary`. `409 map_mismatch` (replay map ≠ `map`);
`409 series_changed` (the series was replaced since inspection: nothing is created); `409` `MapNotChosen` / `UnknownMap` / `SeriesInactive` (`CastError` errors); `400 empty_body`; `422` like the game route.
**End-of-game rank (OCR, Aegis)** (`rank.rs`, spec `docs/specs/2026-10-31-ocr-rang-fin-de-partie-design.md`):
`POST /api/rank/result` 🔒 (upload token). Header `X-Session` = hex SHA-256 of the battlelobby (64 characters, lowercase accepted). JSON body: `{status: valide|indisponible, rank_name?, total_points?, promotion_required?, points_delta?, reason?}` (the `status` values are French literals of the contract: `valide` = valid, `indisponible` = unavailable). `valide`: non-empty `rank_name` and `total_points` ∈ [0, 1000]; if `promotion_required` is provided, `total_points + promotion_required = 1000`. `indisponible`: non-empty `reason`, no numbers, no `rank_name`. `|points_delta| ≤ 1000`. A single reading per session (UNIQUE constraint, atomic write): the latest **valid** one wins, and an `indisponible` never overwrites a `valide`. Responses: `201 {id}` (recorded); `200 {id, kept: true}` (`indisponible` ignored, a `valide` exists); `401` without a valid token (checked before any processing of the body); `413` if the body exceeds 64 KB; `422 {error}`: `missing_session`, `bad_session`, `bad_json`, `bad_text`, `rank_required`, `total_required`, `bad_points`, `scale_mismatch`, `reason_required`, `numbers_on_unavailable`, `rank_on_unavailable`; `500 {error: db}`. Table `ranked_results` (migration 0014). Attachment to the match: `match_id` if the current lobby is linked to an existing game (`content_hash` = session); otherwise the reading stays pending and is attached at the game's upload (`lier_match`, `upload.rs`). Each game projection (`project_match`) then attaches by fingerprint (`match_fingerprint`), which survives a re-process. Known gaps: (1) a reading whose lobby was never linked (lobby replaced, more than 6 h, different BattleTags) stays without a match; attachment by replay fingerprint, planned by the spec, requires Aegis to provide the fingerprint (not done yet); (2) attachment by fingerprint is best-effort, outside the projection transaction. Not exposed for reading yet (`GET`).

Import is done file by file, in date order (the client sorts): one failure does not cancel the others (partial import).
Each mutation → WS `cast.updated`. Design: `docs/specs/2026-10-25-cast-control-room-design.md`.

## Live feed of a game in progress (`live/api.rs`)

Spec: `docs/specs/2026-10-24-feed-live-en-partie-design.md`. State **in memory**, one game at a
time, no table.

| Route | Role |
|---|---|
| `POST /api/live/chunk` 🔑 | bytes appended to a stream, upload `Bearer`. Headers: `X-Session` (≤ 128 chars; **hex SHA-256 of the game's battlelobby** — the only way for the server to match the session to the current lobby and derive the map and operator from it; a new id **replaces** the session), `X-Stream` (`tracker` \| `game`), `X-Offset` (position of the chunk in the stream), `X-Build` (optional; missing = latest embedded protocol). → `200 {status: "applied", consumed, last_loop, events}`; `200 {status: "duplicate"}` (already received); `200 {status: "unreadable"}` (unreadable session); `409 {status: "gap", expected_offset}` (resend from there); `413 {status: "too_large"}` (stream > 64 MB); `400` invalid header; `401` |
| `POST /api/live/end` 🔑 | `{session}` → status `ended`; `404` if it is not the current session |
| `GET /api/live?scope=self\|allies\|all&me=<playerId>` | filtered view (default `scope=self`); `204` if no game. Fields: `status` (`live`/`ended`/`unreadable` + `reason`), `session`, `map` and operator taken from the lobby **only if its hash = session id**, `me` + `me_source` (`explicit`/`lobby`/`single_human`/`null` — undetermined = no ability in `self`), `gates_loop`, `tracker_loop`, `game_loop`, `last_chunk_at`, `protocol {build, fallback, assumed_latest}`, `team_levels`, `players[]` (`player_id`, `team`, `hero`, `hero_unit`, `level`, `talents[{tree_id, loop}]`, `deaths`, `takedowns`, `abilities` = `null` if outside the level, otherwise `[{link, cmd, count, recent[loops of the last 3 minutes]}]`), `feed[]` (200 most recent: `death`/`talent`/`level`/`camp`/`objective`) |
| `DELETE /api/live` 🔒 | closes the session (`ADMIN_TOKEN` guard, open mode if it is not configured) |

🔑 = upload token (like `/api/upload-raw`). Each chunk that adds events, and each
status change → WS `live.updated` (signal only).

## Standalone desktop app (`standalone/api.rs`) — only when `STANDALONE=1`

`GET /api/standalone/status`, `PUT /api/standalone/folders`, `POST /api/standalone/rescan`,
`POST /api/standalone/shutdown`: 404 when the server is not in standalone mode.
In standalone mode **every** route of this page (and the pages, images and `/ws`) accepts only
a loopback `Host` (other hosts 403) and refuses non-`GET`/`HEAD` requests whose `Origin` is not
loopback (403); see [04-serveur.md](04-serveur.md) "Standalone import". Bodies and answers: the route table
in [04-serveur.md](04-serveur.md) ("Standalone import").

## Static

`/` = SPA (`index.html` fallback as `no-cache` — a redeploy changes the bundle hash);
`/images` = hero portraits + vendored map backgrounds; `/assets` = fingerprinted Vite bundle,
served as `Cache-Control: public, max-age=31536000, immutable` (missing asset → 404). Responses
compressed gzip/brotli depending on `Accept-Encoding`.

## WebSocket `/ws`

Broadcast to all clients; see [07-evenements.md](07-evenements.md) for the
types. No useful incoming messages (ping/close only). A lagging client is
"lagged" without disconnection (non-critical events, re-fetch by TanStack Query).
