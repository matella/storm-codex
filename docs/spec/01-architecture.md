# Architecture & invariants

## Overview

```
Gaming PC ──client-rs (Hots-Overlay)──► storm-codex-server (box :5102)
                POST /api/upload-raw          │ axum + Postgres + WS
                                              │
        ┌───────────────┬─────────────────────┼──────────────────┬──────────────┐
        ▼               ▼                     ▼                  ▼              ▼
   Rust SPA        OBS overlays         Jarvis Redis        HotsPatchNotes   raw archive
   (served by      (/widget /queue      (PUBLISH match      (:5001, source   (.StormReplay,
    the binary)     /draft/overlay…)     completed)          of dim_*)        source of truth)
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
- **Best-effort for everything peripheral**: Redis outage, HotsPatchNotes missing,
  dead webhook — the upload→parse→projection pipeline must never depend on them.
- **Single reference source**: heroes/talents/patches come from HotsPatchNotes (or its snapshot),
  replicated into `dim_*` at startup — no second sync pipeline.
- **`PARSER_VERSION`** (`main.rs`): bump on every projection change; it is what drives
  re-processing.

## Performance budgets (contracts, measured — not estimated)

| Budget | Contract | Last measurement |
|---|---|---|
| Full parse (decode + stats) | < 150 ms/replay | 133 ms median (`docs/research/2026-06-12-jalon2-parite.md`) |
| End of game → page updated | < 5 s | 1.4 s (`docs/research/2026-06-12-jalon3-bench.md`) |
| API p95 | < 100 ms | 52 ms (same); `GET /api/scouting/{id}`: 5.9 ms (25 requests, real 3-game report, 2026-09-26) |
| 3-year archive backfill (~2,800) | < 5 min | 1.8 min (same) |

Any PR that puts a budget at risk re-measures it (benches: `storm-stats-dump --bench`,
`backfill_bench.py`).

## Locked decisions (operator — do not reopen without them)

Rust (spike GO, .NET fallback dropped) · Postgres · Nexus Codex design · Dossier (evolution approved on 2026-09-27, spec `2026-09-27-design-dossier-app-design.md`) · replacement of the local Node
server · V1 = full SotS parity · no pre-game · no data migration (backfill) ·
**local overlay only** (Twitch/Azure extension abandoned, `azure.rs` dormant) ·
name Storm Codex. Details and rationale: `docs/specs/` (dated).
