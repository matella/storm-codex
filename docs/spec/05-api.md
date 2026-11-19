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
| `GET /api/matches` | paginated list; filters `map`, `mode`, `hero`, `player`, `limit`/`offset`. Each match carries `perspective_toon` (nullable): result and MVP filters use the same perspective as the UI. An explicit account (`toon:` for a verified handle, otherwise case-insensitive name) restricts eligible rows; without it, verified handles precede manual names. Ties are ordered by team and player row ID. |
| `GET /api/matches.csv` | CSV export (same filters and perspective selection; the UI's "JSON export" = `/api/matches` with a high `limit`) |
| `GET /api/matches/{id}` | full detail `{id, fingerprint, parser_version, match, players}` (each player carries **`apm`**, `null` before re-process) — `match` = full storm-stats object (timeline, objectives, `messages`…) |
| `GET /api/matches/{id}/raw?stream=…` | dump decoded on the fly (7 heroprotocol streams) + LRU cache |
| `GET /api/matches/{id}/replay2d` | 2D viewer model decoded on the fly, plus `players[] {playerId, name, hero, team, win}`. The LRU disk cache (`RAW_CACHE_DIR`) holds only the replay-derived model and toon mapping; player metadata is read from Postgres on every request, so in-place hero repairs are visible on the next read |
| `GET /api/players/search?q=&limit=` | player search: `{hits:[{toon, name, matched_name, matched_old, games, wins, roles[2], last_seen}]}`. Searches the current name **and all former ones** (substring, case-insensitive, literal `_`/`%`); < 2 characters → `hits: []`; `limit` default 20, max 50; order exact > prefix > substring then games. Static route: takes priority over `{toon}` (axum). Measured on the dev corpus (2,723 games, 7,618 players): p95 26 ms (budget < 100 ms) |
| `GET /api/players/{toon}` | player summary + hero pool; **`avg_apm`** = mean of measured APMs (NULLs ignored, `null` if no game measured), `apm_games` = measured games, `recent[].apm` = the game's APM (rounded); `name` = current name (dated history), `names` = all names seen, **most recent first** |
| `GET /api/players/{toon}/identity` | Identity tab: `{toon, name, names:[{name, first_seen, last_seen, games}], profiles:[{name, first_seen, last_seen, games}], suggestions:[{toon, name, level:"high"\|"low", score, reasons:[{kind, text}]}], confirmed:[{toon, name, decided_at}]}`. `suggestions` = **estimate** of accounts belonging to the same person (pure scoring `players/similarity.rs`, preselection bounded to 30 candidates `players/identity.rs`); `profiles` = non-ignored hotkey profile names (`players/profiles.rs`); a **non-generic** name shared by two accounts (reason `profile`, strong signal) is enough on its own for the HIGH level and makes the account a candidate even if it is otherwise excluded; excludes the operator, upload-token owners (except token/profile signal), accounts already met in a game, decided pairs. 404 if the player is unknown. ~400 ms on the dev corpus |
| `GET /api/players/{toon}/chat` | chat habits of an account, **measurements only**: `{games, messages, per_game, all_share, team_share, timing:{early,mid,late}, top_words:[{word,count}], enough}`. Source `matches.data.messages` (type 0, `player` = toon), the account's **500 most recent games** (`games` and `messages` cover that same window; ~65 ms measured on the largest account on the box, 2,128 games); `all_share + team_share` may be < 1 (recipient other than all/team); `top_words` = at most 8 words present in ≥ 3 **distinct messages** (stop words, < 3 and > 20 characters excluded); **never the text**, no toxicity score; `enough` = ≥ 5 games. 404 if the player is unknown |
| `PUT /api/players/{toon}/links/{other}` 🔒 | body `{status:"confirmed"\|"dismissed"}`: "same person" note or dismissed suggestion (**no profile merge**). Normalised pair (A→B ≡ B→A), idempotent; 400 if `toon == other`, 404 if unknown, 422 if the status is invalid |
| `GET /api/heroes` · `GET /api/hero/{hero}` · `GET /api/hero/{hero}/patches` | hero aggregates; cross-reference with patch notes |
| `GET /api/hero-changes` · `GET /api/hero-changes/heroes` | hero sections of the patch notes (buff/nerf); PTR candidates (`channel = ptr`) excluded |
| `GET /api/synergies` | hero pairs (winrates with/against) |
| `GET /api/patches` · `GET /api/patches/{id}` | `dim_patches` list (`{items:[…]}`, old shape + `channel`, `communityOnly`); detail stored offline in the old API shape (`internalId`, `patchName`, `patchType`, `liveDate`, `officialLink`, `content`, `tableOfContents[]`, `sections[]`, + `aliases`, `channel`, `availability`, `hasContent`, `communityOnly`, `reason`; the old numeric `id`/`heroId` are gone). `{id}` = the dataset id (`live:<build>`, `source:…`) **or an alias** (old HotsPatchNotes `internalId`): an alias answers with the canonical patch. Unknown → `null` |
| `GET /api/maps` | aggregate per map |
| `GET /api/dim/heroes` · `GET /api/dim/talents` | replicated reference data |
| `GET /api/dim/hero-attributes` | 4-letter hero attribute code → canonical parser name (`heroAttribute` from `attr.json`, storm-stats); static, no DB. Resolves draft bans, stored raw (`match.bans[t][i].hero` = `Crus`, `DEAT`…) — `dim_heroes` does not carry the `attributeId` |
| `GET /api/trends` | winrate/duration per build/patch |
| `GET /api/now-playing` | Orpheus proxy (music widget) |
| `GET /api/settings` | application settings (including `operator_names`) |

## Local insights (`insights/`) — spec `docs/specs/2026-11-17-local-insights-design.md`

Read-only, computed from the stored storm-stats JSON; no migration. Pure logic in `insights/logic.rs` (unit tests).
Team indices are 0 = blue, 1 = red; times are game seconds.

| Route | Role |
|---|---|
| `GET /api/matches/{id}/insights?from=&to=` | `{fights, camps, isolated_deaths, window, teamfight_totals}`. **fights**: deaths clustered by time and place (a death joins the fight if ≤ 15 s after its previous death and ≤ 30 map units from its centre; ≥ 2 deaths): `{start, end, lost:[b,r], winner (fewer heroes lost; null = even), deaths:[{time, team, victim, victim_hero, killers}], structures_after / camps_after / objectives_after:[b,r]}` (within 30 s after the fight; a structure counts for the team that destroyed it). **camps**: `{time, team, type, reason}`, `reason` ∈ `traded_for_objective` (enemy objective within ±20 s) / `before_objective` (an objective event within 60 s after) / `after_won_fight` (the team won a fight that ended ≤ 30 s before) / `quiet`. **isolated_deaths**: deaths outside any fight. **window** (only with both `from` and `to`, else `null`; one bound alone = 400 `from_and_to`): `{from, to, xp_from, xp_to, players:[{toon, takedowns, deaths}], teams:[{level_from, level_to, xp, structures, camps, objectives}] ×2}` — events in `(from, to]`; XP from the 60 s samples at or before `from` and at or after `to` (`xp_from`/`xp_to` = the samples used). **No damage per window**: the replay stores damage only as end-of-game totals; `teamfight_totals` gives the whole-game `TeamfightHeroDamage` / `TeamfightDamageTaken` / `TeamfightHealingDone` per player. 404 `not_found` |
| `GET /api/records?mode=&map=&hero=&mine=&limit=` | `{stats:[{key, label, rows:[{value, toon, name, hero, map, mode, played_at, match_id, mine}]}]}`: top `limit` (5, max 20) single games per stat — kills, takedowns, assists, deaths, hero damage, siege damage, healing, self-healing, damage taken, XP, camps, time CC'ing heroes; zero values never count; ties → most recent first. `mine` = only operator rows (each row's `mine` flags them either way) |
| `GET /api/maps/early-leads?mode=&mine=&account=&from=&to=` | same filters as `/api/maps` (400 `date` on an unreadable date). `{games, leads:[…], maps:[{map, games, leads:[…]}]}`, lead = `{key, label, games, wins, my_games, my_wins}` for `first_blood`, `first_to_10`, `first_fort`, `first_to_20`: in how many games a team got there first and how many of those it won; `my_*` = games where the operator's team got there first. Leads nobody reached are left out |
| `GET /api/compare?a=<toon>&b=<toon>` | `{a, b, together:{games, wins}, against:{games, a_wins}, shared:[{match_id, map, mode, played_at, together, a_hero, b_hero, a_win, b_win}] (50 most recent)}`; `a`/`b` = `{toon, name, games, wins, kills, deaths, assists (averages), heroes:[{hero, games, wins}] (top 5)}`. 400 `a_and_b` / `same_player` |
| `GET /api/players/{toon}/insights` | `{games, small_sample (< 20 games), nemesis:[{toon, name, kills, games}] (top 3 accounts by takedown participation on this player's deaths), thrown:{losses, count} (losses after leading by ≥ 2 levels at some point, from `levelAdvTimeline`), comebacks:{wins, count} (wins after trailing by ≥ 2), deaths:{total, isolated}, maps / heroes:[{name, games, wins, pct}] (≥ 10 games, best win rate first)}` |
| `GET /api/sessions?account=&limit=` | `{sessions:[{id, start, end, games, wins}]}`, newest first; `limit` 20 (1–100). A **session** = the operator's dated games (perspective row as in `/api/matches`: a verified toon first, then a name in `operator_names`, ties by team then row id; `account` = a name of `operator_names` or `toon:<handle>` of a verified folder) split where two consecutive game **starts** are more than 3 h apart (exactly 3 h stays one session). `id` = first match id; `end` = last start + last game length. Games without `played_at` are ignored. 400 `account` on an unknown account. Spec `docs/specs/2026-11-19-session-review.md` |
| `GET /api/sessions/{id}?account=` | `{id, start, end, span_s, played_s, games:[{match_id, played_at, hero, map, mode, win, length, kills, deaths, assists, award, deltas:{stat: value}, baseline: "hero"\|"overall"}], record:{wins, losses}, streaks:{best_win, worst_loss, current (> 0 wins, < 0 losses)}, best, worst (match ids, both null with fewer than two games with nonempty `deltas`), heroes / maps:[{name, games, wins, base_games, base_wins, small_sample (< 5 baseline games)}] (most played first), baseline:{from, to, modes, games, wins, averages:{stat: value}, small_sample (< 20 games)}, prev, next}` (`prev` older / `next` newer session id or null). **Baseline** = the operator's games in the 90 days before the session start **in the game modes played in the session** (`modes`). Stats: kills, deaths, assists (takedowns − kills), hero_damage, siege_damage, healing, experience; `deltas` = value − baseline average of the same hero when it has ≥ 5 baseline games, else of the whole baseline (1 decimal; absent without a baseline value). **Best / worst** = most / fewest rounded deltas better than that average (positive; negative for deaths), ranking only games with deltas, ties by KDA ratio ((K + A) / max(D, 1)). 404 `not_found` unless `id` starts a session for that perspective; 400 `account` |

