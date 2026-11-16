# Reference dataset adapter — design

Status: **approved by the operator, 2026-11-15.** Date: 2026-11-15.

## Intent

Storm Codex reads its hero, talent, map and patch-note reference data from HotsPatchNotes. That project
(`matella/HeroesOfTheStormPatchNotes`) was rewritten (its PRs #2 to #5, merged 2026-11-12). It no longer has a web
API. It is now a Rust tool that publishes a **versioned dataset** as GitHub releases every night. Its README says the
new contract "intentionally breaks the former HotsPatchNotes API/snapshot contract", and that Storm Codex adaptation is
separate work. This spec is that work.

What breaks today:

- **Box:** `dim::sync_heroes`, `sync_talents`, `sync_patches`, `vendor_images`, `backfill_hero_sections` and the
  `/api/patches/{id}` live proxy (`read::hpn_proxy`) call `/api/heroes`, `/api/heroes/{shortName}`, `/api/patches`,
  `/api/patches/{id}` and `/api/battlegrounds`. They only work while the old .NET service still runs on the box, and
  that service will get no new data. The rewrite removed its code, but did not stop the service already deployed.
- **Standalone installer (PR #32):** `scripts/make-referential.sh` builds `referential.tar.gz` from that same API. No
  snapshot can be built once the old service is gone, and the installer cannot ship without one
  (`desktop/referential-pin.txt` is still a placeholder).

This spec changes **hard rule 7** of `gads/PROJECT.md`: "The hero/talent reference data comes from the HotsPatchNotes API
(replicated into `dim_*` at startup) — no second sync pipeline". The new wording is in "Rule changes" below.

## Goals

1. Both deployments (box and standalone) read the new dataset, **schema version 1**, through one adapter. There is no
   second sync pipeline.
2. The public HTTP contract of Storm Codex does not change. These keep the same JSON shape, so the Rust front-end does
   not change:
   - `/api/dim/heroes`
   - `/api/dim/talents`
   - `/api/patches`
   - `/api/patches/{id}`
   - `/api/hero-changes*`
   - `/api/hero/{hero}/patches`
3. Every hero, talent, map and patch the app shows today is still shown, with the same join keys for the parser's data.
4. Downloads are verified: SHA-256 from the release, with the schema version checked before anything is written.
5. The standalone installer bundles only what the app uses, not the 97 MB `dataset.tar.gz`.
6. Old patch URLs (`/patch/<old internalId>`) keep working.

## Non-goals

- No change to HotsPatchNotes itself. The adapter consumes its contract as published.
- No new UI feature from the richer dataset: abilities, the 12 locales, unit stats, map objectives. These become
  possible later, but are not part of this spec.
- No runtime download in standalone mode. The installed app stays offline except for update checks. Its reference data
  is refreshed by app updates, as decided in the installer design.
- No translation work: the app stays in English (`enus`).

## What we checked (dataset release `data-c8d8ff8f…`, 2026-11-12)

The manifest reads `schemaVersion` 1, `gameVersion` 2.57.0.98348, with 91 heroes, 35 battlegrounds and 276 patches
(26 of them `metadataOnly`). Release assets:

- `manifest.json`
- `game.json` (8 MB)
- `patches.json` (19 MB)
- `assets.json`
- `{locale}.json` (3 MB each)
- the schemas
- `dataset.tar.gz` (97 MB: the files above plus `images/`)
- `dataset.tar.gz.sha256`

| Storm Codex needs | Old API | New dataset | Fit |
|---|---|---|---|
| Talent join key (parser `TierNChoice`) | `talentTreeId` | `game.heroes.<key>.structure.talents.LevelN[].talentId` | **Same values.** 1,477 of the 1,488 IDs in our replay corpus match. The 11 that do not are talents removed or reworked since (e.g. `ArthasDeathsAdvance`, `TyrandeShootingStar`); the old API did not have them either. |
| Talent name, tier, icon, sort, quest flag | detail `talents` | name via `enus.talents[linkId].name.text`; tier from the `LevelN` key; `icon`, `sort`, `isQuest` on the native record | Same data; `linkId` is the key into the locale. |
| Talent description, type | `description`, `type` | `enus.talents[linkId].fullText.text`, `abilityType` | Same data. |
| Hero canonical name (`dim_heroes.id`, the join with `match_players.hero`) | HPN `name`, or `attributeId` mapped through `attr.json` for talents | `game.heroes.<key>.attributeId` (e.g. `Amaz`) mapped through `storm_stats::constants::hero_attribute()` | Same mechanism, now also used for heroes. |
| Hero display name | `name` | `enus.heroes.<key>.name.text` (e.g. "Cassia", "E.T.C.") | OK. |
| Role | `role`, `data.expandedRole` | `enus.heroes.<key>.expandedRole.text` ("Ranged Assassin"…), `roles` | OK. |
| Universe (portrait ring colours) | `universe`: `Warcraft`, `StarCraft`, `Diablo`, `Overwatch`, `Nexus` | `structure.franchise`: `Warcraft`, `Starcraft`, `Diablo`, `Overwatch`, `Classic` | **Values differ:** `Starcraft` → `StarCraft`, `Classic` → `Nexus`. |
| Hero icon (`/images/heroes/<slug>.png`, square) | `icon` | Only the **loading-screen portraits** are bundled (`heroportraits/`, tall art). The square `heroSelect` icon is named in `structure.portraits` but not bundled. | **Gap.** See decision D3. |
| Map art (`/images/battlegrounds/<slug>.png`, slug from the English map name) | `imageUrl` | `game.battlegrounds.<id>.structure.loadingScreenImage` → `assets.json` (`loadingscreens/`, 29 files for 35 maps) | OK. Copied under the slug the front already uses. Maps without art keep the front's fallback, as today. |
| Patch list (`internalId`, `patchName`, `patchType`, `liveDate`, `officialLink`, `hasContent`, `heroCount`, `mapCount`) | `/api/patches` | `patches.json[]`: `id` (`live:<build>`, or `source:<alias>` when the build is unknown), `name`, `patchType`, `date`, `channel`, `availability`, `sources[]`, `aliases[]` | Counts are derived from the sections, `officialLink` from the first `official` source, `hasContent` = `availability == "available"`. |
| Old patch IDs | `internalId` (e.g. `hotfix-patch-notes-march-25-2014`) | `aliases[]` holds them | Used for the redirect (goal 6). |
| Patch detail (`content`, `tableOfContents`, `sections[{sectionType, anchor, classification…}]`) | `/api/patches/{id}` | `contents.enus.{html, text, sections[{id, heading, level, html, text, heroId, battlegroundId}]}` | Built by the adapter in the old shape (D2). |
| Hero sections (`patch_hero_sections`) | `sectionType == "Hero"`, `entityName`, `heroShortName`, `anchor`, `classification`, `shortSummary`, `content` | sections with `heroId` (2,174 sections in total) | `classification` and `shortSummary` are **gone** (D2). |
| PTR notes | — | `channel: "ptr"`, only the newest cycle, marked `candidate` | Imported, with `patchType` kept and the PTR label shown as today. |
| Community notes | — | `sources[].authority: "community"` | Must not be presented as official (dataset rule): see "Contract rules we inherit". |

## Decisions to approve

**D1. Where the adapter lives — in storm-codex, as a pure module that reads the dataset.**
- A new module, `crates/storm-codex-server/src/refdata/`, parses dataset schema v1 into the rows of `dim_heroes`,
  `dim_talents`, `dim_patches`, `patch_hero_sections`, plus the list of images to copy. It does no I/O and is
  unit-tested on fixtures cut from a real release.
- Two thin loaders use it:
  - **box**: downloads a release;
  - **standalone**: reads the bundled trimmed file.
- Alternative rejected: asking HotsPatchNotes to also publish a Storm-Codex-shaped export. Its ADR keeps consumer
  adapters out of the producer.

**D2. Classification and short summary — port the old classifier into storm-codex.**
- The old buff/nerf/rework/bugfix verdict and its one-line summary ("Q damage 96 → 110 (+15%)") came from
  `PatchClassifier.cs`: 81 lines of pure regex heuristics, unit-tested.
- It is ported to Rust (`refdata::classify`) and run on each hero section's `text` at ingest.
- The `/hero-changes` filters and badges keep working. The labels stay heuristic, as before.
- The patch detail is rebuilt in the old shape, so the front is unchanged:
  - `sectionType` = `Hero` when `heroId`, `Battleground` when `battlegroundId`, else `General`;
  - `anchor` = section `id`;
  - `tableOfContents` from `heading`/`level`;
  - `content` = the article `html`.
- Alternative: drop the classification. That removes the filter and the badges from `/hero-changes`, a visible
  regression.

**D3. Square hero icons — ask HotsPatchNotes to bundle the `heroSelect` portraits.** Recommended.
- The front uses a square portrait everywhere: rings, tables, the overlay. The dataset names it
  (`structure.portraits.heroSelect`) but does not ship it.
- Options:
  - (a) **Recommended:** add `heroSelect` to the bundled core assets in HotsPatchNotes. This is a producer change in
    its own repo and issue. It is about 91 small files.
  - (b) Crop the loading portrait at ingest. This needs an image crate, and the result is worse.
  - (c) Keep the square icons vendored once by the old API and stop refreshing them. New heroes would then show
    initials.
- Until (a) lands, (c) is the fallback, because the image files already on the box are kept.

**D4. Box source — the latest release, verified, refreshed every 24 h.**
- `REFDATA_URL` (default
  `https://github.com/matella/HeroesOfTheStormPatchNotes/releases/latest/download/`) replaces both
  `HOTSPATCHNOTES_URL` and `REFERENTIAL_URL`.
- Each round:
  1. Read `manifest.json` and refuse a `schemaVersion` other than 1 (logged, previous data kept).
  2. Skip the round when `datasetVersion` equals the last ingested one (`app_settings`).
  3. Otherwise download `game.json`, `enus.json`, `patches.json` and `assets.json` plus the needed images, and check
     each against the SHA-256 the manifest lists.
- `latest` is acceptable on the box because the producer only promotes a release after it is validated.
- Alternative: pin a release in `.env`. That is safer, but needs a manual bump per patch. Rejected for the box, used by
  the installer (D5).

**D5. Standalone bundle — a trimmed snapshot built from a pinned dataset release.**
- `scripts/make-referential.sh` is replaced by a server subcommand:
  `storm-codex-server refdata trim --from <dataset.tar.gz> --out referential.tar.gz`.
- It verifies `dataset.tar.gz.sha256` and keeps only:
  - `manifest.json`, `game.json`, `patches.json`, `assets.json`;
  - `locales/enus.json`;
  - the hero portraits and the map art the app uses.
- `desktop/referential-pin.txt` then pins **the dataset release** (URL and SHA-256 of `dataset.tar.gz`), and the release
  workflow runs the trim. The installer stays offline at runtime.
- The talent and ability icons (`abilitytalents/`, 1,110 files, about 25 MB) are not bundled, because the app does not
  show them today.
- Estimated bundle: about 30 MB of JSON (it compresses well) plus the images. The final number is to be measured, not
  guessed.

**D6. Patch identity and old URLs.**
- `dim_patches.internal_id` becomes the dataset `id`, and `aliases` are stored with the row. `GET /api/patches/{id}`
  and the `/patch/<id>` page accept an alias and answer with the canonical patch, so old links and stored
  notifications still work.
- The switch is one migration: it empties `dim_patches` and `patch_hero_sections`, so the first ingest is a "first seed"
  and sends no "new patch" notification (the existing `was_empty` rule).
- `patch.new` events and the webhook keep their payload. `internalId` is the new ID.

**D7. Remove the old paths.** The following go in the same change:
- `dim::sync_*`, `vendor_images`, `backfill_hero_sections`, `ingest_snapshot` (old layout), `read::hpn_proxy`;
- `HOTSPATCHNOTES_URL`, `REFERENTIAL_URL`;
- `scripts/make-referential.sh`, `scripts/fetch-referential.ps1`. The new pin is fetched by the release workflow.

There is no period where both sources run: one pipeline, as rule 7 requires.

## Contract rules we inherit from the dataset (README, schema v1)

- **Identity:** hero keys and battleground `normalizedId` are identifiers; display names are not. Storm Codex keeps its
  own join key (the parser name via `attributeId`) and stores the dataset key in `data`.
- **Missing text stays missing:** a missing English name falls back to the dataset key, never to another locale.
- **`metadataOnly` patches:** listed, with `hasContent: false` and the dataset `reason` in the detail. They are not
  empty patches.
- **PTR:** a PTR entry is a `candidate`. It is labelled PTR and never counted as a live change in `/hero-changes`
  (`channel == "ptr"` excluded there).
- **Community notes:** a patch whose sources are only `community` is shown with a "Community notes" label, never as
  official.
- **HTML:** the dataset ships sanitized HTML. The front still renders it only through `SafeHtml` (`core::sanitize`
  allow-lists), as today, so no trust is added.

## Rule changes (applied with the implementation, after approval)

`gads/PROJECT.md`, hard rule 7, becomes: "The hero/talent/map/patch reference data comes from the HotsPatchNotes
**dataset** (schema v1, GitHub releases), through the single `refdata` adapter, replicated into `dim_*`. Box: latest
validated release, verified, refreshed every 24 h. Standalone: a trimmed snapshot of a pinned release, bundled in the
installer. No second sync pipeline." The environment section loses the HotsPatchNotes API `:5001`/`:5100` from Storm
Codex's needs.

Specs updated in the same commit as the code (rule 0):
- `docs/spec/04-serveur.md`: reference data, configuration;
- `docs/spec/05-api.md`: patch alias;
- `docs/spec/06-modele-donnees.md`: `dim_patches.aliases`, the migration;
- `docs/spec/09-operations.md`: env, release pin, trim step;
- `docs/spec/01-architecture.md`: source of the reference data.

## Acceptance criteria

1. Ingesting the release `data-c8d8ff8f…` fills these tables:
   - `dim_heroes` with 91 rows, every universe in the front's set;
   - `dim_talents` with every `talentId`;
   - `dim_patches` with 276 rows;
   - `patch_hero_sections` with one row per hero section of available `enus` content.
   Counts are checked by a test on a fixture built from that release.
2. On the committed mini-corpus, the played-hero correction (`project::talent_hero`) gives the same result as with the
   old reference data: an automatic diff, no claim.
3. These endpoints return the same JSON shape as before (snapshot tests): `/api/dim/heroes`, `/api/dim/talents`,
   `/api/patches`, `/api/patches/{id}`, `/api/hero-changes`. The Rust front-end passes its tests unchanged.
4. An old ID (e.g. `hotfix-patch-notes-march-25-2014`) resolves to its patch.
5. A tampered file (SHA-256 mismatch), or a manifest with `schemaVersion` 2, is refused and the previous data is kept.
6. The classifier gives the old verdicts on the old service's test cases, ported verbatim.
7. The standalone bundle size is measured and written in `docs/spec/09-operations.md`. The installer seeds offline from
   it on a clean VM (`docs/standalone-acceptance.md`).
8. Box: a 24 h round with an unchanged `datasetVersion` downloads only `manifest.json`.

## Open questions

- D3 needs an issue on HotsPatchNotes (bundle the `heroSelect` portraits). Who files it, and is (c) acceptable until then?
- The order with PR #32: this spec assumes PR #32 merges first with its placeholder pin, and this work then replaces its
  snapshot pipeline before the first installer release. The alternative is to fold it into PR #32 (a larger PR).
- `/hero-changes` for PTR candidates: hide them (this spec), or show them with a PTR badge?
