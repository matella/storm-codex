# Architecture & invariants

## Overview

```
Gaming PC ──client-rs (Hots-Overlay)──► storm-codex-server (box :5102)
                POST /api/upload-raw          │ axum + Postgres + WS
                                              │
        ┌───────────────┬─────────────────────┼──────────────────┬──────────────┐
        ▼               ▼                     ▼                  ▼              ▼
   Rust SPA        OBS overlays         Jarvis Redis        HotsPatchNotes   raw archive
   (served by      (/widget /queue      (PUBLISH match      dataset releases (.StormReplay,
    the binary)     /draft/overlay…)     completed)          → dim_*)         source of truth)
```

A **single binary** (`storm-codex-server`) serves the API, the built front-end and the overlays.
The `storm-replay` (decoding) and `storm-stats` (analysis) crates are pure libraries, publishable
independently (MIT).

### Standalone desktop app (Windows installer)

```
desktop/ (Tauri v2 shell, one instance, tray)
  ├─ supervises ─► bundled Postgres 17 (127.0.0.1:<pg port>, cluster in <data>/pg)
  ├─ supervises ─► storm-codex-server, STANDALONE=1 (127.0.0.1:<port>, in-process watcher)
  └─ window ─────► http://127.0.0.1:<port>/ (the same SPA, served by the server)
```

Same binary and same pages as the box; only the process supervision differs. `desktop/` is
**not** part of the Cargo workspace (own `Cargo.lock`, like `spike/*`); its process logic is in
`crates/storm-codex-supervisor` (workspace member) and `desktop/src/supervise.rs`. Operations:
[09-operations.md](09-operations.md) "Standalone desktop app".
Design: `docs/specs/2026-11-12-standalone-installer-design.md`.

## The 3 data tiers (hard rule no. 5)

1. **Raw archive** (`ARCHIVE_DIR`): every `.StormReplay` received is kept as is —
   **source of truth**. Never deleted by the pipeline.
2. **Postgres**: **complete** projection of the parse (`matches.data` + `match_players.data` JSONB,
   scalar columns promoted for filters). `parser_version` everywhere → **idempotent** re-process
   (delete-then-insert by fingerprint). The database can be rebuilt from the archive.
3. **Dump decoded on the fly** (`GET /api/matches/{id}/raw?stream=…`) + **bounded LRU disk cache**
   (`RAW_CACHE_MAX_BYTES`). **Never any mass pre-decoding.**

## Invariants (do not break)

- **Parity proven, not claimed**: any gap between storm-stats and hots-parser is a documented
  tolerance (`tools/parity-harness/tolerances.json`, [03-storm-stats.md](03-storm-stats.md)).
- **Idempotence**: re-uploading/re-processing a replay never creates a duplicate
  (two-level fingerprints, see [04-serveur.md](04-serveur.md)).
- **Jarvis spine events**: `schema_version`, `correlation_id`/`causation_id`,
  `occurred_at`/`recorded_at`, past-tense `entity.verb` type ([07-evenements.md](07-evenements.md)).
- **Best-effort for everything peripheral**: Redis outage, HotsPatchNotes release unreachable,
  dead webhook — the upload→parse→projection pipeline must never depend on them.
- **No live third-party requests from stored content**: patch-note HTML never embeds a remote
  image URL. `refdata::patchimg` rewrites any Blizzard CDN `<img src>` to a local
  `/images/patch-images/…` path at dataset build time (box and standalone alike); only the box
  fetches the bytes once (spec `docs/specs/2026-11-19-patch-image-proxy.md`, P1).
- **Single reference source**: heroes/talents/maps/patches come from the HotsPatchNotes **dataset**
  (schema v1, GitHub releases) through the single `refdata` adapter, replicated into `dim_*`. Box:
  latest validated release, SHA-256 verified, refreshed every 24 h. Standalone checks the same stable
  feed independently of startup, replay import and application updates. It activates only after its
  required images are staged and verified; offline startup uses cached data or replay-derived fallbacks.
  The standard installer does not include the reference snapshot; legacy local bundles remain supported
  ([approved download spec](../specs/2026-11-19-standalone-reference-download.md)).
- **`PARSER_VERSION`** (`main.rs`): bump on every projection change; it is what drives
  re-processing.

