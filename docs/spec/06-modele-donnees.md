# Data model — Postgres

Source of truth: `crates/storm-codex-server/migrations/` (embedded, run at startup
by `sqlx::migrate!`). Strategy: **JSONB for the lossless projection** (idempotent
re-process) + **promoted scalar columns** for the hot filter/sort axes (indexed).

## Core (0001, 0002, 0023)

| Table | Role | Key points |
|---|---|---|
| `upload_tokens` | personal upload tokens | `token_hash` = hex SHA-256 (never the plaintext); `revoked_at` |
| `uploads` | 1 row per received file (traceability) | `fingerprint` (content, UNIQUE), `archived_path` (see "Archived paths"), `status` ∈ pending/parsed/duplicate/parse_failed, `error_class`/`error_msg`, `parser_version`, `match_id` → matches (0002); **`parse_attempts`** (migration 0023, `INT NOT NULL DEFAULT 0`) = parses started by the upload path or the startup resume, incremented **before** each parse — a row the startup resume finds `pending` with `parse_attempts >= 3` is marked `parse_failed` / `error_class = too_many_attempts` instead of being re-parsed (see `04-serveur.md`, "Resume at startup") |
| `matches` | projection of a game | `fingerprint` (game, UNIQUE), promoted scalars (build, mode, map, length, played_at, winner, first_*), **`data` = complete storm-stats `match` object** (except the heroes in `takedowns`: played hero, see `04-serveur.md` § Played-hero correction), `parser_version` |
| `match_players` | 10 rows per match | promoted scalars (hero, team, win, kills…), **`data` = complete `player` object** — `hero` and the heroes copied into `deaths`/`takedowns` = played hero (talents), original in `heroCorrectedFrom` —, UNIQUE (match_id, toon_handle), CASCADE; **`hotkey_profile`** (migration 0015) = `SUserOptionsEvent.m_hotkeyProfile`, the player's hotkey profile name (index on `lower(btrim(…))`); NULL until the replay is re-processed (`PARSER_VERSION` 3); **`apm`** (migration 0016, `DOUBLE PRECISION`) = the player's SCmdEvents ÷ game minutes (`matches.length`), NULL until re-processed (`PARSER_VERSION` 4) or unknown duration; **`match_players_mvp_idx`** (migration 0018) = partial index `(match_id, toon_handle)` on the rows whose first award (`data #>> '{gameStats,awards,0}'`) is `EndOfMatchAwardMVPBoolean`: the archive's MVP filter reads it instead of detoasting `data` per game |
| `players` | aggregated player reference | `toon_handle` PK, `last_name`, `names` (JSONB aliases) — denormalised, best-effort, derivable from `match_players`. **Neither `last_name` nor the order of `names` is chronological** (UPSERT in processing order): the current name and the order of names come from the **dated history** `match_players.name` × `matches.played_at` (`players/names.rs`; NULL `played_at` = oldest) |
| `player_links` | "same person" decisions (0011) | `(toon_a, toon_b)` PK with `CHECK toon_a < toon_b` (ordered pair, one row per pair), `status` ∈ confirmed/dismissed, `decided_at`. Suggestions are computed on demand, never stored. No `CREATE EXTENSION` |

## Manual play sessions (0024)

`manual_sessions` stores a positive identity (negated at the API boundary), a UTC `started_at`, nullable
`ended_at` and nullable `name` (1–80 characters when set; the API trims whitespace). A partial unique
index allows only one row with no `ended_at`. No windows are deleted by the API.

`match_session_overrides` has one row per `match_id` (FK to `matches`, cascade on match deletion).
A missing row uses the time window; a row with `NULL` `manual_session_id` forces automatic grouping;
a non-null FK explicitly pins that match to a manual window. Reprocessing that retains the match ID
preserves the override. Neither operation rewrites replay data, the projection, or statistics.

## Manual definitions (0001, 0004)

`teams` (roster JSONB of ToonHandles + `league` text column — 0004), `leagues` (legacy,
unused: grouping is done by `teams.league`), `collections` (match_ids JSONB).
Recreated by hand in the UI — **never** derived from replays.

## Replicated reference data (0001, 0005, 0006, 0007, 0017) — owned by the `refdata` adapter

