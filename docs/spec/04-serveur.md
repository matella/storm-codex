# storm-codex-server — pipeline & configuration

Crate: `crates/storm-codex-server` (axum 0.8 + sqlx 0.9 + tokio). A single binary: API + front-end
+ overlays. `PARSER_VERSION` (in `main.rs`) drives re-processing — **bump it on every
projection change**.

## Upload pipeline (`upload.rs`)

```
POST /api/upload(-raw)  [personal Bearer token]
  → auth (SHA-256 of the token vs upload_tokens.token_hash)
  → archive the raw file (ARCHIVE_DIR)              ← BEFORE any parse attempt
  → file dedup (uploads.fingerprint = SHA-256 of the content) → 409 duplicate
  → parse in a pool (Semaphore = number of cores, spawn_blocking off the HTTP thread)
  → Postgres projection (project.rs) + uploads.match_id link
  → WS `match.parsed` + (opt-in) Jarvis Redis PUBLISH + (dormant) Azure push
```

- **Response semantics**: `parsed` / `409 duplicate` / `parse_failed` if the result arrives
  within ≤ 2 s; otherwise `202 accepted` (pool saturated — backfill), the result arrives over WS.
- **Classified failures** (`uploads.error_class` + `reject_class()`): storm-replay classes (io,
  decoding…) and storm-stats statuses (unsupported_map, stats_failure/-2, unverified_build/-7…) —
  exhaustive, visible in `GET /api/admin/uploads`.
- **Body limited to 64 MB** (`DefaultBodyLimit`): the axum default (2 MB) silently rejected long
  replays (> 2 MB) with a 413 and made the uploader loop.
