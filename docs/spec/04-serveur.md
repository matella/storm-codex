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
  → WS `match.parsed` + (dormant) Azure push
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

- **Insights corpus warm-up** (`insights::cache::warm_up`): spawned at startup, loads the in-memory corpus of the
  local insights in the background (see `05-api.md`, "Local insights"; it also serves the session review `/api/sessions`);
  failures are logged, never fatal.
- **Insights corpus concurrency** (`insights::cache::corpus`): the version check runs without any lock and an
  up-to-date corpus is returned as a shared `Arc`, so cache hits run concurrently. A reload (id scan + `load`)
  holds only a refresh guard (one reload at a time; waiters re-check the version once it is free) and publishes
  its result under a brief lock. `invalidate` (reference hero repair) never waits for a reload: it bumps a
  generation, and a reload begun under an older generation is not published.

- **Graceful shutdown**: Ctrl-C or SIGTERM (`docker stop`) — or, in standalone mode,
  `POST /api/standalone/shutdown` → the server stops accepting connections
  and lets in-flight requests finish (`with_graceful_shutdown`). Measured in the container:
  shutdown in ~0.5 s, exit code 0 (before: SIGTERM ignored, SIGKILL after 10 s). Unfinished
  parses are resumed at the next startup (above).
- **`migrate` subcommand** (`migrate_cmd.rs`): `storm-codex-server migrate [--to <version>]` runs this build's
  embedded migrations on `DATABASE_URL` and exits without starting the server (no bind, no background task).
  Without `--to`: every pending migration; with `--to N`: up to version `N` only (`run_to`). Exit codes: **0** done;
  **2** usage (bad arguments, `DATABASE_URL` unset); **3** `N` is newer than this build's newest migration (the
  database or backup comes from a newer app); **1** any other failure (unknown version `N`, connection, migration
  error). Messages go to stderr; `DATABASE_URL` (it holds a password) is never printed. Used by the desktop shell's
  backup import (`09-operations.md`, "Untrusted dump: data only"): first `--to N` on the staging database, then
  without a target once the rows are restored.
- **`refdata trim` subcommand** (`refdata/load.rs`): `storm-codex-server refdata trim --from <dataset.tar.gz>
  --out <referential.tar.gz>` checks the release tarball against `<from>.sha256` (sha256sum format), refuses a
  schema other than v1 or a file that does not match its manifest SHA-256, and writes the standalone bundle: the
  manifest, `game.json`, `patches.json`, `assets.json`, `locales/enus.json` and the hero/map images the app uses
  (no talent icons, no other locale). Prints the dataset version, file count, size and SHA-256. Exit codes:
  **0** written; **2** usage; **1** any refusal or I/O error. Run by the desktop release workflow (`09-operations.md`).
- **Compression**: `/v2/` serves an accepted `.br` or `.gz` sibling before falling back to
  the original. Representations vary on `Accept-Encoding`; MIME types come from the original
  path. The global `CompressionLayer` skips already-encoded responses and handles eligible
  originals on the fly (its default predicate skips small responses, images and streams).
- **SPA HTTP cache**: successful fingerprinted Trunk CSS, JS and WASM assets get
  `Cache-Control: public, max-age=31536000, immutable`. HTML and unversioned assets (including
  copied fonts) get `no-cache`. Missing assets return 404 with `no-store`, not the SPA HTML.
- **Referrer privacy**: every application response sends `Referrer-Policy: no-referrer`,
  including standalone access-guard rejections.
- **Cast/draft write guard**: middleware `auth::guard_writes` (see 05-api.md).

## Standalone import (`standalone/`)

Only in standalone mode (`STANDALONE=1`); without it the routes below answer 404 and nothing
starts. One background task (`standalone/orchestrator.rs`) owns the upload ledger and runs,
once the listener is bound (`/api/health` answers 200 during the whole sequence):

1. **initialize** import (phase `seeding`); reference sync runs separately and does not gate
   import. In a background task started **once the first import run ends** (step 6; so it does not
   compete with the backfill for `parse_sem`, whose saturation makes uploads answer 202
   before their outcome is known), the **re-process after an update**: every
   upload `parsed` with `parser_version < PARSER_VERSION` is re-parsed and re-projected
   (`admin::outdated_uploads` + `admin::reprocess_one`, one `parse_sem` permit each, so it
   shares the parse budget with the live uploads), then the outdated scouting replays
   (`scouting::api::reprocess_stale`). Progress in the status (`reprocessTotal`,
   `reprocessDone`; `0`/`0` when nothing is outdated). It stops between two replays on
   shutdown; the rest is picked up at the next start (the query is the criterion). An upload
   the new parser cannot re-process keeps its previous projection and gets the current
   `parser_version` (`admin::mark_reprocess_failed`), so it is not re-parsed at every start.
   Rows `parse_failed` are not retried automatically: `POST /api/admin/reprocess` still
   retries both;
