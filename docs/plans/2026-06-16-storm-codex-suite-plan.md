# Storm Codex Suite — Implementation Plan

> **For agentic workers:** implement lot by lot. Each lot is self-contained, produces working/testable
> software, and ships (commit + déploiement box) on its own. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Regrouper la suite HotS en un produit unifié 100% Rust côté utilisateur, lançable via un seul
`docker compose up`, pré-seedé, auto-frais.

**Architecture:** storm-codex (Rust) = site unique (stats + overlays + patch notes) + un Postgres
partagé ; HotsPatchNotes (.NET) = outil mainteneur produisant un snapshot référentiel publié ;
uploader headless dockerisable ; repo suite parapluie avec compose + images ghcr.io.

**Tech Stack:** Rust (axum, sqlx), React+Vite+TS, Postgres, .NET (ingester, hors bundle), Docker
Compose, GitHub Actions / ghcr.io.

**Réf. spec :** `docs/specs/2026-06-16-storm-codex-suite-design.md`

**Ordre d'exécution recommandé (valeur visible d'abord, packaging ensuite) :**
Lot 3 → 2 → 4 → 1 → 5 → 8 → 9 → 10 → 6 → 7.
Justif : les vues patch notes (lot 3) marchent **dès maintenant** contre l'API HotsPatchNotes live
existante (storm-codex a déjà `HOTSPATCHNOTES_URL`) → valeur immédiate ; le snapshot/seeding (1-2) et
le packaging (6-7) sont la couche « anyone runs it » qui vient après.

---

## Chunk 3 : Vues patch notes intégrées (storm-codex) — valeur visible d'abord

