# Storm Codex Suite — Implementation Plan

> **For agentic workers:** implement batch by batch. Each batch is self-contained, produces working/testable
> software, and ships (commit + box deployment) on its own. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Group the HotS suite into a unified product, 100% Rust on the user side, launchable via a single
`docker compose up`, pre-seeded, self-refreshing.

**Architecture:** storm-codex (Rust) = single site (stats + overlays + patch notes) + a shared
Postgres; HotsPatchNotes (.NET) = maintainer tool producing a published reference-data snapshot;
dockerisable headless uploader; umbrella suite repo with compose + ghcr.io images.

**Tech Stack:** Rust (axum, sqlx), React+Vite+TS, Postgres, .NET (ingester, outside the bundle), Docker
Compose, GitHub Actions / ghcr.io.

**Spec ref.:** `docs/specs/2026-06-16-storm-codex-suite-design.md`

**Recommended execution order (visible value first, packaging next):**
Batch 3 → 2 → 4 → 1 → 5 → 8 → 9 → 10 → 6 → 7.
Rationale: the patch notes views (batch 3) work **right now** against the existing live HotsPatchNotes API
(storm-codex already has `HOTSPATCHNOTES_URL`) → immediate value; the snapshot/seeding (1-2) and
the packaging (6-7) are the "anyone runs it" layer that comes after.

---

## Chunk 3: Integrated patch notes views (storm-codex) — visible value first

**Files:**
- Modify: `crates/storm-codex-server/src/read.rs` (handlers `patches_list`, `patch_detail` — proxy of
  the HotsPatchNotes API, like `now_playing`)
- Modify: `crates/storm-codex-server/src/main.rs` (routes `/api/patches`, `/api/patches/{id}`)
- Create: `web/src/pages/Patches.tsx` (list) · `web/src/pages/Patch.tsx` (detail)
- Modify: `web/src/api.ts` (types Patch + fetchers) · `web/src/App.tsx` (routes) ·
  `web/src/components/Layout.tsx` ("Patch Notes" tab)

- [ ] **Step 1** — read.rs: `patches_list`/`patch_detail` proxy `{HOTSPATCHNOTES_URL}/api/patches`
  and `/api/patches/{id}` via `ureq`+`spawn_blocking` (model on `now_playing`). Best-effort → `[]`/null if unavailable.
- [ ] **Step 2** — main.rs: register the 2 routes.
- [ ] **Step 3** — `cargo check -p storm-codex-server` (PASS).
- [ ] **Step 4** — check `curl /api/patches` on the box returns the list (already served by HotsPatchNotes).
- [ ] **Step 5** — api.ts: `interface Patch`/`PatchDetail` + `fetchPatches()/fetchPatch(id)`.
- [ ] **Step 6** — Patches.tsx: list (patchName, type, date, heroCount/mapCount), filter by type, detail link.
- [ ] **Step 7** — Patch.tsx: detail (hero/talent/map changes; reuses Avatar/talentInfo).
- [ ] **Step 8** — App.tsx routes `/patches`, `/patch/:id`; Layout tab "Patch Notes".
- [ ] **Step 9** — `npm run build` (✓ built); box deployment; check rendering (preview).
- [ ] **Step 10** — commit `feat(patches): integrated patch notes views (API proxy)`.

## Chunk 2: Reference-data snapshot + `dim_patches` (decouples from live HotsPatchNotes)

**Files:**
- Create: migration `crates/storm-codex-server/migrations/000X_dim_patches.sql` (patches table + JSONB data)
- Modify: `crates/storm-codex-server/src/dim.rs` (`sync_patches`; option of ingestion from a snapshot)
- Modify: `crates/storm-codex-server/src/config.rs` (`referential_url: Option<String>`)
- Modify: `crates/storm-codex-server/src/main.rs` (ingestion at boot + periodic refresh)
- .NET side (box, HotsPatchNotes repo): `export` command/endpoint → `referential.tar.gz`
  (heroes.json, talents.json, patches.json, images/).

- [ ] **Step 1** — migration `dim_patches (id TEXT PK, name, type, live_date, data JSONB)`.
- [ ] **Step 2** — dim.rs `sync_patches(db, base)`: from the (live) API → upsert dim_patches (dim_heroes pattern).
- [ ] **Step 3** — read.rs: `patches_list/patch_detail` read **dim_patches** (no longer the proxy).
- [ ] **Step 4** — config `REFERENTIAL_URL` + `dim::ingest_snapshot(url)`: downloads the tarball,
  ingests heroes/talents/patches, unpacks images into `images_dir`; versioned (skip if up to date);
  baked-in fallback.
- [ ] **Step 5** — main.rs: at boot, if `REFERENTIAL_URL` → ingest_snapshot; otherwise live sync (compat).
  Periodic refresh (~24 h).
- [ ] **Step 6** — .NET: `export-snapshot` command producing the tarball.
- [ ] **Step 7** — cargo check + web build + deployment + check. Commit `feat(referential): snapshot + dim_patches`.

## Chunk 4: New patch notifications

**Files:** `crates/storm-codex-server/src/dim.rs` (version detection), `ws.rs` (`patch.new` event),
`src/config.rs` (`patch_webhook_url`), `web/src/components/Layout.tsx` (toast/dot).