**Corpus** (`insights/cache.rs`): records, early leads, personal insights and the session review read an in-memory extract of every game (game length, per-player stats and first award,
compact deaths with interned toons, early leads, level lead), not the JSON per request (one pass over `match_players.data` detoasts
~8 kB a row: 12 s for records on the box's 3,480 games). Each request first reads a version of `matches` (count, max id, sum of `parser_version`; no JSON): unchanged → the same corpus; otherwise the corpus is diffed against `matches` by `(id, parser_version)`: deleted or re-processed games are dropped, missing ones loaded (out-of-order ids included); games are shared (`Arc`) between refreshes. Loaded 20 games per round trip and warmed up in the background at startup
(`cache::warm_up`). Measured on the box's data (copy, release build): load 3.3 s, server RSS 55 MB after it (11 MB without);
then records 35 ms, early leads 6 ms, player insights 75–100 ms, compare 12 ms, match insights 4 ms. Compare's player cards also come from the corpus (shared games stay one SQL join, no JSON).

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
`Team.logo = "/api/cast/logo/{side}?v={updated_at_ms}"` · `GET /api/cast/logo/{side}` → bytes (`404` if none; `X-Content-Type-Options: nosniff`,
`Content-Security-Policy: sandbox; default-src 'none'`). The stored row is re-checked like an upload (accepted type + magic
bytes) and answered `404` otherwise: a row restored from an imported backup is never served as another type (e.g. HTML on
the app's origin, which the desktop shell grants IPC).
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

`GET /api/standalone/status`, `GET /api/standalone/identity`, `PUT /api/standalone/folders`, `POST /api/standalone/rescan`,
`POST /api/standalone/shutdown`: 404 when the server is not in standalone mode.
Rescan returns 202 only when queued; rescan and folder save return 503 if the import worker
cannot accept the command. A folder list already persisted before that failure is used on restart.
Overlapping folder saves queue their updates in persistence order.
In standalone mode **every** route of this page (and the pages, images and `/ws`) accepts only
a loopback `Host` (other hosts 403) and refuses any request, `GET` and the `/ws` upgrade
included, whose `Origin` is present and is not the app's own (`http://` + loopback name + bound
port) (403); see [04-serveur.md](04-serveur.md) "Standalone import". Bodies and answers: the route table
in [04-serveur.md](04-serveur.md) ("Standalone import").

### Standalone reference status and controls

| Request | Response |
|---|---|
| `GET /api/standalone/reference` | `200` with `{enabled, phase, version, revision, artwork}` |
| `PUT /api/standalone/reference` with `{enabled: bool}` | Persists the preference, queues a check when enabled, and returns `200` with the same shape. |
| `POST /api/standalone/reference/refresh` | Requests one check: `202` when enabled, `409` when disabled. |

These routes are standalone-only (`404` otherwise) and use the same Host/Origin checks. There is
no request-supplied feed URL. `phase` is `waiting`, `syncing`, `ready` or `error` and is independent
of import progress. `enabled` is independent of cached dataset usability. `version` is the active
dataset version (a string), or `null`. `artwork` is `true` only when the active dataset's managed
image generation is complete on this PC; it is checked locally (never over the network) at startup,
so it is `false` after restoring a database without its asset folder, for a legacy unmanaged image
folder, or with no active dataset, even while downloads are disabled. `revision` is an opaque **string**: compare for equality, not
ordering. It changes when an activation becomes visible, including an update with no new patch ID
and same-version asset recovery. It does not change on a refused check or a preference toggle.
Every preference change cancels the check in progress: a download started before sync was turned off
never activates, even if sync is turned back on before it finishes; the check queued by re-enabling
runs instead.
Clients poll this local status (not GitHub) and refetch reference-dependent data when it changes.

In standalone mode, operator perspective in Matches/CSV, Heroes, Maps (including early leads),
Trends, Synergies, Records and resolved lobby stats includes exact folder ToonHandles in addition to
manual names. Existing plain `account` values still match names; selectors may send
`account=toon:<handle>` for verified folders to keep the same filter through a rename. The manual
`operator_names` setting and its write payload are unchanged. Independent OBS sources still read
manual names or their existing explicit override.

## Static

`/` = SPA (`index.html` fallback as `no-cache` — a redeploy changes the bundle hash);
`/images` = hero portraits + vendored map backgrounds; `/assets` = fingerprinted Vite bundle,
served as `Cache-Control: public, max-age=31536000, immutable` (missing asset → 404). Responses
compressed gzip/brotli depending on `Accept-Encoding`.

## WebSocket `/ws`

Broadcast to all clients; see [07-evenements.md](07-evenements.md) for the
types. No useful incoming messages (ping/close only). A lagging client is
"lagged" without disconnection (non-critical events, re-fetch by TanStack Query).