**Files:**
- Modify: `crates/storm-codex-server/src/read.rs` (handlers `patches_list`, `patch_detail` — proxy de
  l'API HotsPatchNotes, comme `now_playing`)
- Modify: `crates/storm-codex-server/src/main.rs` (routes `/api/patches`, `/api/patches/{id}`)
- Create: `web/src/pages/Patches.tsx` (liste) · `web/src/pages/Patch.tsx` (détail)
- Modify: `web/src/api.ts` (types Patch + fetchers) · `web/src/App.tsx` (routes) ·
  `web/src/components/Layout.tsx` (onglet « Patch Notes »)

- [ ] **Step 1** — read.rs : `patches_list`/`patch_detail` proxient `{HOTSPATCHNOTES_URL}/api/patches`
  et `/api/patches/{id}` via `ureq`+`spawn_blocking` (calquer `now_playing`). Best-effort → `[]`/null si indispo.
- [ ] **Step 2** — main.rs : enregistrer les 2 routes.
- [ ] **Step 3** — `cargo check -p storm-codex-server` (PASS).
- [ ] **Step 4** — vérifier `curl /api/patches` sur le box renvoie la liste (déjà servie par HotsPatchNotes).
- [ ] **Step 5** — api.ts : `interface Patch`/`PatchDetail` + `fetchPatches()/fetchPatch(id)`.
- [ ] **Step 6** — Patches.tsx : liste (patchName, type, date, heroCount/mapCount), filtre par type, lien détail.
- [ ] **Step 7** — Patch.tsx : détail (changements héros/talents/cartes ; réutilise Avatar/talentInfo).
- [ ] **Step 8** — App.tsx routes `/patches`, `/patch/:id` ; Layout onglet « Patch Notes ».
- [ ] **Step 9** — `npm run build` (✓ built) ; déploiement box ; vérif rendu (preview).
- [ ] **Step 10** — commit `feat(patches): vues patch notes intégrées (proxy API)`.

## Chunk 2 : Snapshot référentiel + `dim_patches` (découple du HotsPatchNotes live)

**Files:**
- Create: migration `crates/storm-codex-server/migrations/000X_dim_patches.sql` (table patches + données JSONB)
- Modify: `crates/storm-codex-server/src/dim.rs` (`sync_patches` ; option ingestion depuis snapshot)
- Modify: `crates/storm-codex-server/src/config.rs` (`referential_url: Option<String>`)
- Modify: `crates/storm-codex-server/src/main.rs` (ingestion au boot + refresh périodique)
- Côté .NET (box, repo HotsPatchNotes) : commande/endpoint `export` → `referential.tar.gz`
  (heroes.json, talents.json, patches.json, images/).

- [ ] **Step 1** — migration `dim_patches (id TEXT PK, name, type, live_date, data JSONB)`.
- [ ] **Step 2** — dim.rs `sync_patches(db, base)` : depuis l'API (live) → upsert dim_patches (pattern dim_heroes).
- [ ] **Step 3** — read.rs : `patches_list/patch_detail` lisent **dim_patches** (plus le proxy).
- [ ] **Step 4** — config `REFERENTIAL_URL` + `dim::ingest_snapshot(url)` : télécharge le tarball,
  ingère heroes/talents/patches, décompresse images dans `images_dir` ; versionné (skip si à jour) ;
  fallback baké.
- [ ] **Step 5** — main.rs : au boot, si `REFERENTIAL_URL` → ingest_snapshot ; sinon sync live (compat).
  Refresh périodique (~24 h).
- [ ] **Step 6** — .NET : commande `export-snapshot` produisant le tarball.
- [ ] **Step 7** — cargo check + build web + déploiement + vérif. Commit `feat(referential): snapshot + dim_patches`.

## Chunk 4 : Notifications nouveau patch

**Files:** `crates/storm-codex-server/src/dim.rs` (détection version), `ws.rs` (event `patch.new`),
`src/config.rs` (`patch_webhook_url`), `web/src/components/Layout.tsx` (toast/pastille).

- [ ] **Step 1** — à l'ingestion, si nouvelle version → broadcast WS `{type:"patch.new", name,...}`.
- [ ] **Step 2** — Layout : écouter `patch.new` → toast + pastille sur l'onglet Patch Notes (réutilise le mécanisme replays).
- [ ] **Step 3** — config `PATCH_WEBHOOK_URL` ; POST `{patchName,type,date,link}` best-effort (Discord/ntfy/générique).
- [ ] **Step 4** — build + déploiement ; commit `feat(patches): notif nouveau patch (toast + webhook)`.

## Chunk 1 : Postgres partagé (HotsPatchNotes-api → Postgres)

**Files:** repo HotsPatchNotes (box) : `docker-compose.yml` (activer service postgres OU pointer le
Postgres partagé), `appsettings`/env (`ConnectionStrings__DefaultConnection` → Host=postgres). Migration EF si nécessaire.

- [ ] **Step 1** — basculer la connection string de SQLite → Postgres partagé (option déjà commentée).
- [ ] **Step 2** — appliquer les migrations EF sur la base `hotspatchnotes`.
- [ ] **Step 3** — rebuild hotspatchnotes-api ; vérifier `/api/patches` + `/api/heroes` OK sur Postgres.
- [ ] **Step 4** — commit (repo HotsPatchNotes) `chore(db): Postgres partagé`.

## Chunk 5 : Image uploader headless

**Files:** repo Hots-Overlay/client-rs : `Dockerfile` (build headless), un binaire/feature sans GUI
(`--headless` ou crate-feature désactivant eframe/tray), config par env (SERVER_URL, AUTH_TOKEN, REPLAY_DIRS).

- [ ] **Step 1** — feature `headless` : `main` sans eframe/tray, lit env, lance watcher + re-scan périodique (déjà en place), boucle.
- [ ] **Step 2** — Dockerfile (rust:alpine ou debian) buildant la feature headless.
- [ ] **Step 3** — test local : `docker run -e SERVER_URL=… -v <replays>:/replays` → upload OK (re-scan).
- [ ] **Step 4** — commit (Hots-Overlay) `feat(uploader): image headless dockerisable`.

## Chunk 8 : Onboarding + assistant 1er lancement

**Files:** `web/src/components/Onboarding.tsx` (tour + wizard), `web/src/pages/Admin.tsx` (section
« Connect the uploader »), localStorage `onboarding_done`.

- [ ] **Step 1** — détecter 1er lancement (pas de operator_names && pas de matchs) → wizard : pseudo
  opérateur, **générer token upload**, afficher URLs OBS + commande uploader.
- [ ] **Step 2** — tour guidé (driver.js ou stepper maison) re-déclenchable via bouton « ? ».
- [ ] **Step 3** — Admin : page « Connect the uploader » (URL serveur, token, exemples natif/docker).
- [ ] **Step 4** — build + déploiement ; commit `feat(onboarding): wizard 1er lancement + tour`.

## Chunk 9 : « What's new » / changelog in-app

**Files:** `web/src/whatsnew.ts` (entrées versionnées), `web/src/components/WhatsNew.tsx`, localStorage `seen_version`.

- [ ] **Step 1** — fichier changelog (version → liste de nouveautés).
- [ ] **Step 2** — au load, si `APP_VERSION` > `seen_version` → modale « What's new » ; dismiss = vu.
- [ ] **Step 3** — build + déploiement ; commit `feat(ui): panneau What's new au changement de version`.

## Chunk 10 : Sauvegardes (simple, local)

**Files:** repo suite : `scripts/backup.sh` (pg_dump → volume), doc restore dans README ; volume Postgres nommé.

- [ ] **Step 1** — script `backup.sh` : `pg_dump` vers `./backups/storm_codex-<date>.sql.gz`.
- [ ] **Step 2** — documenter restore (`psql < dump`). Optionnel : service cron léger dans le compose.
- [ ] **Step 3** — commit (repo suite) `feat(ops): backup/restore simple`.

## Chunk 6 : CI publication d'images (ghcr.io)

**Files:** `.github/workflows/publish.yml` dans chaque repo (storm-codex, HotsPatchNotes, Hots-Overlay)
+ une Action cron snapshot dans le repo suite.

- [ ] **Step 1** — storm-codex : workflow build+push `ghcr.io/matella/storm-codex` (sur tag/main).
- [ ] **Step 2** — HotsPatchNotes : workflow push `ghcr.io/matella/hotspatchnotes-api`.
- [ ] **Step 3** — Hots-Overlay : workflow push `ghcr.io/matella/hots-uploader`.
- [ ] **Step 4** — repo suite : Action cron → run ingestion .NET → export snapshot → GitHub Release.
- [ ] **Step 5** — ⚠️ nécessite l'opérateur : droits ghcr + secrets. Vérifier un build CI.

## Chunk 7 : Repo suite + compose + docs

**Files:** nouveau repo `storm-codex-suite` : `docker-compose.yml`, `.env.example`, `README.md`.

- [ ] **Step 1** — compose : `postgres` partagé + `storm-codex-server` (image ghcr) + `uploader-headless` (profil optionnel).
- [ ] **Step 2** — `.env.example` : DATABASE_URL, REFERENTIAL_URL, ADMIN_TOKEN (vide=ouvert), PATCH_WEBHOOK_URL, REPLAY_DIRS…
- [ ] **Step 3** — README : « docker compose up », onboarding, brancher l'uploader (natif/docker).
- [ ] **Step 4** — ⚠️ opérateur : `gh repo create matella/storm-codex-suite` ; push.
- [ ] **Step 5** — test bout-en-bout sur une machine neuve : `docker compose up` → site + données + overlays.

---

**Hors scope (YAGNI) :** i18n, multi-utilisateurs/auth avancée, thème clair, données de démo, réécriture .NET→Rust.

**Points nécessitant l'opérateur :** droits/secrets ghcr (chunk 6), création du repo suite GitHub (chunk 7).