2. **detect** the folders: every `Documents/Heroes of the Storm/Accounts/*/*/Replays/Multiplayer`
   (known-folder API, OneDrive-relocated Documents included) plus the extra folders saved in
   `app_settings.standalone_extra_folders`; each extra folder is expanded to the folders under it
   that hold replays (itself when none does yet). None → phase `no-folders`;
3. **ledger** (`LEDGER_FILE`) opened; a corrupt file is moved to `<file>.corrupt` and an empty
   ledger started (the server dedups re-sent files). Writes are atomic (unique temp file per
   write, synced, renamed); overlapping writes publish in order (sequence number checked under
   a lock: an older snapshot never replaces a newer one); stale temp files are removed on open;
4. **live watcher** started on the folders before the worker lists the backfill batch, and
   **disk gate** on `ARCHIVE_DIR`, sized on that same batch of replays **not yet in the ledger**: free space must be ≥ `2 × their size + 1 GiB`
   (`storm_codex_supervisor::required_free_bytes`); otherwise phase `disk-full`: no backfill
   until a rescan passes, but the live watcher still runs (new games keep being imported). When
   the free space cannot be measured (unusual filesystem), a warning is logged and the import is
   **not** blocked;
5. **upload token**: a fresh random token is generated at each start and its SHA-256 written to
   the single `upload_tokens` row named `standalone-watcher` (inserted once, then updated in
   place with `revoked_at` cleared, under a transaction advisory lock: no duplicate row, never
   stuck revoked, the previous session's token stops working). The clear token lives only in
   memory (never logged, never on disk). `/api/upload` keeps its auth contract for everyone.
   One server per database: a second server on the same database would rotate the token.
   Steps 3 or 5 failing → phase `error`;
6. **backfill** (phase `importing` appears
   together with the nonzero batch total; committed projections are reflected in **folder identity**): oldest
   first, 4 at a time (at most the CPU count: the server parses one replay per core and
   answers 202 when all are busy, so a parse failure would be recorded as uploaded), through `POST http://<bound address>/api/upload` (the address actually
   bound, so `BIND_ADDR` may use port 0). Replays reported by the watcher meanwhile wait.
   In the live loop (normally phase `ready`), queued paths outside the current effective folders
   are discarded. Each remaining replay is uploaded unless the ledger already has it, and its
   outcome is recorded (write-through).

A rescan or a folder change received during the backfill cancels it (the ledger is flushed) and
the import restarts. Counts describe the current import run (backfill + live uploads since it
started). An empty scan does not enter `importing`; if the same folders are already `ready`, it
keeps the settled counters, phase and disk measurements. Otherwise, a successful empty scan
settles directly into `ready` with zero counters. `failed` lists the files that exhausted their retries
(retried at the next start), `rejected` the files refused as bad content during the run: a 4xx
content error, or a `200 {"status": "parse_failed", "error_class": c}` answer (the replay is
archived but gave no match: too old, unsupported mode, computer players, stats failure...),
listed with reason `not imported: c` and recorded in the ledger as rejected (never re-sent: the
server keeps the archived copy, and `POST /api/admin/reprocess` re-parses `parse_failed` uploads).

Overlapping folder saves resolve their folder lists before persisting them. Persistence,
publication to `extraFolders`, and queueing are serialized so the worker applies changes in
the order they were saved.

**Shutdown** (Ctrl-C, SIGTERM or `POST /api/standalone/shutdown`): the import task is told to
stop first, while the server still answers: in-flight uploads are abandoned (not recorded, the
server dedups them at the next start), the ledger is flushed once and the task ends; it is
awaited for at most 10 s (then aborted) before the HTTP server drains and `run()` returns.

| Route | Answer |
|---|---|
| `GET /api/standalone/status` | `{phase, folders, extraFolders, total, done, duplicates, rejected: [{file, reason}], failed: [file], freeBytes, neededBytes, dataDir, version, reprocessTotal, reprocessDone}` — `phase` ∈ `seeding`, `importing`, `ready`, `no-folders`, `disk-full`, `error`; `folders` = effective folders (detected + replay folders found under each extra); `extraFolders` = the extra folders exactly as saved (the raw list `PUT /folders` replaces, loaded from `app_settings` before the server listens — so already right during `seeding` — and updated on each `PUT`; a refused `PUT` (400) changes nothing; `[]` when none) — clients edit this list, never `folders`; `freeBytes`/`neededBytes` `null` before the first disk gate; `dataDir` = parent of `ARCHIVE_DIR`; `version` = server crate version; `reprocessTotal` / `reprocessDone` = background re-process after a parser bump (step 1) |
| `GET /api/standalone/identity` | `[{folder, toon, name, aliases, confirmed}]` in effective-folder order. Unresolved folders have `toon: null`, `name: null`, and `aliases: []`; `confirmed` is `true` when the folder's toon is in the persisted confirmed set (below). Read-only; never changes manual names. |
| `PUT /api/standalone/folders` | body `{folders: [path]}` replaces the extra folders (send `extraFolders` ± one path). Each must be an existing plain local directory (no UNC, device or verbatim path; saved entries failing that rule are dropped on load, so a restored backup cannot point the watcher at a share), else **400** `{error: "not an existing folder: …"}`; more than 64 entries → **400** `{error: "too many folders …"}`, a body over 64 KiB → **413**; only directory listings are read. Saved
| `POST /api/standalone/rescan` | **202**; re-detects the folders (a HotS replay folder created since, e.g. after the first game or for a new account, is picked up and watched), then checks for pending work. **503** `{error: …}` if the worker queue is closed |
| `POST /api/standalone/shutdown` | **202**; same graceful shutdown as Ctrl-C (used by the desktop shell before it stops Postgres) |
| `POST /api/admin/identity/confirm` 🔒 | body `{toon}` — idempotent upsert into `confirmed_identity` (`docs/specs/2026-11-21-standalone-confirmed-identity.md`); admin-gated like `PUT /api/admin/settings`. **404** if `toon` is empty; no other validation (the server only ever sees toons it itself produced and displayed) |
| `DELETE /api/admin/identity/confirm/{toon}` 🔒 | idempotent delete from `confirmed_identity`; same admin gating. **404** if `toon` is empty |

The routes are unauthenticated (loopback-only server, single user, no `ADMIN_TOKEN`, so the
whole API is in open mode). In standalone mode every request to **any** route (the whole API,
the pages, `/v2` bundles, `/images`, `/ws`; one middleware on the whole app, `app_router`) must
come from this PC, otherwise **403**:
- **Host** (every method, reads included): the `Host` header (or the HTTP/2 authority) must
  be exactly `127.0.0.1`, `localhost` or `[::1]` (case-sensitive, no suffix), with no port or
  the port actually bound; a missing `Host` is refused. This blocks DNS rebinding (a foreign
  name resolving to 127.0.0.1 cannot read the stats or the status, whose folders and data path
  carry the Windows user name, mint an upload token or delete data). The desktop shell loads
  `http://127.0.0.1:<port>/`, which passes; OBS browser sources on `127.0.0.1`/`localhost`
  and the bound port pass too;
- **Origin** (every method, `GET` and WebSocket upgrades included): a request carrying an
  `Origin` header is refused unless it is the app's own origin: `http://` followed by an
  authority that passes the Host rule above (loopback name, no port or the bound port). This
  refuses a foreign page posting to the local server (including a body-less
  `fetch(…, {mode: 'no-cors'})` to `/api/admin/reprocess` or `/api/draft/reset`), reading it
  with a CORS fetch, or opening `ws://127.0.0.1:<port>/ws` (a WebSocket is not bound by the
  same-origin policy and its upgrade is a `GET`), as well as `Origin: null` and another local
  web app on another port. Requests without `Origin` (navigations, `<img>`, OBS browser
  sources, the in-process uploader, the shell) are unaffected.

Server mode (the box, `STANDALONE` unset) is unchanged: no Host or Origin filter (it sits behind
its reverse proxy).

**Operator identity on a player's PC.** From each effective folder ending in
`Heroes of the Storm/Accounts/<account>/<toon>/Replays/Multiplayer`, use the exact projected
ToonHandle (`<region>-Hero-<realm>-<id>`). No ownership inference for flat folders or unrecognized
handles. `GET /api/standalone/identity` reads `players::names::name_history` for the known handles:
latest nonblank name by `matches.played_at` (undated last, equal last-seen time ties by name), with all
nonblank historical aliases. Reads see committed projections, including after a no-op backfill,
restart, or a live replay accepted before parsing completed; no identity state is written by imports.

**Confirmed identity** (`docs/specs/2026-11-21-standalone-confirmed-identity.md`): a third,
persisted source of "me" alongside live folder detection and manual names. Migration 0022 adds
`confirmed_identity(toon TEXT PRIMARY KEY, confirmed_at TIMESTAMPTZ)`. `standalone::operator_toons`
(`crate::standalone::operator_toons`, now `async`) is the sorted, deduplicated union of the live
folders' toons and every row of `confirmed_identity` — a confirmed toon stays "me" even after its
replay folder is removed or renamed. Saving is explicit per account (the Welcome page's "Add to My
Identity" button, never automatic), so a shared PC's detected-but-unconfirmed accounts are not
silently added. All call sites (`goal.rs`, `insights/api.rs`, `lobby/api.rs`, `manage.rs`,
`read.rs`) await it; no behaviour change beyond the extra persisted toons.

The app uses verified toons across renames plus separately saved manual `operator_names`. The old
standalone heuristic and migration 0003 shared that setting with the manual editor, so neither old
heuristic values nor the seed (`matella`) are automatically removed: they cannot safely be separated
from manual choices. Review or clear those entries in Welcome or Settings if they are not yours.
Box mode still uses manual settings only.

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
(`summary.rs`) and inserts into `scouting_games` — never into `uploads`/`matches`. Before the insert
(and the re-parse update) it applies the projection's talent-based hero correction from `dim_talents`
to `data` and the summary players, after `LOCK TABLE scouting_games IN ROW EXCLUSIVE MODE`, so a
standalone reference activation either repairs the row or commits its lookup first. Pure core
unit-tested (`summary`, `side`, `facts`, `pack`, `xlsx`, `analysis`); `api.rs` recomputes the
facts under the report's row lock (`SELECT … FOR UPDATE`) on every mutation. Hero roles:
`dim_heroes.data->>'expandedRole'` (the parser's `gameStats.Role` is 0 in custom games).
`POST /api/admin/reprocess` also re-parses stale `scouting_games` and then recomputes.

## Archive reads (`read.rs`, `read/filter.rs`)

`GET /api/matches` and `/api/matches.csv` build their SQL with `sqlx::QueryBuilder`: constant fragments
only, every user value bound. `read/filter.rs` parses and validates the replay search parameters (spec
`docs/specs/2026-11-19-replay-search.md`; unit-tested) and appends their conditions. One `FROM`/`WHERE`
builder serves the JSON page and the CSV. The JSON list uses one SQL statement: materialize matching IDs
and sort keys once, then count and page that set in the same snapshot.
The operator perspective row is joined only when a filter reads it, as one `DISTINCT ON (match_id)` pass
over the perspective rows (no JSON read); the page is chosen on match IDs first, then only its rows are
expanded (players JSON, per-row perspective lookup), so a deep `offset` does not expand skipped rows.
Postgres aggregates JSON records and the handler relays that JSON text without a second serialization.
The MVP filter is an `EXISTS` on the same `(match_id, toon_handle)` row, served by the partial index of
migration 0018.

## Fingerprints (two-level dedup)

- `uploads.fingerprint` = SHA-256 of the **file content** — fast pre-parse dedup.
- `matches.fingerprint` = hash of `date|map|length|sortedToons` — **game** dedup (compatible
  with the historical overlay). `project_match` is idempotent (delete-then-insert in a transaction,
  retry on deadlock 40P01/40001, `players` UPSERT OUTSIDE the transaction so as not to serialise
  the backfill).

## Reference data (`refdata/`) — one adapter, the HotsPatchNotes dataset

Spec `docs/specs/2026-11-15-reference-dataset-adapter-design.md`. HotsPatchNotes publishes a
**versioned dataset** (schema v1) as GitHub releases (`data-<sha256 of dataset.tar.gz>`); its old
HTTP API is gone. `refdata::build` (pure, no I/O) turns the dataset's `manifest.json`, `game.json`,
`locales/enus.json`, `patches.json` and `assets.json` into the rows of `dim_heroes`, `dim_talents`,
`dim_patches` (+ `aliases`) and `patch_hero_sections`, plus the images to copy. `refdata::load::store`
writes them in **one transaction** (full refresh; a dataset row that disappears is removed) and
records `app_settings.refdata_dataset_version`.

- **Join keys.** Hero: the parser name (`dim_heroes.id` = `name`), via the dataset `attributeId`
  → `storm_stats::constants::hero_attribute()`; fallback the English name, then the dataset key.
  Talent: `tree_id` = the dataset `talentId` (= the parser's `TierNChoice`); name/description from
  `enus.talents[linkId]`, tier from the `LevelN` key, `type` = `abilityType`.
- **Front values kept.** Universe: `Starcraft` → `StarCraft`, `Classic` → `Nexus`. Patch type
  labels of the old API: `live` → `Patch Notes`, `balance` → `Balance Update`, `hotfix` →
  `Hotfix Patch`, `ptr` → `PTR`. Hero icon `/images/heroes/<slug>.png`, slug = English name,
  accents folded, lowercase alphanumeric (the old file names). Map art
  `/images/battlegrounds/<slug>.png` from `structure.loadingScreenImage` (the release's
  `Battleground.slug`, the same rule as the front's `map_slug`; see Images below).
- **Images (D3).** The square `heroSelect` icon when the dataset bundles it, else the loading
  portrait. A file already in `IMAGES_DIR` is never overwritten, so the square icons vendored by
  the old API stay on the box; new heroes and fresh installs get the portrait. Destinations are
  built from dataset names, so each path segment must be a plain file name (ASCII alphanumerics,
  `-`, `_`, `.`, `(`, `)`; not `.`/`..`), otherwise the image is skipped with a warning
  (`refdata::safe_image_dest`). A map image is named from the release's `Battleground.slug`
  (lowercase, apostrophes dropped, whitespace → `-`, only `[a-z0-9-_.()]` kept: "Checkpoint:
  Hanamura" → `checkpoint-hanamura`); a release without `slug` gets the same rule from
  `refdata::map_slug`, and the front's `map_slug` (`web-rs/core`) applies it too.
- **Patch detail.** Rebuilt in the old API shape and stored in `dim_patches.data.detail`:
  `sectionType` = `Hero` (section `heroId`), `Battleground` (`battlegroundId`) or `General`;
  `anchor` = section `id`; `tableOfContents` from `heading`/`level`; `content` = the article HTML
  with each section `id` set on its heading (matched by position from the end; rebuilt from the
  sections when they do not line up). `classification` / `shortSummary` (gone from the dataset)
  come from `refdata::classify`, a port of the old `PatchClassifier.cs`, run on each section's
  `text`. Counts: `heroCount`/`mapCount` = distinct heroes/maps of the sections;
  `officialLink` = first `official` source (else `officialMirror`/`officialArchive`);
  `hasContent` = `availability == "available"`; `communityOnly` = only `community` sources;
  `metadataOnly` patches keep the dataset `reason` in the detail.
- **Hero sections.** One `patch_hero_sections` row per section with a `heroId` of available
  English content, `hero_name` = the parser name, `channel` = `live`/`ptr`.
- **Completeness.** Matching checksums do not make a release complete. `refdata::build` refuses
  (before any write or staging) a release whose required catalogs are empty (`game.json`
  heroes/battlegrounds, `enus.json` heroes/talents/battlegrounds, `assets.json`, `patches.json`, or
  no talent or artwork at all), an `assets.json` record without a path or SHA-256, and a declared
  hero portrait or map `loadingScreenImage` with no asset record. Documented exceptions: a hero's
  `heroSelect` icon may be absent when its `loading` portrait resolves (D3; the trimmed test fixture
  relies on it), a hero or map that declares no artwork gets none, and a map without an English name
  is skipped. Copying from `dataset.tar.gz` fails when a listed image is not in the archive, so a
  generation is never staged with only the images that survived. This applies to the box refresh
  too: a box round whose archive lacks a listed image copies the images found before the error
  and logs "images not copied". The rows stay ingested, so the version counts as current: the
  missing images are only retried with the next `datasetVersion`. The pinned release
  (`desktop/referential-pin.txt`) resolves all 91 hero and 35 map artworks.
- **Patch-note image proxy/cache** (spec `docs/specs/2026-11-19-patch-image-proxy.md`, P1). Patch
  HTML (`dim_patches.data.detail.content`, `patch_hero_sections.content`) can embed `<img src>`
  pointing at Blizzard's CDN (observed host: `bnetcmsus-a.akamaihd.net`). `refdata::patchimg::rewrite`
  runs inside `build_patch` (pure, no I/O) and replaces any such `src` with a local
  `/images/patch-images/<sha256(url)>.<ext>` path **before** the HTML is stored — in both box and
  standalone mode, so no stored content ever embeds a live third-party URL. `IMAGES_DIR/patch-images/`
  is a plain subfolder, already served by the `/images` `ServeDir` (no new route). Only
  `refdata::load::refresh` (box) actually fetches the referenced images (`patchimg::fetch_missing`,
  capped by `PATCH_IMAGE_MAX_FILE_BYTES` and a `image/png|jpeg|gif|webp|avif` content-type allowlist,
  atomic temp+rename write); standalone's `ingest_bundle`/`stage_bundle` stay 100% network-free, so an
  image never fetched by the box before the bundle was trimmed is simply missing offline (broken
  `<img>`, no request attempted) — strictly better than the previous live leak. Unlike the rest of
  `IMAGES_DIR` (permanent, D3), `patch-images/` is a bounded cache: `raw::enforce_lru` evicts it down
  to `PATCH_IMAGE_CACHE_MAX_BYTES` after each fetch pass. One failing image is logged and skipped,
  same policy as the existing hero/map art fetch; it never fails the patch data write, which already
  committed first.
- **Archive budget** (`refdata::load::Limits::DEFAULT`, box and standalone). `dataset.tar.gz` may
  be at most **512 MiB** downloaded (the HTTP stream stops one byte past it), **1 GiB** once
  decompressed (counted at the gzip decoder, skipped entries included), and **32 MiB** per image
  entry (refused from the tar header before it is buffered; data files keep the 256 MiB cap). The
  pinned release is 100,024,455 B compressed, 158,981,122 B expanded, largest image 5,393,600 B. A
  breach refuses the round: the download is deleted, standalone staging is removed and the active
  generation stays. The legacy `REFERENTIAL_FILE` bundle is hashed by streaming and refused over
  512 MiB. The local tar readers used for that bundle and `refdata trim` buffer the kept entries
  at once, so their memory is bounded by the 1 GiB expansion budget, not by one entry.

**Box** (`REFDATA_URL`, default
`https://github.com/matella/HeroesOfTheStormPatchNotes/releases/latest/download/`; empty =
no refresh). Background task at startup then every 24 h, **best-effort**; while `dim_heroes` is
empty, retry every **10 min**. Each round (`refdata::load::refresh`):
1. download `manifest.json`; refuse a `schemaVersion` other than 1 (logged, previous data kept);
2. stop when `datasetVersion` equals `app_settings.refdata_dataset_version` (the round downloaded
   only the manifest);
3. else download `game.json`, `enus.json`, `patches.json`, `assets.json`, check each against the
   manifest SHA-256 (any mismatch refuses the round before a write), build and store;
4. when some image is missing from `IMAGES_DIR`, download `dataset.tar.gz`, check it against
   `dataset.tar.gz.sha256`, and copy the missing images (each checked against `assets.json`).

**Standalone mode** (`STANDALONE=1`; approved download spec 2026-11-19). The reference worker
(`refdata/standalone.rs`) starts alongside import. It checks the same fixed stable feed at startup
and every 24 h. Failed checks retry after 1, 5 and 30 minutes, then return to the daily cadence.
Manual refresh and enabling queue a check; requests coalesce and rounds never overlap. The persisted
`app_settings.standalone_reference_enabled` defaults to true. Disabling prevents new rounds and
activation of work still being fetched; it does not remove cached rows or assets. Each preference
change bumps a round epoch: a round only fetches and activates while its starting epoch is current,
so disabling then re-enabling during a download cannot revive that round; the queued check runs
once instead.

- The manifest schema and metadata checks use the existing adapter. An unchanged version with
  complete verified local images fetches only `manifest.json`. Otherwise the worker verifies the
  archive checksum, verifies every required image against both asset and manifest hashes, and
  flushes the images to an isolated generation under `<data>/reference-assets/<revision>`.
- The shared transaction writer installs the reference rows and records the generation and version
  in `app_settings` (`standalone_reference_active` and `refdata_dataset_version`). Until commit the
  old generation remains active. Standalone hero and battleground artwork use the committed
  generation, with legacy `IMAGES_DIR` as fallback; `/images/minimaps` is unchanged. Superseded
  managed generations are retired on restart. Cached legacy rows and files are preserved on upgrade.
  A successful image from the captured managed generation is immutable for one year only when
  the query is exactly `ref=<that generation's revision>`. Unkeyed or stale URLs and legacy bytes
  still revalidate (`no-cache`); misses use `no-store`. No `If-Modified-Since` is used on these
  routes, since a same-second activation can change bytes without changing the image name.
- `REFERENTIAL_FILE`, when provided, remains an offline seed and uses the same activation path,
  even with network sync disabled. A changed bundle is accepted until a network generation is
  active; then a legacy resource cannot downgrade it. A restore with missing images keeps its
  rows: the next successful check restages even the same version and publishes a new revision.
  Startup checks the active generation's files locally (no network, whatever the preference) and
  reports the result as the status `artwork` flag, so cached rows with missing artwork are not
  presented as available offline. Before the server binds, this check is cheap: every listed image
  must be a non-empty file. The worker then hashes every image in the background before its first
  bundle or network check and corrects `artwork`; that first check reuses this result instead of
  hashing again when it is less than 5 minutes old (sync may be enabled long after startup). The
  background hash stops between images when the app is quitting. Every later check that finds the active version (network manifest or same legacy
  bundle) re-verifies the files and sets `artwork` to the result, so it returns to true once the
  files are back without a new activation.
- A release refused for its content is remembered in memory by `datasetVersion`: at the first
  resource-budget or shape failure, but only after two consecutive SHA-256 mismatches on the same
  version (a single mismatch may be a corrupted transfer; the next check downloads it again). While
  `manifest.json` still names it, later automatic checks fetch only the manifest and report
  `phase: error`, instead of downloading the data files and archive again; the first such check
  logs a warning, later ones log at debug. A new version clears it, so does a restart (the first
  check after it retries the release once), and so does the user's Retry
  (`POST /api/standalone/reference/refresh`), which downloads the release once more. Automatic
  checks (schedule, failure retries, enabling) keep it.
- A check interrupted by shutdown exits quietly, and one cut short by a preference change counts as
  cancelled, whatever error the interruption caused; neither is reported as a failed check.
  Cached rows remain readable during a check or failure.
- Activation repairs hero attribution of already projected replays in bounded batches, using stored
  talent IDs and the live correction rule. It updates the promoted hero and affected player/match
  JSON (including takedowns), retaining original hero provenance and match/upload IDs. It does not
  re-decode or re-upload replays. Only the local activation transaction fences projection writes;
  neither network I/O nor staging holds that fence. Repair is idempotent and invalidates the
  insights corpus when it changes a match. The same transaction repairs scouting replays
  (`scouting::api::repair_reference_heroes`, fence `LOCK TABLE scouting_games IN SHARE ROW EXCLUSIVE
  MODE`): `scouting_games.data` players/takedowns and `summary` players by toon (draft picks and bans
  are untouched). After commit and before the new revision is published, every report's facts are
  recomputed (repaired heroes, roles from the new `dim_heroes`); `facts_version` only moves for a
  report whose facts changed (jsonb comparison), so an imported analysis is not marked stale for nothing. The 2D replay disk cache stores only replay-derived
  geometry and the playerId → toon mapping; player name/hero/team/win are read from
  `match_players` on every request, so a cache entry warmed before the repair (even across a
  restart) serves the repaired hero. The entry (`{id}-replay2d-v{VIEWER_VERSION}-split2.json`) is
  the toons JSON line, the model's byte length as a decimal line, then the model's serialised
  bytes; a hit splices the fresh `players` array into those bytes without parsing the model. It is
  written to a unique temporary file in the cache directory and renamed into place, so readers
  never see a partial entry; a malformed header or a model whose length differs from the header
  is a miss (decoded again), as are entries of the older `-split` layout. A model that cannot be serialised as a JSON
  object without a `players` key returns 500 and is not cached.

`Config::validate()` (run right after `from_env()`) rejects a non-loopback `BIND_ADDR` in this mode
("standalone mode requires a loopback BIND_ADDR"): the IP must be exactly `127.0.0.1` or `::1`, the
addresses the Host guard accepts (another 127/8 address would make every in-process upload a 403).

Each **new patch** detected (a dataset id not in `dim_patches`; none on the first seed, when the
table is empty) → WS event `patch.new` + optional outgoing webhook (`PATCH_WEBHOOK_URL`,
Discord-compatible format); `internalId` is the dataset id.

## Draft simulator (`draft/`)

**Pure, zero-I/O** engine (`mod.rs`): a format = ordered list of steps `{order, action}`;
`first_pick: Side` decouples the order (First/Second) from the visual side (blue/red). Authoritative
singleton state in memory (`AppState.draft`), **persisted** in `draft_live` (JSONB, id=1),
reloaded at startup. Each mutation broadcasts `draft.updated` on `/ws`. **Versioned persistence**
(same pattern as cast): `api::persist` copies the state and takes its version (`AppState.draft_version`)
together under the draft read lock, so versions follow the order of the states, then hands the snapshot
to a single dedicated saver task (`AppState.draft_save_tx`, spawned by `draft::store::spawn_saver`) that
writes `draft_live` one request at a time and skips a snapshot not newer than the last one it wrote:
concurrent mutations, the cast → draft sync and the auto-play timer's ticks can never leave an older
draft in the database, and no lock is held across the database round trip. Write failure → the route
answers `500 persist_failed` (R24, same contract as cast: the state stays applied in memory and
`draft.updated` is broadcast; same v2-fails-then-v1-succeeds edge case). An auto-play tick has no
caller: its write failure is logged and the next tick or mutation retries. A cast mutation with draft
sync answers `500 persist_failed` if either the series or the draft write failed (the series is
persisted in both cases).
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
persists **its own** snapshot; writes to `cast_live` are serialised by a single dedicated saver task
(`AppState.cast_save_tx`, spawned by `cast::store::spawn_saver`) that processes one request at a
time in arrival order — callers hand off their snapshot and await a reply, so no lock is held across
the database round trip. A snapshot older than the last one the saver task has written is ignored
(`AppState.cast_saved`, a non-blocking hint of that last version, mirrored into memory after each
successful write): two concurrent mutations can never leave the old state in the database. Write failure → the route answers `500 persist_failed` (R24; the state stays
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

## Overlay layout persistence (`overlay_layout.rs`)

Migration 0020 adds the user-authored layout and widget tables. The five admin operations reuse
`auth::is_admin` before request body extraction, while the UUID-addressed OBS read is public. Saves
lock the parent and replace its placements in one transaction; full reads use a single statement
snapshot. See `05-api.md` for the wire contract. No widget data fetching or live events are introduced.

## Daily goal tracker (`goal.rs`)

`docs/specs/2026-11-20-streamer-widgets.md`, widget 7 — the one streamer widget needing new backend
state. Migration 0019 adds `app_goal` (singleton, see `06-modele-donnees.md`). `GoalKind`
(`wins_today`/`games_today`) and the day-boundary math (`day_bounds`, injectable `DateTime<FixedOffset>`
so it is testable without the real clock) and the kind switch (`compute_progress`) are pure, native
unit-tested (midnight reset across positive/negative offsets, kind switch, no-goal state); the handlers
are a thin SQL layer on top, same shape as `manage.rs`'s settings handlers.

- `GET`/`PUT /api/admin/goal` (admin-gated, same `auth::is_admin` contract as `manage::put_settings`):
  read/set/clear the goal. `PUT` upserts the single row (never a second one); a `kind` omitted or `null`
  clears it (idempotent); an unknown `kind` or non-positive `target` is `400`.
- `GET /api/goal/progress` (public, read-only, like the other OBS endpoints): **not stored** — recomputed
  from `matches`/`match_players` on every read, using the same operator-perspective tally as
  `manage::trends` (`app_settings.operator_names` in box mode, `standalone::operator_toons` in standalone
  mode) over the box's local-midnight-to-local-midnight window (`chrono::Local`'s offset, fixed for the
  duration of one request). `{kind, target, done, reset_at}`, all `null`/`0` with no goal set.
- `/goal` OBS source (`sources/`, registered in `v2_page`/`page_of` like the other session sources):
  polls `/api/goal/progress` every 10 s; renders a blank transparent frame when `kind`/`target` are
  `null` (no placeholder box).

## Configuration (env — `config.rs`, template: `.env.example`)

| Variable | Default | Role |
|---|---|---|
| `DATABASE_URL` | **required** | Postgres |
| `BIND_ADDR` | `127.0.0.1:8088` | HTTP listen address |
| `ARCHIVE_DIR` | `./.archive` | raw replays (source of truth) |
| `RAW_CACHE_DIR` / `RAW_CACHE_MAX_BYTES` | `./.raw-cache` / 5 GiB | LRU cache of dumps |
| `ADMIN_TOKEN` | empty = **open mode** | empty: no admin auth (LAN/Tailscale self-hosting); set: Bearer required on admin writes |
| `WEB_DIR` | empty = API only | built front-end (`web-rs/dist`: `overlay/`, `sources/`, `app/`, one Trunk output each) served by the binary. Every page answers at `/`, after the API routes: `/cast/overlay` and `/draft/overlay` get `overlay/index.html`; `/ticker`, `/widget`, `/queue`, `/recap`, `/stream-pool`, `/patch-banner`, `/record-flash`, `/nemesis`, `/goal` and `/overlay/custom` get `sources/index.html`; everything else (including `/overlay-builder`) gets `app/index.html` (`no-cache`, read at each request). The bundles' files are under `/v2/<bundle>/` (fingerprinted, cached for a year; a missing file-like path answers 404, never an HTML page); `/v2/...` pages still work. The Docker image sets `/app/web` |
| `IMAGES_DIR` | `./.images` | vendored images, served on `/images` |
| `PATCH_IMAGE_MAX_FILE_BYTES` | 8 MiB | per-image cap when the box fetches a Blizzard patch-note image (`refdata::patchimg`) |
| `PATCH_IMAGE_CACHE_MAX_BYTES` | 200 MiB | total bound on `IMAGES_DIR/patch-images` (LRU-evicted; unlike the rest of `IMAGES_DIR`, which is permanent) |
| `REFDATA_URL` | the producer's `releases/latest/download/`; empty = off | reference dataset release (box; see above) |
| `STANDALONE` | empty = off | `1` = standalone (installer) mode: loopback-only `BIND_ADDR` (checked at startup), independent reference sync |
| `REFERENTIAL_FILE` | empty | optional legacy trimmed `referential.tar.gz` (`refdata trim`) for offline seeding in standalone mode |
| `LEDGER_FILE` | `<ARCHIVE_DIR>/../uploaded.json` | standalone only: upload ledger of the in-process watcher (the desktop shell passes `<data>/uploaded.json`) |
| `PATCH_WEBHOOK_URL` | empty | outgoing new-patch notification |
| `AZURE_PUSH_URL` / `AZURE_PUSH_TOKEN` | empty | **dormant** (Twitch extension abandoned) |

**Stateless** server apart from the data directories: all config through env (V2-ready).