- `X-Filename` is **percent-decoded** (apostrophes in map names: Blackheart's Bay).
- **Archive written atomically** (temporary file then rename): a shutdown mid-write never
  leaves a truncated replay as the source of truth.
- **Resume at startup** (`upload::resume_pending`): a `202 accepted` queues the parse in
  memory; uploads left `pending` (restart before the parse finished) are re-queued at startup
  from their archive. The client never resends an accepted file; without this resume they stayed
  stuck.
- **Status writes logged**: a failure to write `uploads.status` (parsed or
  parse_failed) is logged at `error`, never swallowed.

## Process lifecycle (`main.rs`)

- **Graceful shutdown**: Ctrl-C or SIGTERM (`docker stop`) → the server stops accepting connections
  and lets in-flight requests finish (`with_graceful_shutdown`). Measured in the container:
  shutdown in ~0.5 s, exit code 0 (before: SIGTERM ignored, SIGKILL after 10 s). Unfinished
  parses are resumed at the next startup (above).
- **Compression**: `CompressionLayer` (gzip / brotli depending on `Accept-Encoding`) on all
  responses; the default predicate skips small responses, images and streams.
- **SPA HTTP cache**: `/assets/*` (names fingerprinted by Vite) as
  `Cache-Control: public, max-age=31536000, immutable` — a missing asset answers 404, never
  `index.html`; `index.html` and the SPA routes as `no-cache`.
- **Cast/draft write guard**: middleware `auth::guard_writes` (see 05-api.md).

## Played-hero correction (`project.rs`)

In ARAM (3 heroes offered + reroll), attribute `4002` read by the parser — faithful to hots-parser
7.55.7, which does the same — often captures an **offered** hero, not the one played (e.g. Braxis
Outpost, build 98348: all 10 parser heroes are wrong, Kel'Thuzad comes out as "Kharazim").
storm-stats remains a faithful port; the correction lives in the projection:

- played hero = **majority vote of the player's talents** via `dim_talents.tree_id → hero_id`
  (`talent_hero`); no resolvable talent or empty reference data → parser hero kept;
- resolved **before** `matches.data` is written, and applied everywhere storm-stats copied the
  hero at event time: `players[].hero` (original recorded in `heroCorrectedFrom`),
  `victim.hero` / `killers[].hero` of `players[].deaths`, `players[].takedowns` (of **all**
  players) and `match.takedowns`. Replacement **by handle** (`player`), never by hero name
  (two players may have the same parser hero); the `"0"` "Nexus Forces" killer is left intact;
- does not yet cover (same cause, still on `4002` names): `match.heroes`,
  `match.teams[].heroes`, `players[].with.heroes`, `players[].against.heroes` and the picks.

Death/takedown counts do not change (only the names are rewritten). A match projected
before this fix is refreshed by `POST /api/admin/reprocess` (`PARSER_VERSION` 2), which also
re-parses the `scouting_games` — side effect: every affected report is recomputed
(`facts_version + 1`), so an imported analysis turns `stale` even though its facts do not depend
on this projection.

## Scouting (`scouting/`)

A path **separate** from the upload pipeline: `POST /api/scouting/{id}/replays` archives into
`ARCHIVE_DIR/scouting/<sha256>.StormReplay`, parses on the same pool (`parse_sem`), summarises
(`summary.rs`) and inserts into `scouting_games` — never into `uploads`/`matches`. Pure core
unit-tested (`summary`, `side`, `facts`, `pack`, `xlsx`, `analysis`); `api.rs` recomputes the
facts under the report's row lock (`SELECT … FOR UPDATE`) on every mutation. Hero roles:
`dim_heroes.data->>'expandedRole'` (the parser's `gameStats.Role` is 0 in custom games).
`POST /api/admin/reprocess` also re-parses stale `scouting_games` and then recomputes.

## Fingerprints (two-level dedup)

- `uploads.fingerprint` = SHA-256 of the **file content** — fast pre-parse dedup.
- `matches.fingerprint` = hash of `date|map|length|sortedToons` — **game** dedup (compatible
  with the historical overlay). `project_match` is idempotent (delete-then-insert in a transaction,
  retry on deadlock 40P01/40001, `players` UPSERT OUTSIDE the transaction so as not to serialise
  the backfill).

## Reference data (`dim.rs`) — two mutually exclusive sources

Background task at startup then every 24 h, **best-effort**; as long as `dim_heroes` is
empty (source unreachable at startup, e.g. box switched off), retry every **10 min**.
Failed writes are logged (`warn`):
- `REFERENTIAL_URL` (**takes priority**): published `referential.tar.gz` snapshot → self-contained
  bundle without live HotsPatchNotes (`dim::ingest_snapshot`).
- `HOTSPATCHNOTES_URL`: live API (maintainer setup) → `sync_heroes`, `sync_talents`,
  `vendor_images` (portraits/maps copied into `IMAGES_DIR`, served on `/images`),
  `sync_patches`, `backfill_hero_sections`.
- **Standalone mode** (`STANDALONE=1`, installer bundle): no network. When `dim_heroes` is empty
  and `REFERENTIAL_FILE` is set, it is seeded once from that on-disk snapshot
  (`dim::ingest_snapshot_file`, same ingest path as `ingest_snapshot`, one scratch dir per call);
  the 24 h refresh loop is not started. `Config::validate()` (run right after `from_env()`)
  rejects a non-loopback `BIND_ADDR` in this mode ("standalone mode requires a loopback BIND_ADDR").

Each **new patch** detected → WS event `patch.new` + optional outgoing webhook
(`PATCH_WEBHOOK_URL`, Discord-compatible format).

## Draft simulator (`draft/`)

**Pure, zero-I/O** engine (`mod.rs`): a format = ordered list of steps `{order, action}`;
`first_pick: Side` decouples the order (First/Second) from the visual side (blue/red). Authoritative
singleton state in memory (`AppState.draft`), **persisted** in `draft_live` (JSONB, id=1),
reloaded at startup. Each mutation broadcasts `draft.updated` on `/ws`.
Design: `docs/specs/2026-06-19-draft-simulator-design.md`.
**Draft from a replay** (`draft/replay.rs`, `schema_version` 2): `DraftState.replay` (`#[serde(default)]`,
so v1 states reload) carries the queue of loaded Custom games (`LoadedGame`: script of the 16 steps,
player names), the current game, `fearless`, auto-play (`playing`, `interval_ms` 30,000 by default,
`next_at_ms`). `reveal_phase`/`back_phase` advance by **phase** (block of consecutive steps with the same role);
`select_game` prepares an empty draft (fearless: series bans = picks of the queue's previous games);
`select_game_with` / `add_game_with` / `set_fearless_with` take the format of an active cast series, which
overrides `replay.fearless` (M5: Fearless → Fearless + series bans; Standard/Normal → Standard, the replay
scripts following the 16-step Standard order).
`public()` strips scripts and player names from unrevealed picks (view served by `GET /api/draft`).
Spec: `docs/specs/2026-10-25-draft-replay-design.md`.

## Cast control room (`cast/`)

Module `cast/`: `mod.rs` (types `Series`, `Team`, `PoolMap`/`MapStatus`, `GameRecord`, `Scene`,
`LineupMode`, `SlideState`, `CastError`), `series.rs` (**pure** operations, zero I/O: pool, games,
score, lineup mode), `store.rs` (persistence), `lineup.rs` (recent heroes per player, read-only on `match_players`), `summary.rs` (`GameSummary` extracted from the storm-stats output of a Custom replay, pure), `replay.rs` (route `POST /api/cast/games/{n}/replay`: on-the-fly decoding, nothing stored outside `cast_live`), `api.rs` (routes). Authoritative singleton series in
memory (`AppState.cast`), **persisted** in `cast_live` (JSONB, id=1), reloaded at startup; any
unreadable state (non-object, `players` ≠ 5, unknown scene/format/side/status) → fallback to a fresh
inactive series, startup never fails. Each mutation: a single write lock on a copy (a
`CastError` changes nothing), then persistence and `cast.updated` broadcast on `/ws`. **Versioned
persistence**: each mutation gets a version (`AppState.cast_version`, assigned under the write lock) and
persists **its own** snapshot; writes are serialised (`AppState.cast_saved` = last version written)
and a snapshot older than the last one written is ignored — two concurrent mutations can never
leave the old state in the database. Write failure → the route answers `500 persist_failed` (R24; the state stays
applied in memory and `cast.updated` is broadcast; edge case: if the v2 save fails and then a concurrent v1 save succeeds, the database keeps v1 and memory v2 — the v2 caller received `500 persist_failed`, consistent with R24, a restart would reload v1). **Draft sync** (`api::sync_draft`): re-reads the
**current** cast state under a read lock while the draft write lock is held (never both write locks),
so that a more recent mutation is not overwritten by the snapshot of an older one. Loading
(`store::load`): database read error logged at `error`, unreadable JSON at `warn`, `schema_version`
newer than the code at `warn` (loaded anyway, tolerant reading). Errors: `CastError::code()` gives
the stable code returned by the routes. Open mode like
`/api/draft`. Rules: `mark_map` refuses an already-played map (`MapAlreadyPlayed`); every mutation except
`POST /api/cast/series` and `/scene` requires `active` (`SeriesInactive`).
`POST /api/cast/series` also aligns the draft (format, steps, fresh game and series: `DraftState::align_to_series`, after the teams/score/Bo sync, locks never held together); the replay queue is kept but its current game is deselected and auto-play stopped (like `/api/draft/config`), so that the engine never diverges from the series format.
Design: `docs/specs/2026-10-25-cast-control-room-design.md`.

## Live feed (`live/`)

Game in progress, fed by `POST /api/live/chunk` (blocks of `TempWriteReplayP1`). `mod.rs`:
single in-memory session (`AppState.live`, never persisted — the archived replay remains the source
of truth), one `storm_replay::StreamDecoder` per stream, idempotence by offset (`plan_chunk`,
pure function: duplicate ignored, overlap trimmed, gap → `409`), memory caps (64 MB per
stream → `413`, feed bounded to 2,000 items, ability timelines limited to 3 min), status `live`/`ended`/
`unreadable` (a decoding error freezes the session without ever inventing state). `state.rs`:
**pure** reducer (heroes, team levels, talents, deaths/takedowns, camps, objectives, ability
orders by `_userid` with 0.3 s dedup) and view filtered by level `self`/`allies`/`all`.
Heroes: `hero_units.json` (tracker unit → canonical name, 90/90), derived from the archive by
`examples/derive_hero_units.rs`. End-to-end simulator: `examples/live_sim.rs`.
Design: `docs/specs/2026-10-24-feed-live-en-partie-design.md`.

## Configuration (env — `config.rs`, template: `.env.example`)

| Variable | Default | Role |
|---|---|---|
| `DATABASE_URL` | **required** | Postgres |
| `BIND_ADDR` | `127.0.0.1:8088` | HTTP listen address |
| `ARCHIVE_DIR` | `./.archive` | raw replays (source of truth) |
| `RAW_CACHE_DIR` / `RAW_CACHE_MAX_BYTES` | `./.raw-cache` / 5 GiB | LRU cache of dumps |
| `ADMIN_TOKEN` | empty = **open mode** | empty: no admin auth (LAN/Tailscale self-hosting); set: Bearer required on admin writes |
| `WEB_DIR` | empty = API only | built front-end (`web-rs/dist`: `overlay/`, `sources/`, `app/`, one Trunk output each) served by the binary. Every page answers at `/`, after the API routes: `/cast/overlay` and `/draft/overlay` get `overlay/index.html`; `/ticker`, `/now-playing`, `/widget` and `/queue` get `sources/index.html`; everything else `app/index.html` (`no-cache`, read at each request). The bundles' files are under `/v2/<bundle>/` (fingerprinted, cached for a year; a missing file-like path answers 404, never an HTML page); `/v2/...` pages still work. The Docker image sets `/app/web` |
| `IMAGES_DIR` | `./.images` | vendored images, served on `/images` |
| `REDIS_URL` / `JARVIS_CHANNEL` | empty = no emission / `jarvis:events` (box production: `storm-codex:match_completed`) | Jarvis events |
| `HOTSPATCHNOTES_URL` / `REFERENTIAL_URL` | empty | reference data (see above) |
| `STANDALONE` | empty = off | `1` = standalone (installer) mode: loopback-only `BIND_ADDR` (checked at startup), no reference-data network refresh |
| `REFERENTIAL_FILE` | empty | bundled `referential.tar.gz` that seeds an empty `dim_heroes` in standalone mode |
| `ORPHEUS_URL` | empty | music proxy for `/api/now-playing` |
| `PATCH_WEBHOOK_URL` | empty | outgoing new-patch notification |
| `AZURE_PUSH_URL` / `AZURE_PUSH_TOKEN` | empty | **dormant** (Twitch extension abandoned) |

**Stateless** server apart from the data directories: all config through env (V2-ready).
