# Milestone 3 — `storm-codex-server`: upload + Postgres + workers + WS + backfill

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. **Context7 MCP mandatory**
> before using axum / sqlx / tokio / tower (moving versions) — `resolve-library-id`
> then `query-docs`. Do not code these APIs from memory.

**Goal:** a Rust binary `storm-codex-server` (axum + Postgres) that receives replays
(`POST /api/upload`, token), archives them, parses them in a pool (storm-replay→storm-stats), projects
into Postgres (`parser_version` everywhere, idempotent re-process), pushes `match.parsed` over WebSocket,
serves the REST read endpoints, the `…/raw` dump (LRU cache), and the admin; plus the
**backfill** mode of `client-rs`.

**Accept (spec):** 100 % of the archive archived and attempted; **≥ 99 % parsed** (failures listed and
classified in Admin); **end of game → page up to date < 5 s**. Budgets: PG write + WS push
< 200 ms; 3-year backfill (~3,000 replays) < 5 min (workers = number of cores).

**Architecture:** existing workspace (storm-replay, storm-stats). New binary crate
`crates/storm-codex-server`. **Dev = local Docker Postgres** (`docker compose` in the repo);
box deployment later. sqlx (compile-time checked queries, migrations). client-rs cloned:
`C:\Users\matth\Desktop\Coding\Hots-Overlay\client-rs`.

**3 tiers (spec reminder):** raw archive (files, source of truth); Postgres (complete
projection); dump decoded on demand + bounded LRU cache. **Never any mass pre-decoding.**

---

### Task 1: dev Docker Postgres + server scaffold + /api/health
- `docker-compose.dev.yml`: Postgres 17 (port 5433 so as not to clash with an existing PG), volume,
  `POSTGRES_*`; `.env.example` (DATABASE_URL, BIND_ADDR, ARCHIVE_DIR, RAW_CACHE_MAX_BYTES).
- `crates/storm-codex-server`: axum, tokio, sqlx (runtime tokio + postgres + macros + migrate),
  tower-http (trace), thiserror, serde. `main.rs`: config from env, sqlx pool, `/api/health`
  (returns `{status:"ok", parser_version, db:"up"}`).
- **Context7** axum (router, State, handlers) + sqlx (PgPool, migrate) before coding.
- Test: `docker compose -f docker-compose.dev.yml up -d`, `cargo run -p storm-codex-server`,
  `curl /api/health` → 200. Commit.

### Task 2: DB schema (sqlx migrations) + parser_version
- `migrations/0001_init.sql`: `upload_tokens` (id, name, token_hash, created_at, revoked);
  `uploads` (id, token_id, filename, fingerprint UNIQUE, archived_path, status
  enum[pending|parsed|duplicate|parse_failed], error_class, error_msg, parser_version,
  replay_version_build, created_at, parsed_at); `matches` (id, fingerprint UNIQUE, build, mode,
  map, duration_loops, length, date, winner, first* , patch_codex, parser_version, created_at);
  `match_players` (match_id, slot, toon_handle, hero, team, win, score JSONB, talents JSONB,
  awards JSONB, hero_level, …); `talents`, `draft` (picks/bans JSONB or rows);
  `timeline_events` (match_id, kind, payload JSONB) — takedowns/objectives/structures/xp/
  teamfights/messages/votes; `players` (toon_handle PK, names JSONB/alias, tags);
  `teams`/`leagues`/`collections` (rosters); `dim_heroes`/`dim_talents` (reference data).
  Indexes on match(map,mode,date,build), match_players(toon_handle,hero,match_id).
- JSONB vs columns decision: complete score screen in JSONB `score` (80 stats — avoid 80
  columns), + promoted columns for the hot filter/sort axes (hero, team, win, kills,
  deaths, hero_dmg…). Document in the SQL.
- Test: migration applies, `\d` consistent. Commit.