## Performance budgets (contracts, measured — not estimated)

| Budget | Contract | Last measurement |
|---|---|---|
| Full parse (decode + stats) | < 150 ms/replay | 133 ms median (`docs/research/2026-06-12-jalon2-parite.md`) |
| End of game → page updated | < 5 s | 1.4 s (`docs/research/2026-06-12-jalon3-bench.md`) |
| API p95 | < 100 ms | 52 ms (same); `GET /api/scouting/{id}`: 5.9 ms (25 requests, real 3-game report, 2026-09-26) |
| Standalone reference status (`GET /api/standalone/reference`) | API p95 < 100 ms | 0.74 ms (200 warm loopback requests, Linux debug server, clean DB after first sync, 2026-11-19) |
| 3-year archive backfill (~2,800) | < 5 min | 1.8 min (same) |
| Local insights (`/api/records`, `/api/maps/early-leads`, `/api/matches/{id}/insights`, `/api/compare`) | API p95 < 100 ms | warm: 35 / 6 / 4 / 12 ms (copy of the box DB, 3,480 games, release, 2026-11-18) |
| Archive list `GET /api/matches` (page of 100 + `X-Total-Count`), replay search filters | API p95 < 100 ms; 100-row pages, p95 <= 65 ms accepted (operator, 2026-11-19), worst measured 61 ms | Release HTTP p95: 70.3 ms unfiltered, 67.3 ms map + mode, 76.1 ms Braxis + deaths, 75.3 ms teammate + death bound (200 of 209 matches), 79.7 ms all typed bounds, 63.7 ms at `offset=3000`; 19.9 ms for `limit=50` (these were the 200-row measurements superseded by the accepted 100-row page; 61 ms worst case, all typed bounds). Warm loopback, 40 sequential requests per query, synthetic archive of 3,500 games / 35,000 players with actual storm-stats JSON, PostgreSQL 17, shared host, 2026-11-19. The same-fixture SQL probe reduced unfiltered p95 from ~77 ms (old page only) to ~63 ms (page + count); a diagnostic without award extraction was 8.5 ms. Box copy not measured. |
| Local insights: `/api/players/{toon}/insights` | API p95 < 100 ms — **at the limit, operator to accept or ask for more work** | 75–100 ms (same setup; account with 2,684 games) |
| Session review (`/api/sessions`, `/api/sessions/{id}`) | API p95 < 100 ms (spec target < 20 ms) | warm, sequential loopback: list p50 6.4 / p95 6.7 ms (300 requests, `limit` 20 and 100); review p50 6.4 / p95 6.7 ms (300 requests over the 100 latest sessions, up to 19 games each) (copy of the box DB, 3,492 games, release, 2026-11-19; idle, not during import) |
| Insights corpus (in memory) | load in the background at startup; RAM documented | 3.3 s load, +44 MB RSS for 3,480 games (~13 kB/game) (same setup) |
| Overlay layout API (`overlay_layout.rs`) | API p95 < 100 ms | public detail 4.99 ms, admin list 1.37 ms, admin replace 15.07 ms (100 warm sequential loopback HTTP requests each after 10 warm-ups, Linux debug server, disposable PostgreSQL 17 with one synthetic layout of 100 widgets, 2026-11-20; no concurrent import) |
| `GET /api/goal/progress` (daily goal tracker, widget 7) | API p95 < 100 ms | 1.0 ms avg, 1.5 ms worst (25 warm loopback requests, debug server, dev Postgres with a test-fixture archive, 2026-11-20); pure, recomputed on every read — no corpus, one small join by `played_at` + `toon_handle`/`name` |

Any PR that puts a budget at risk re-measures it (benches: `storm-stats-dump --bench`,
`backfill_bench.py`).

## Locked decisions (operator — do not reopen without them)

Rust (spike GO, .NET fallback dropped) · Postgres · Nexus Codex design · Dossier (evolution approved on 2026-09-27, spec `2026-09-27-design-dossier-app-design.md`) · replacement of the local Node
server · V1 = full SotS parity · no pre-game · no data migration (backfill) ·
**local overlay only** (Twitch/Azure extension abandoned, `azure.rs` dormant) ·
name Storm Codex. Details and rationale: `docs/specs/` (dated).
