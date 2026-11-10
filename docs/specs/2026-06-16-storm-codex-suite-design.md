# Spec — Storm Codex Suite (all-in-one HotS)

**Date:** 2026-06-16 · **Status:** design approved (brainstorm), ready for an implementation plan.
**Goal:** bring the HotS suite (stats + overlays + patch notes + uploader) together into **a unified,
self-hostable product that anyone can launch with `docker compose up`**.

## Vision
Today the suite is scattered: storm-codex (Rust, stats/overlays), HotsPatchNotes (.NET, patch
notes + hero/talent reference data), the client-rs uploader (Rust, gaming PC), Orpheus (Node, music).
Goal: **a single website**, **a single database**, **a single `docker compose up`**, **pre-seeded**, that
anyone can take and run **locally** without scraping or configuring a complex stack.

## Locked decisions (with rationale)
1. **Not a monolithic "single image" → a single `docker compose`.** Mixing Rust/.NET/Node in one
   image is an anti-pattern. For the user, `docker compose up` is just as simple.
2. **User product = 100% Rust**: storm-codex (stats + overlays + **built-in patch notes**) +
   uploader. **HotsPatchNotes (.NET) leaves the bundle** and becomes a **maintainer ingestion tool**
   (scrapes BlueTracker + fetches `heroes-talents`). Rationale: scraping is a maintainer's job,
   not a user's; the product stays single-stack, without rewriting the scraper (which works).
3. **A single shared Postgres** in the compose (database `storm_codex`; the patches/heroes/
   talents reference data lives in it too, see §Reference data). No SQLite/Mongo on the product side.
4. **Orpheus excluded** from the public bundle (depends on a personal Spotify auth). The music widget degrades to
   "Music — off".
5. **Distribution: pre-built images published on ghcr.io.** The user builds nothing.
6. **Purely local use → simple security.** Open mode by default (ADMIN_TOKEN optional, already
   in place), basic local backups. No advanced security/auth (reverse proxy/TLS documented in case of
   exposure, out of product scope).

## Architecture
- **`storm-codex-suite` repo** (new, umbrella): `docker-compose.yml` (references the ghcr.io
  images) + `.env.example` + README. No application code.
- **Compose services**: `postgres` (shared) · `storm-codex-server` (Rust, serves the API + the React
  + the overlays + the patch notes) · `uploader-headless` (Docker option, see §Uploader).
- **storm-codex = THE site**: its React is the only front-end. `HotsPatchNotes.Web` is dropped.

## Reference data by snapshot (heroes / talents / patches / images)
**Problem:** the product must not depend on a live HotsPatchNotes or make the user scrape.
**Solution:** distribute the **data already produced**, not the scraper.
- **Production (maintainer)**: HotsPatchNotes (.NET) exports a **versioned snapshot** =
  `referential-<version>.tar.gz` (`heroes.json`, `talents.json`, `patches.json` + portrait/map
  images). Published as a **GitHub Release** of the suite repo.
- **Consumption (storm-codex)**: new `REFERENTIAL_URL` (default = latest release). At startup
  + periodic check (~24 h) + Admin button: if a newer version exists → download, ingest into
  Postgres (**existing `dim_heroes`/`dim_talents` pattern, extended to `dim_patches`**), unpack the
  images into `images_dir`. Idempotent. **Fallback**: snapshot baked into the image if offline.
- **Consequence**: the user **never scrapes, never runs .NET, never has an empty database**.

## Patch notes freshness
- Blizzard patch → **cron GitHub Action** (maintainer side) runs the ingestion → produces the snapshot
  → publishes a new Release.
- Users: automatically fresh at boot / periodically / via a button, within ≤24 h.

## "New patch" notifications
Detection of a newer snapshot version triggers:
- **In-app**: WS broadcast `patch.new` → toast + badge on the "Patch Notes" tab (reuses the
  existing toast/WS system of new replays).
- **Optional outgoing webhook**: configurable `PATCH_WEBHOOK_URL` → POST `{patchName, type, date,
  link}`, compatible with Discord / ntfy / generic endpoint (Jarvis spine at the maintainer's). Off by
  default.

## Patch notes in storm-codex (full integration)
- storm-codex-server **proxies/serves** the patches from its own database (`dim_patches`), like
  dim/now-playing today → a single origin, no CORS.
- **React views (Nexus Codex design)**: "Patch Notes" tab → list (name, type, date, number of heroes/
  maps) + detail of a patch (hero/talent/map changes). Reuses portraits/talents.

## Headless uploader (Docker option)
- New `uploader-headless` image: client-rs **without GUI**, configured by env (server URL + token +
  replays path), **bind-mount** of the HotS folder, **periodic re-scan** (code already in place).
- The native Windows `.exe` stays the main option (reliable file watching). The headless image targets
  all-Docker users (Docker Desktop + bind-mount).

## Onboarding & what's new (in-app)
- **First-launch wizard**: at the first boot, guided path → operator name, **generation of an
  upload token**, **OBS URLs** to copy, link/command to connect the uploader. Can be re-triggered via
  a "?" button. Admin page "Connect the uploader".
- **"What's new"**: at each app version upgrade, panel of what is new since the last
  visit (version seen in localStorage → modal, dismiss = seen). Distinct from HotS patch notes.

## Backups (simple, local)
- Named Postgres volume + scheduled `pg_dump` (light script/cron) + **documented restore**. No
  encryption/offsite (local use). Safety net for the match history.

## CI / image publication
- Each repo (storm-codex, HotsPatchNotes-api, uploader) gets a **GitHub Actions** workflow: build + push
  to `ghcr.io/matella/*`. The suite repo references those tags + a cron Action for the snapshot.

## Out of scope (YAGNI)
FR/EN i18n · multi-user accounts / advanced auth · light theme · demo data · rewriting
HotsPatchNotes in Rust (only the ingester might be one day, optional).

## Breakdown into batches (build order)
1. **Shared Postgres** — HotsPatchNotes-api → Postgres (instead of SQLite); a single DB service.
2. **Reference snapshot + `dim_patches`** — .NET export + Rust ingestion (boot/periodic/fallback).
3. **Patch notes views** — tab + list + detail in the storm-codex React.
4. **New patch notifications** — WS `patch.new` + toast/badge + optional webhook.
5. **Headless uploader image** — Dockerfile without GUI + env config + bind-mount.
6. **Image publication CI** — Actions per repo → ghcr.io.
7. **Suite repo + compose + docs** — end-to-end `docker compose up`.
8. **Onboarding + first-launch wizard** (+ "connect uploader" page).
9. **In-app "What's new" / changelog.**
10. **Backups/restore** (simple, local).

Each batch can be delivered independently; new batches can be added without reopening the architecture.