### Task 3: storm-stats projection → Postgres rows
- `project.rs` module: `MatchStats` (storm-stats output `{match,players}`) → typed structs →
  idempotent transactional INSERT (UPSERT by fingerprint; a re-process deletes/replaces the
  match's rows and re-inserts, driven by `parser_version`). The fingerprint = same formula as
  the existing one/HeroesProfile (MD5 BlizzIDs+random) — check the formula in client-rs/HeroesProfile.
- storm-stats exposes what is needed (add typed accessors if needed rather than reparsing
  the JSON). Idempotence: re-inserting the same replay does not create a duplicate, updates if
  `parser_version` change.
- Integration test: project 3 replays of the mini-corpus → expected rows (10 match_players,
  draft, non-empty timeline). Commit.

### Task 4: upload pipeline + worker pool + response semantics
- `POST /api/upload` (Bearer token validated against `upload_tokens`): reads the file, computes the
  fingerprint, **archives first** (ARCHIVE_DIR), inserts `uploads(pending)`; if the fingerprint is
  already `parsed` → immediate `duplicate` response. Otherwise queues a job in the **tokio pool**
  (size = number of cores, never on the HTTP thread): parse storm-replay→storm-stats→project, sets
  `uploads` to `parsed`/`parse_failed`(+typed error_class from `storm_replay::Error` / panic
  isolated via `catch_unwind`). **Semantics**: the request waits for the result up to 2 s (typically
  < 0.5 s) and returns the final status; saturated pool → `202 {status:"accepted"}`. No failure
  blocks the queue.
- **Context7** tokio (mpsc/Semaphore/spawn_blocking for the CPU parse) + axum (extractors, body).
- Tests: upload OK → parsed; re-upload → duplicate; corrupted file → classified parse_failed;
  budget < 200 ms excluding parse. Commit.

### Task 5: WebSocket `/ws` + `match.parsed` push (< 5 s criterion)
- `/ws` (axum ws + tokio broadcast): at the end of a successful parse, broadcasts
  `{type:"match.parsed", match_id, map, …}`. Backfill: periodic progress.
- Test: WS client receives the event < 200 ms after parse; end-to-end upload→event < 5 s.
  **Context7** axum WebSocket + tokio::sync::broadcast. Commit.

### Task 6: REST read endpoints (the API the front-end will consume in milestone 4)
- `GET /api/matches` (map/hero/player/date/mode/patch filters, pagination); `/api/matches/{id}`
  (10-player score, draft, timeline); `/api/players/{toon}` (summary, hero pool);
  `/api/heroes`, `/api/heroes/{h}`; minimal trends/rankings; CSV/JSON exports.
  Views/materialised views for hot aggregates. **Budget p95 < 100 ms** (measure on the
  backfilled archive). Commit (may be split into several).

### Task 7: `…/raw` dump + bounded LRU cache
- `GET /api/matches/{id}/raw?stream=…`: decodes the archived file on the fly (storm-replay),
  compressed disk cache **bounded LRU** (`RAW_CACHE_MAX_BYTES`, default 5 GB). Never any
  mass pre-decoding. Test: 2 requests → 2nd served from the cache; eviction when the cap is reached.
  Commit.

### Task 8: admin (tokens, re-process, backfill, health)
- `POST /api/admin/tokens` (create/revoke); `POST /api/admin/reprocess` (all or by filter,
  idempotent, follows `parser_version`); `GET /api/admin/uploads` (state/health, classified failures);
  backfill state. Simple admin auth (admin token from env). Commit.

### Task 9: client-rs — re-pointing + backfill mode + latency
- In `Hots-Overlay/client-rs`: re-point to `/api/upload` (instead of `/api/upload-raw`);
  **backfill mode** (full scan of the archive, throttled upload, resume after interruption, tray
  progress bar); lower the file stabilisation (~5 s → ~1 s). Build and test
  against the local server. Commit in the Hots-Overlay repo (separate).

### Task 10: real backfill + acceptance criterion + bench + STATUS + push
- Backfill of the complete local archive (2,821 replays) to the server (Docker Postgres).
  **Check: 100 % archived and attempted; ≥ 99 % parsed** (failures listed/classified in Admin —
  storm-stats's "unknown map" rejections count as attempted/classified, not as loss);
  backfill < 5 min; API p95 < 100 ms. Report `docs/research/2026-06-12-jalon3-bench.md`,
  STATUS, commit + **push**.

---

## Known pitfalls
- **Context7 first** for axum 0.7+/sqlx 0.8+ (extractors, State, ws, migrate, PgPool) — moving
  APIs, do not code from memory (CLAUDE.md rule).
- Parse = CPU-bound: `spawn_blocking` or a dedicated pool, never on the HTTP runtime.
- `parser_version`: bump when storm-stats changes → the re-process recomputes.
- Fingerprint: reuse the exact formula of the existing one (cross-source anti-duplicate) — read it
  in client-rs/uploader.rs and HeroesProfile, do not invent.
- Docker Postgres on 5433 (a PG may already run on 5432).
- storm-stats rejects maps outside `MapType` (status -2): on the server side, it is a
  `parse_failed(error_class="unsupported_map")` **attempted and classified**, not a loss — the
  "≥ 99 % parsed" criterion must take these known rejections into account (to document; lifting the map table is
  post-parity work, see milestone 2 report).
- Re-process idempotence: transaction, delete-then-insert by fingerprint.