- [ ] **Step 1** — at ingestion, if new version → WS broadcast `{type:"patch.new", name,...}`.
- [ ] **Step 2** — Layout: listen to `patch.new` → toast + dot on the Patch Notes tab (reuses the replays mechanism).
- [ ] **Step 3** — config `PATCH_WEBHOOK_URL`; best-effort POST `{patchName,type,date,link}` (Discord/ntfy/generic).
- [ ] **Step 4** — build + deployment; commit `feat(patches): new patch notification (toast + webhook)`.

## Chunk 1: Shared Postgres (HotsPatchNotes-api → Postgres)

**Files:** HotsPatchNotes repo (box): `docker-compose.yml` (enable the postgres service OR point to the
shared Postgres), `appsettings`/env (`ConnectionStrings__DefaultConnection` → Host=postgres). EF migration if needed.

- [ ] **Step 1** — switch the connection string from SQLite → shared Postgres (option already commented out).
- [ ] **Step 2** — apply the EF migrations on the `hotspatchnotes` database.
- [ ] **Step 3** — rebuild hotspatchnotes-api; check `/api/patches` + `/api/heroes` OK on Postgres.
- [ ] **Step 4** — commit (HotsPatchNotes repo) `chore(db): shared Postgres`.

## Chunk 5: Headless uploader image

**Files:** Hots-Overlay/client-rs repo: `Dockerfile` (headless build), a binary/feature without GUI
(`--headless` or crate feature disabling eframe/tray), config from env (SERVER_URL, AUTH_TOKEN, REPLAY_DIRS).

- [ ] **Step 1** — `headless` feature: `main` without eframe/tray, reads env, starts watcher + periodic re-scan (already in place), loop.
- [ ] **Step 2** — Dockerfile (rust:alpine or debian) building the headless feature.
- [ ] **Step 3** — local test: `docker run -e SERVER_URL=… -v <replays>:/replays` → upload OK (re-scan).
- [ ] **Step 4** — commit (Hots-Overlay) `feat(uploader): dockerisable headless image`.

## Chunk 8: Onboarding + first-launch wizard

**Files:** `web/src/components/Onboarding.tsx` (tour + wizard), `web/src/pages/Admin.tsx` (section
"Connect the uploader" section), localStorage `onboarding_done`.

- [ ] **Step 1** — detect first launch (no operator_names && no matches) → wizard: operator
  nickname, **generate upload token**, show OBS URLs + uploader command.
- [ ] **Step 2** — guided tour (driver.js or home-made stepper) re-triggerable via a "?" button.
- [ ] **Step 3** — Admin: "Connect the uploader" page (server URL, token, native/docker examples).
- [ ] **Step 4** — build + deployment; commit `feat(onboarding): first-launch wizard + tour`.

## Chunk 9: "What's new" / in-app changelog

**Files:** `web/src/whatsnew.ts` (versioned entries), `web/src/components/WhatsNew.tsx`, localStorage `seen_version`.

- [ ] **Step 1** — changelog file (version → list of new features).
- [ ] **Step 2** — on load, if `APP_VERSION` > `seen_version` → "What's new" modal; dismiss = seen.
- [ ] **Step 3** — build + deployment; commit `feat(ui): What's new panel on version change`.

## Chunk 10: Backups (simple, local)

**Files:** suite repo: `scripts/backup.sh` (pg_dump → volume), restore doc in the README; named Postgres volume.

- [ ] **Step 1** — `backup.sh` script: `pg_dump` to `./backups/storm_codex-<date>.sql.gz`.
- [ ] **Step 2** — document restore (`psql < dump`). Optional: light cron service in the compose.
- [ ] **Step 3** — commit (suite repo) `feat(ops): simple backup/restore`.

## Chunk 6: Image publishing CI (ghcr.io)

**Files:** `.github/workflows/publish.yml` in each repo (storm-codex, HotsPatchNotes, Hots-Overlay)
+ a snapshot cron Action in the suite repo.

- [ ] **Step 1** — storm-codex: build+push workflow `ghcr.io/matella/storm-codex` (on tag/main).
- [ ] **Step 2** — HotsPatchNotes: push workflow `ghcr.io/matella/hotspatchnotes-api`.
- [ ] **Step 3** — Hots-Overlay: push workflow `ghcr.io/matella/hots-uploader`.
- [ ] **Step 4** — suite repo: cron Action → run .NET ingestion → export snapshot → GitHub Release.
- [ ] **Step 5** — ⚠️ requires the operator: ghcr rights + secrets. Check a CI build.

## Chunk 7: Suite repo + compose + docs

**Files:** new repo `storm-codex-suite`: `docker-compose.yml`, `.env.example`, `README.md`.

- [ ] **Step 1** — compose: shared `postgres` + `storm-codex-server` (ghcr image) + `uploader-headless` (optional profile).
- [ ] **Step 2** — `.env.example`: DATABASE_URL, REFERENTIAL_URL, ADMIN_TOKEN (empty=open), PATCH_WEBHOOK_URL, REPLAY_DIRS…
- [ ] **Step 3** — README: "docker compose up", onboarding, plug in the uploader (native/docker).
- [ ] **Step 4** — ⚠️ operator: `gh repo create matella/storm-codex-suite`; push.
- [ ] **Step 5** — end-to-end test on a fresh machine: `docker compose up` → site + data + overlays.

---

**Out of scope (YAGNI):** i18n, multi-user/advanced auth, light theme, demo data, .NET→Rust rewrite.

**Points requiring the operator:** ghcr rights/secrets (chunk 6), creation of the GitHub suite repo (chunk 7).
