# Data model — Postgres

Source of truth: `crates/storm-codex-server/migrations/` (embedded, run at startup
by `sqlx::migrate!`). Strategy: **JSONB for the lossless projection** (idempotent
re-process) + **promoted scalar columns** for the hot filter/sort axes (indexed).

## Core (0001, 0002)

| Table | Role | Key points |
|---|---|---|
| `upload_tokens` | personal upload tokens | `token_hash` = hex SHA-256 (never the plaintext); `revoked_at` |
| `uploads` | 1 row per received file (traceability) | `fingerprint` (content, UNIQUE), `archived_path` (see "Archived paths"), `status` ∈ pending/parsed/duplicate/parse_failed, `error_class`/`error_msg`, `parser_version`, `match_id` → matches (0002) |
| `matches` | projection of a game | `fingerprint` (game, UNIQUE), promoted scalars (build, mode, map, length, played_at, winner, first_*), **`data` = complete storm-stats `match` object** (except the heroes in `takedowns`: played hero, see `04-serveur.md` § Played-hero correction), `parser_version` |
| `match_players` | 10 rows per match | promoted scalars (hero, team, win, kills…), **`data` = complete `player` object** — `hero` and the heroes copied into `deaths`/`takedowns` = played hero (talents), original in `heroCorrectedFrom` —, UNIQUE (match_id, toon_handle), CASCADE; **`hotkey_profile`** (migration 0015) = `SUserOptionsEvent.m_hotkeyProfile`, the player's hotkey profile name (index on `lower(btrim(…))`); NULL until the replay is re-processed (`PARSER_VERSION` 3); **`apm`** (migration 0016, `DOUBLE PRECISION`) = the player's SCmdEvents ÷ game minutes (`matches.length`), NULL until re-processed (`PARSER_VERSION` 4) or unknown duration |
| `players` | aggregated player reference | `toon_handle` PK, `last_name`, `names` (JSONB aliases) — denormalised, best-effort, derivable from `match_players`. **Neither `last_name` nor the order of `names` is chronological** (UPSERT in processing order): the current name and the order of names come from the **dated history** `match_players.name` × `matches.played_at` (`players/names.rs`; NULL `played_at` = oldest) |
| `player_links` | "same person" decisions (0011) | `(toon_a, toon_b)` PK with `CHECK toon_a < toon_b` (ordered pair, one row per pair), `status` ∈ confirmed/dismissed, `decided_at`. Suggestions are computed on demand, never stored. No `CREATE EXTENSION` |

## Manual definitions (0001, 0004)

`teams` (roster JSONB of ToonHandles + `league` text column — 0004), `leagues` (legacy,
unused: grouping is done by `teams.league`), `collections` (match_ids JSONB).
Recreated by hand in the UI — **never** derived from replays.

## Replicated reference data (0001, 0005, 0006, 0007) — owned by the `dim.rs` job

| Table | Contents | Join key |
|---|---|---|
| `dim_heroes` | 90 heroes (name, role, universe, data) | internal name |
| `dim_talents` | ~1,900 talents | **`tree_id`** (0005) = `talentTreeId`, the same identifier as `player.talents[TierNChoice]` written by the parser |
| `dim_patches` | list of patch notes | `internal_id`; detail proxied on demand |
| `patch_hero_sections` | hero sections of patches (0007) | `hero_key` (normalised name) → hero ↔ patch links, BUFF/NERF/MIXED classification |

These tables are **rebuildable** (full refresh at startup/24 h) — no primary data.

## Settings & draft (0003, 0008)

- `app_settings` (JSONB key/value): first use `operator_names` — the operator's
  accounts, to display THEIR perspective (session, matches, widget, Jarvis brief).
- `draft_live`: singleton (id=1, CHECK) — the whole draft simulator state as JSONB
  (config, picks/bans, fearless history), timestamped.

## Cast control room (0012)

- `cast_live`: singleton (id=1, CHECK) — the whole cast series (teams, map pool, games,
  scene, slideshow) as JSONB, timestamped; `schema_version` in the JSON, missing fields → defaults.
  `series_id` (uuid string, `""` before the first series): identity set by `POST /api/cast/series`, re-checked
  by the replay upload after decoding (a series replaced in the meantime → `409 series_changed`).
  `games[].summary` (`GameSummary`, set by `POST /api/cast/games/{n}/replay`, never re-decoded):
  `{duration_secs, winner, takedowns:[b,r], team_level, structures, merc_camps, objectives (each [b,r] or null),
  bans:[[heroes],[heroes]], players:[PlayerSummary], xp_lead:[[s, Blue lead]]}`;
  `PlayerSummary = {side, toon, name, hero, kills, takedowns, deaths, assists, hero_damage, siege_damage, healing}`
  (storm-stats paths: [03-storm-stats.md](03-storm-stats.md) "Reading guide"). A `summary` that is unreadable
  when re-read becomes `null` (the series stays loadable).
- `cast_logo` (0013): one logo per side — `side` PK (`blue`|`red`, CHECK), `mime`, `bytes` BYTEA (≤ 512 KB),
  `updated_at` (used as the `?v=` version in `Team.logo`).

## Scouting (0010)

Scouting reports (spec `docs/specs/2026-09-26-rapports-scouting-design.md`). **Isolated** from the
personal stats tables: an opponent's replay is never projected into `matches`.

| Table | Role | Key points |
|---|---|---|
| `scouting_reports` | a report | `title`, `target_name`, `roster`/`anchors` (JSONB of toon_handles; roster `[]` = auto detection), `facts` = snapshot `{facts, detection, roster, roster_auto}` recomputed on every mutation, `facts_version`, `analysis` (normalised JSON, only one, overwritable) + `analysis_facts_version` / `analysis_model` / `analysis_imported_at` |
| `scouting_games` | one replay of a report | UNIQUE `(report_id, fingerprint)`, `file_sha256`, `archived_path` (`ARCHIVE_DIR/scouting/`, see "Archived paths"), `target_team` + `target_source` (`roster`/`anchor`/`manual`), **`summary`** = compact summary (the only input to the facts computation), `data` = complete storm-stats output, `parser_version`; CASCADE from the report |

A report's status (`empty`/`ready`/`analyzed`/`stale`) is **derived**, never stored.

### Archived paths

`uploads.archived_path` and `scouting_games.archived_path` are written **relative to
`ARCHIVE_DIR`**: `<sha256>.StormReplay` and `scouting/<sha256>.StormReplay`
(`Config::archive_entry` / `scouting_entry`). Rows written before (box history, backups made
on another PC) hold an absolute path. Every reader goes through `Config::archived_file`: an
absolute path that exists is used as is; otherwise the file is looked up by its name under the
current `ARCHIVE_DIR` (`scouting/` kept, `/` or `\` separators), so a backup restored on
another PC or Windows user profile keeps working without rewriting rows (no migration).

## Conventions for any new migration

1. Sequential `NNNN_description.sql` file — **never** modify an applied migration
   (checked in CI: `scripts/check-migrations.sh` rejects any change to a migration already
   present on the target branch). Line
   endings forced to LF (`.gitattributes`): sqlx checks the checksum of the exact bytes, a
   Windows checkout with CRLF gave `VersionMismatch` on a database migrated with LF.
2. Large structure → JSONB; filter/sort axis → promoted column + index.
3. If the projection changes shape: bump `PARSER_VERSION` (main.rs) → `POST /api/admin/reprocess`.
4. Update **this file** in the same commit.