| Table | Contents | Join key |
|---|---|---|
| `dim_heroes` | 91 heroes (name, role, universe, data: dataset key, attributeId, icon, expandedRole…) | `id` = `name` = parser name (via `attributeId`) |
| `dim_talents` | ~1,940 talents | **`tree_id`** (0005) = dataset `talentId` (= the old `talentTreeId`), the same identifier as `player.talents[TierNChoice]` written by the parser |
| `dim_patches` | 260 patch notes, detail in `data.detail` | `internal_id` = dataset id; **`aliases`** (0017, `text[]`, GIN) = old HotsPatchNotes internalIds |
| `patch_hero_sections` | hero sections of patches (0007) | `hero_key` (normalised parser name) → hero ↔ patch links, BUFF/NERF/MIXED/REWORK/BUGFIX classification (`refdata::classify`); **`channel`** (0017) `live`/`ptr` |

These tables are **rebuildable** (one-transaction full refresh from a HotsPatchNotes dataset release, see
`04-serveur.md`) — no primary data. `app_settings.refdata_dataset_version` holds the `datasetVersion` last
ingested (a box round with the same version stops after the manifest).

Migration **0017** (`refdata_dataset`): empties `dim_patches` and `patch_hero_sections` (the patch identity becomes
the dataset id, so the first ingest is a first seed and sends no `patch.new`), adds `dim_patches.aliases`
(`text[] NOT NULL DEFAULT '{}'`, GIN index) and `patch_hero_sections.channel`, and deletes
`app_settings.reference_snapshot_sha256` (the standalone bundle is ingested again). Applied on the dev Postgres
(tests on a fresh database per test). Old ids in `aliases`: in release `data-d6776d73…`, 254 of the 300 distinct
old internalIds the old service listed resolve. The other 46 old `/patch/<id>` links answer "Patch not found": 19 old
PTR notes (the dataset exports only the current PTR cycle, by its own policy) and 27 Nexus-only patches (a source
the dataset may not use).

## Settings & draft (0003, 0008)

- `app_settings` (JSONB key/value): first use `operator_names` — the operator's
  accounts, to display THEIR perspective (session, matches, widget, Jarvis brief). `brief_voice`
  (`docs/specs/2026-11-21-butler-brief-full-fledged.md`): `"fr"` (default) / `"en"` / `"off"`,
  the voice of the Jarvis butler brief on `/queue` (`Off` disables the line entirely). No new
  migration — `app_settings` is a plain key/value table, a new settings key is just a new row.
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

## Daily goal tracker (0019)

`docs/specs/2026-11-20-streamer-widgets.md`, widget 7. `app_goal`: singleton (`id=1`, `CHECK id = 1`,
same pattern as `cast_live`/`draft_live`) — `kind` (`text`, `CHECK kind IN ('wins_today', 'games_today')`),
`target` (`int`, `CHECK target > 0`), `created_at`. Cleared by `DELETE` (no row = no active goal), not by
a nullable flag, so the table never holds a half-set goal. Progress itself (done count, reset instant) is
**not stored** — `GET /api/goal/progress` recomputes it from `matches`/`match_players` on every read
(`storm-codex-server::goal`), the same "no derived state" rule as the report status above.

## Overlay layouts (0020)

`overlay_layout` stores UUID, name, and UTC creation/update timestamps. `overlay_widget_instance`
uses its own UUID and a foreign key with `ON DELETE CASCADE`. The `(layout_id, z_order, id)` index
supports ordered reads and cascades. Constraints enforce the ten-type catalog, whole-cell 80×45
bounds and the per-widget JSONB settings allowlist (keys, types and the `me` length/control-character
limits; see `05-api.md`). The constraint also applies to direct backup restores. The API further
limits names, body size and widget count. Replacement is atomic and preserves the layout UUID. Layouts are user data,
not reconstructable from replays; back up both tables (see `09-operations.md`). No extensions required.

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
absolute path that exists **under `ARCHIVE_DIR`** is used as is; otherwise the file is looked up
by its name under the current `ARCHIVE_DIR` (`scouting/` kept, `/` or `\` separators). Nothing
resolves outside the archive: an absolute path elsewhere is looked up by name, and a name that is
empty, `.`, `..` or holds a `:` (drive prefix) resolves to the archive folder itself, which no
reader can open (a crafted row in an imported backup cannot point a reader at another file); so a backup restored on
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
