# STATUS — read first, update last

## Standalone Windows installer (2026-11-14), branch `feat/standalone-installer` (not merged, not released)

Spec `docs/specs/2026-11-12-standalone-installer-design.md`, plan `docs/plans/2026-11-12-standalone-installer.md`
(Tasks 1–10 implemented). One Windows app: Tauri v2 shell (`desktop/`) supervising a bundled Postgres 17
(`crates/storm-codex-supervisor`) and the server in standalone mode (`STANDALONE=1`: loopback only, in-process replay
watcher `crates/storm-codex-watcher`, first-launch import with progress, `/api/standalone/*`), Welcome page and "This PC"
settings in the web app, tray, single instance, updater (signed, `latest.json`), export/import of all data, autostart
(off by default), NSIS installer (per user, keep/delete data on uninstall) and `release-desktop.yml`. Contracts:
`docs/spec/04-serveur.md` (Standalone import), `05-api.md`, `08-frontend.md`, `09-operations.md` (Standalone desktop app).

**Acceptance: not complete** — [`docs/standalone-acceptance.md`](standalone-acceptance.md). Run on the operator's PC
with a debug shell and the real replay folders (read in place, scratch data root): first-launch import of 3,427
replays with no manual step, hard kill mid-import then resume with no data loss (3,425 distinct files = 3,425
uploads), new replay live in 2.6 s, six OBS routes 200, second launch focuses the first, clean stop, accented data
root OK. Measured: parse median ≈ 140 ms (spike50), API p95 39 ms on 3,345 matches, backfill ≈ 2.7 min for 3,427
files, first start 7.7–8.8 s (debug shell). Fixed during the run: a port held on `[::]` by another program
(qBittorrent on 8088) made `http://localhost:<port>` reach that program (`pick_port` now probes `[::1]`); saved extra
folders now load before the server listens (were `[]` while seeding); Postgres' narrow-char tools now get every path
(binaries included) in a form that fits the ANSI code page (8.3 short path, else a junction in a private
`%ProgramData%\StormCodex-<SID>` folder), so a Cyrillic profile name works (L1, verified end to end with binaries and
data under a Cyrillic folder); replays the server stores as `parse_failed` are listed as "not imported" (L5).

**Final whole-branch review (2026-11-14): fixed** — C1 (VC++ runtime bundled app-locally in `postgres\bin` + static
CRT for the release server, both checked by `scripts/check-pe-imports.ps1`), I1 (archived paths relative + resolved
under the current archive: backups move across PCs), I2 (newer bundled reference snapshot ingested after an update),
I3 (outdated `parser_version` re-processed in the background, progress on Welcome), I4 (Host/Origin checks on the
whole app in standalone mode), I5 ("Start with Windows" in This PC), plus in-game names detected after the first
import (were the box operator's seed) and clean import error messages. Report:
`.superpowers/sdd/2026-11-12-standalone-installer/final-fix-report.md`. Not verifiable here: a VM without the VC++
redistributable (acceptance step 0), the release workflow and NSIS hooks (never run).

**Next step (operator):**
1. Generate the updater key pair (`cargo tauri signer generate`), set the secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, replace `plugins.updater.pubkey` in `desktop/tauri.conf.json`.
2. Build the reference snapshot from the real HotsPatchNotes API (`scripts/make-referential.sh`), upload it to a
   non-latest pre-release, commit its URL + SHA-256 in `desktop/referential-pin.txt`.
3. First dry run of `release-desktop.yml` (Run workflow → draft): proves the NSIS hooks compile; record installer
   size and first start in `09-operations.md`.
4. Run the VM acceptance checklist in `docs/standalone-acceptance.md` (install, live game, OBS, kill/power cut,
   update N → N+1, hooks, uninstall keep/delete, Cyrillic/accented profile, no HotS install, export/import).
5. Review/merge the branch and tag `desktop-v0.1.0`.

Follow-ups outside v1: HotsPatchNotes snapshot publication (matella/HeroesOfTheStormPatchNotes#1), runtime snapshot
refresh, SQLite instead of bundled Postgres (matella/storm-codex#31), a code-signing certificate (SmartScreen).

**React removed (2026-11-12), branch `chore/remove-react-frontend`:** `web/`, `.nvmrc`, the CI `web` job, the Dockerfile Node stage, the `storm-codex-web` launch config and the server's React serving (`serve_spa`, `WEB_ROOT`, `WEB_V2_DIR`) are gone. `WEB_DIR` now points at the Rust bundles (`web-rs/dist`; `/app/web` in the image) and every page answers at `/`; `publish` waits for `web-rs`. Rollback = deploy the previous image tag. Last commit with the React sources: `2134853` (the `Port of web/src/...` comments in `web-rs/` refer to it). Specs 01/04/08/09/10, `gads/PROJECT.md` and the spec README rewritten for the Rust front-end; dated specs, plans, runbooks and STATUS-historique are left as written (history).

**Leftovers closed (2026-11-12):** nothing of React is left unported. The `window.__cal` hook is ported; Replay 2D (playback, calibration save, webm export) and every Cast write (New series, marks, start/remove game, winners, per-game and batch replay load, Draft pick/undo/reset, Draft replay Auto) were exercised in the Rust app on an isolated database, with no discrepancy found; stale comments fixed. Only `web/` itself remains, for rollback, until one release after the switch.

**Step 5 — the switch (2026-11-12), branch `feat/rust-frontend-switch`:** `WEB_ROOT=rust` makes the server answer every page at `/` with the Rust front-end (`serve_v2_at_root`, API routes first, bundle files stay under `/v2/<bundle>/`, `/v2/...` still works); the app picks its router base from the loaded URL (`route::init_base`), OBS URL hints and the Cast preview/copy URL follow it. Dockerfile: new stage `web-rs` (pinned Trunk 0.22.0-beta.1, built OK in Docker: 3.4 MB app dir), image sets `WEB_V2_DIR=/app/web-rs` and `WEB_ROOT=rust`, React build kept for rollback (`WEB_ROOT=react` + restart). Checked at `:8098` with `WEB_ROOT=rust`: pages, client-side navigation and back, `/draft` legacy redirect, Cast URLs, every OBS source (`/cast/overlay`, `/draft/overlay`, `/ticker`, `/widget`, `/queue`, `/now-playing`) and `/v2/...`, no console error. **Not done:** the full Docker image was not built end to end (only the `web-rs` stage; CI `publish` will), real-OBS pass on the final URLs (operator), deleting `web/` + its CI job + the Node stage (one release after the switch).

**Size pass (2026-11-11, measured, nothing changed in the build):** the complete app is **1,242 kB gzip / 911 kB brotli** (wasm 3.2 MB raw; React's whole JS+CSS is 246 kB gzip). The server already negotiates brotli, so the transferred size is the brotli one. Code size by crate: `tachys` view machinery 35 % (incl. `IntoAny`), `core` 13 %, `storm_web_app` 10 %, `reactive_graph` 9 %, per-type serde 5.5 %, `fetch` futures (`ScopedFuture`) 4.6 %. `opt-level="s"` is worse (1,406 kB gzip) than `"z"`; `wasm-opt z` is already on. Remaining levers, each small: a non-generic core for `ui::fetch` (about 4 %), fewer distinct `view!` shapes. Budget decided: **app <= 1 MB brotli** enforced in CI (`SIZE_CODEC=br`). JetBrains Mono is now self-hosted in the `sources` and `overlay` bundles (verified: no Google Fonts request from `/v2/ticker`; the overlay still loads Cinzel/Oswald from Google like React).

## Rust front-end migration — step 4 started: Scouting and Admin (2026-11-11)
Step 3 is complete. Step 4 (PR #27): **Scouting** (list and creation) and **Admin** (identity, upload health, tokens, teams, collections)
are ported and DOM identical to React on real data; Admin writes were exercised on the dev database. New `net::write` (authenticated
writes with the stored admin token) and `core/scouting.rs`. **Scouting report** is also ported (DOM identical to React on real data; rename, roster, upload, import tested). **Export HTML** is ported too
(`core/reportexport.rs` + `app/export.rs`; output equal to React's on report 3 modulo asset-rule order and minimap encodes). The **Player Identity tab** is ported too. The **Cast control room** is ported too (rail, scenes, pool, Draft Live and Replay; `core/castctl.rs`; DOM identical to React on every workspace; the destructive actions were later run on an isolated empty database, see `08-frontend.md`). Step 4 is complete. Next: size pass on the app bundle (operator sets the budget), then the step 5 switch (needs the operator's go).

### Earlier: step 3 complete
Fifth batch (PR #27): **Match detail** (DOM identical to React on match 2746, SVG attributes included) and **Live** (identical for the idle
state and for a synthetic game through a mock `/api/live`). The Match **Replay 2D** tab is ported (canvas pixel identical to React at the same time). Logic in
`core/matchdetail.rs` and `core/livegame.rs` (90 native tests); talents are loaded once and shared. **App size: about 640 kB gzip for the
wasm (13 pages → 15 pages)**: the size pass is overdue and the operator must set the app budget. Step 3 is done; next is step 4 (Replay 2D,
Scouting, Scouting report, Admin, Cast, Player Identity tab), then the step 5 switch (needs approval).

### Earlier: Hero, Trends, Hero changes, Patch added
Fourth batch (PR #27): **Hero** (URL-driven filter, maps, winning builds with talent names, patch history), **Trends**, **Hero changes**
(By hero / Timeline, filters, unfolded history) and **Patch**. DOM identical to React on real data for Hero, Trends and Hero changes
(including interactions). **Patch detail** could only be checked on its "not found" path (the dev database returns `null` for every patch);
the content goes through the same `SafeHtml` as the patch history. New: the HTML sanitiser, **without `ammonia`** (about 300 kB): an
allowlist walk over an inert `<template>` (`core/sanitize.rs`, `app/src/safehtml.rs`), stricter than DOMPurify; the operator should
confirm this replaces the spike's `ammonia` option. Lists of unfoldable entries are keyed (Leptos reuses rows by position, which moved the
open state of a `<details>` onto another hero). Logic in `core/herodetail.rs`, `trends.rs`, `patchnotes.rs`, `sanitize.rs` (75 native
tests). **App size: 533 kB gzip with 13 pages** (428 kB with 9): +105 kB for four pages, the size pass is now needed. Remaining in
step 3: Live, Match detail.

### Earlier: Players, Player, Synergies added
Third batch (PR #27): **Players** (debounced search, `?q=`), **Player** (cover, hero pool, recent games, chat habits; the Identity tab is a
placeholder until step 4 because it writes with the admin token) and **Synergies**, all DOM-identical to React on real data. Logic in
`core/players.rs` (62 native tests). **App size: 428 kB gzip with 9 pages** (361 kB with 6): the size pass the operator postponed is now
overdue before the remaining pages. Remaining in step 3: Hero, Trends, Hero changes, Patch, Live, Match detail.

### Earlier: Session and Matches added
Second batch (PR #27): pages **Session** and **Matches** (DOM identical to React on real data; Matches filters live in the URL and were
exercised: mode, result, MVP, map search, reset, CSV/JSON links, row click and back). Logic in `core/dashboard.rs` (last session with the
3 h gap, tallies, filter and URL parameters), 58 native tests. **App size is now 361 kB gzip with 6 pages** (was 274 kB with 4): each page
costs about 40 kB, so the full app would be around 1 MB gzip; the operator must set the budget, and a size pass (view generics, `opt-level`,
wasm-opt level) is worth doing before the later pages. **Match detail** (dynamic JSON, timeline, chat, bans, XP curve) is next, then
Players, Player, Hero, Synergies, Trends, Hero changes, Patch, Live.

### Earlier in step 3: app bundle, layout, 4 pages
After the operator's go ("continue to pages"): new bundle `web-rs/app` (`/v2/app/`, router base `/v2`), with the top bar, the live
channel (toast, "online", new-patch dot), the onboarding and what's new modals, the `/draft` redirects, and the pages **Heroes, Maps,
Patch Notes, Leagues** (normalised DOM identical to React on real data). Fonts are now self-hosted (`web-rs/app/fonts`). The other
pages show a placeholder. **Size**: the app bundle is 274 kB gzip with these 4 pages, so it will pass 300 kB with the other pages and
the sanitiser; the 300 kB contract is for the OBS bundles, the operator has to set the app budget (CI records the size without a limit
for now). **Checklist of what remains in step 3**: Session (dashboard), Live, Matches, Match (without Replay 2D), Players, Player, Hero,
Synergies, Trends, Hero changes, Patch. **Next**: Session and Matches, then the player and hero pages.

## Rust front-end migration — step 2 done: the six OBS sources are ported (2026-11-11)
All OBS sources now exist under `/v2/`: `/cast/overlay`, `/draft/overlay` (bundle `overlay`, 229 kB gzip) and `/ticker`, `/widget`,
`/queue`, `/now-playing` (+ `?mini`, `?reveal`) (bundle `sources`, 191 kB gzip). Each checked in the browser against React: normalised DOM
identical on real data and on a synthetic multi-game session (mixed modes, awards, two operator accounts, streak, music card); reveal
phases observed (big, then mini after about 2.6 s). Logic lives in `web-rs/core` (`session.rs`, `music.rs`, `route.rs`) with native tests
(35). One bundle for everything came to 314 kB (over the 300 kB contract), so the session and music sources are a second bundle; the server
routes each path to its bundle (test `v2_serves_app_and_overlay_pages`); CI builds and size-checks both. **Blockers**: (1) real-OBS check
of each source (operator; `/v2/cast/overlay` was already pending); (2) **checkpoint (spec section 8): the operator decides whether to
continue with the app pages** (step 3 read-only pages, step 4 interactive pages, step 5 switch); (3) the switch needs approval. **To
fix before the switch**: self-host JetBrains Mono (loaded from Google Fonts in `base.css` and the cast CSS); the Dockerfile does not
build `web-rs` yet. **Next** (after the go): app bundle `web-rs/app` and the read-only pages.

## Rust front-end migration — step 1 foundation started: `web-rs/` + `/v2` (2026-11-11)
Started **before** the real-OBS go (operator's call: "continue"): React stays at `/`, everything lives under `/v2/`, so it is
discardable if the OBS check fails. Done: `web-rs/` workspace (`core`, `overlay`), the cast overlay (five scenes) served at
`/v2/cast/overlay` by the server (`WEB_V2_DIR`, test `v2_serves_app_and_overlay_pages`), CI job `web-rs` with the 300 kB size budget
(227 kB), docs (04, 08, 09, 10). GADS 1.3.0 pulled (`gads/standard/CODING_PRINCIPLES.md`: surgical changes, no speculative code):
only what the overlay uses was carried over from the spike. **Blockers**: (1) real-OBS check of `/v2/cast/overlay` (operator) before the
go is final; (2) checkpoint after step 2 (OBS sources): the operator decides whether to continue; (3) the switch needs approval.
**Next**: `/v2/draft/overlay`, `/ticker`, `/now-playing`, `/widget`, `/queue` (each needs its own real-OBS check), then the checkpoint.

## Rust front-end migration — Leptos chosen (2026-11-11)
The operator picked **Leptos** after the step-0 spike (overlay with all scenes ≈ 219 kB transferred vs
≈ 287 kB for Dioxus, 300 kB limit). Decision recorded in §6 of the spec. **Still open before the go**: real-OBS
check of the Leptos overlay (operator), cold time-to-first-scene measured on it. Next (after go): port order §8,
step 1 foundation. The Dioxus spike apps stay in `spike/` as reference only.

## Rust front-end migration — step 0 spike: overlay, profile, control room, canvas, sanitising done (2026-11-10)
`spike/frontend-rust/` (own workspace, excluded from the root one; results in its `RESULTS.md`).
The cast overlay (`ingame` + `series` map strip) and the player profile (Stats / Identity tabs) run in
Leptos 0.8 and Dioxus 0.7 against the real server with a shared Rust core: DOM identical to React's,
overlay behaviour (fade, live update, reconnection) and profile behaviour (`?tab=`, show all) verified.
All five overlay scenes (lineups, post-game slides, draft included) are DOM-identical to React in Dioxus and
identical in Leptos but for two `<g>` wrappers in an SVG: ≈ 219 kB (Leptos) / ≈ 287 kB (Dioxus) transferred, the
latter only 4 % under the 300 kB limit (React ≈ 143 kB for one scene); profile ≈ 215 / 294 kB gzip. Canvas + `MediaRecorder` and sanitising pass, but the Rust sanitiser alone costs ≈ 296 kB gzip, so
it cannot live in the bundle the overlays load. The reduced control room (`/cast`: scene tabs, Put on air,
scaled preview, Blue team editor, error banner) behaves the same in both: ≈ 137 kB (Leptos) / ≈ 260 kB
(Dioxus) gzip. **Not done**: rest of the control room,
real-OBS check (operator), cold time-to-first-scene, a real patch page. No go decision yet.
Tools installed on the PC: `trunk` 0.22.0-beta.1 (0.22.1 needs rustc 1.96), `dioxus-cli` 0.7.10,
`wasm-bindgen-cli` 0.2.130.

## Rust front-end migration — spec validated (2026-11-10)
Spec `docs/specs/2026-11-09-rust-frontend-migration-design.md`: port `web/` to Rust/WASM served by the
same Axum server, page by page under `/v2/`, React kept until the operator approves the switch.
Step 0 = throwaway go/no-go spike in `spike/frontend-rust/` (Leptos vs Dioxus on the player profile,
cast control room and cast overlay). **An overlay that misses its size/time limit ends the whole
port** (operator decision). Real-OBS checks are the operator's. Dependencies upgraded the same day
(PR #19: sqlx 0.9, redis 1, sha2 0.11, tower-http 0.7, React 19, TypeScript 7; `libsqlite3-sys`
RUSTSEC-2026-0459 ignore stays until sqlx allows >= 0.39).

## APM per game and per player (2026-11-08, branch `feat/apm`)
Spec `docs/specs/2026-11-08-apm-par-partie-design.md` (operator request, batch D "keyboard" reduced to APM).
`storm-stats` counts the `SCmdEvent`s per toon (`Output.actions`, outside parity), migration `0016`
(`match_players.apm`), `PARSER_VERSION` 4. API: `avg_apm`/`apm_games`/`recent[].apm` on
`/api/players/{toon}`, `apm` per player on `/api/matches/{id}`. Web: Avg APM tile, badge per recent
game, APM column in the score table. **To do (operator)**: `POST /api/admin/reprocess` to fill
`apm` on the corpus. Rest of batch D (keys used, fingerprint): not done.

## Chat habits (2026-11-08)
Plan `docs/plans/2026-11-08-chat-habitudes.md` (spec `2026-11-08-signaux-replay-supplementaires-design.md`, batch C).
`GET /api/players/{toon}/chat` (`players/chat.rs`): measures only, never the text, no
migration. "Chat" section of the player page + compact line in the scouting "Their players"
(not in the HTML export). **Remaining** (specs approved, plans to write): B deserters, A cosmetics,
E level (mastery/boost), D keyboard. Idea 2 (groups) dropped: empty fields in the replays.
Verified: clippy, `cargo test` (dev Postgres), vitest 256, web build. Not checked by eye: rendering
of the section with real data.

## Hotkey profiles → accounts of the same person (2026-11-08)
Spec `docs/specs/2026-11-08-profils-hotkeys-comptes-design.md` (approved). `storm-stats` reads
`SUserOptionsEvent.m_hotkeyProfile` (outside Node parity), migration `0015` (`match_players.hotkey_profile`),
`PARSER_VERSION` 3, module `players/profiles.rs`, signal `profile` (strong, HIGH on its own), section
"Hotkey profiles" of the Identity tab. **To do (operator)**: `POST /api/admin/reprocess` on the
corpus to fill the column, then a probe on the real corpus to tune the generic threshold (5 accounts).
Verified: clippy, `cargo test --workspace` with dev Postgres, vitest, web build.

## Full review + fixes (2026-11-07, branch `fix/revue-complete`)
Operator request: review the whole project then "fix it all". No data contract change
(no migration, no `PARSER_VERSION` bump).
- **Dev database**: migration `0014` had been applied during PR #12 then rewritten before the
  merge → `VersionMismatch(14)` and 97 DB tests failing on the dev database (CI starts from an
  empty database). No other database affected (the box does not have `0012`). Repaired: schema already compliant,
  checksum updated in `_sqlx_migrations` (10 `ranked_results` rows kept).
  CI guard: `scripts/check-migrations.sh`.
- **Server**: `pending` uploads resumed at startup; archive written atomically; clean
  shutdown (SIGTERM, 0.5 s measured in the container); gzip/brotli compression; `/assets/*` in
  immutable cache; cast/draft writes guarded by `ADMIN_TOKEN` when it is set; DB errors
  no longer swallowed (token revocation `404`/`500`, teams/collections `500`, reprocess logged);
  admin token compared by digest; reference data retried every 10 min as long as it is empty.
- **Front**: pages loaded on demand (main bundle 535 → 240 kB); admin token sent
  by the control room and the draft (`authHeaders`); react-router 7, vite 8, vitest 5, dompurify 3.4.17.
- **storm-stats**: `process.rs` split into `src/process/` (7 files) with no logic
  change — outputs identical byte for byte on 114 replays, parity 114/114.
- **Docker/CI**: cargo-chef (rebuild after a code change: 91 s), non-root user
  (uid 10001) + `storm-codex-init` service that resets `/data` to that uid; a single workflow: fmt,
  frozen migrations, clippy/tests, `cargo audit` (clean; `rsa` ignored as it is never compiled),
  `npm audit`, then publication of `latest` + `sha-…` **only if everything is green**; `publish.yml`
  deleted; `cargo fmt` applied to the whole workspace.
- **Verified**: see the PR report (commands and results). **Not done**: deployment on the
  box (on the first deployment, `storm-codex-init` resets `/data` to uid 10001 automatically);
  the stray file `e399b92a…` at the root (not tracked by git) still has to be deleted by hand.
- **Deployment warning**: if `ADMIN_TOKEN` is set on the box, the `/cast` control room and the
  draft now require the token entered in Admin (otherwise `401`). The OBS overlays are not
  affected (read-only).

## Cast control room — batch replay import (2026-10-31, merged into `main`, PR #10)
Spec `docs/specs/2026-10-31-cast-batch-replay-import-design.md` (approved, amended 2026-11-01: per-file routes,
`series_changed` guard, Swap sides option). Verified: `cargo test --workspace` with Postgres, `tsc`, vitest.
**Not verified**: browser walkthrough of the batch import.

## Cast control room — usability (2026-10-31, merged into `main`, PRs #9 and #10)
Spec `docs/specs/2026-10-31-cast-usage-ux-design.md` (approved by the operator, recommended choices). Shipped in the working tree: `since_ms` (server rotation anchor, `POST /api/cast/slide`); permanent OBS URL + skin + copy; explicit going on air; steps and next action in the rail; plain-language errors; labels. Verified: clippy `-D warnings` green, `cargo test -p storm-codex-server` 254 passed, vitest 247 passed, `npm run build` OK. **Not verified**: manual walkthrough in the browser (spec criterion 3) and rotation measured over two tabs (criterion 5). Not deployed on the box.

## Cast control room — SHIPPED on 2026-10-27 (PR #5 merged into `main`; **not yet deployed on the box**)
Spec `docs/specs/2026-10-25-cast-control-room-design.md` · plan `docs/plans/2026-10-25-cast-control-room.md`.
A **series** (Bo1/3/5/7, Fearless/Standard/Normal, map pool, 2 teams × 5 players, logos) is the single source
of truth for the teams; the caster drives it from `/cast` and OBS reads `/cast/overlay`.
- **Server** `src/cast/`: `series.rs` (pure engine: veto in any order, games, computed/forced score),
  `store.rs` (singleton `cast_live`, migrations `0012_cast_live` + `0013_cast_logo`, fallback to a fresh series if the JSON is unreadable),
  `sync.rs` (series → draft, one way; starting a series aligns the draft engine, R20), `api.rs`
  (`/api/cast`, `/series`, `/teams`, `/pool/map`, `/games/start`, `/games/{n}/winner`, `DELETE /games/{n}`, `/score`, `/scene`,
  `/lineup-mode`, `/slide`, logos `/logo/{side}`, `GET /api/cast/lineup`), `lineup.rs` (3 recent heroes / 20 games,
  hidden < 5 games), `summary.rs` + `replay.rs` (`POST /api/cast/games/{n}/replay`: **Custom** replay decoded on
  the fly, **nothing written to the database**, Blue/Red orientation proven by toon then name, `?swap=1`). WS `cast.updated`.
- **Front**: `/cast` (control room: series/teams rail with a player picker, pool, preview = the overlay in an iframe,
  Lineups / Series / Draft / In-game tabs, Draft moved here; `/draft*` redirect to `/cast?scene=draft`),
  `/cast/overlay` (outside Layout, transparent background for OBS: `lineups` A/B scenes, `series` + post-game slideshow
  with 4 tabs, `draft`, `ingame` with tabs anchored on the 1920×1080 HUD, `hide`).
- **Measurements (release build, dev Postgres, 2026-10-27)**: scene → overlay updated **7–9 ms** typical, **37 ms** max
  (12 switches, DOM of a browser on 127.0.0.1, not OBS; WS `cast.updated` received in 4–6 ms; budget < 500 ms) ·
  `GET /api/cast/lineup` 10 players **median 5.9 ms, p95 8.1 ms, 1st call 40 ms** (budget < 100 ms) ·
  **post-game decoding: 417–444 ms** (full route, local upload of 1.6 MB included, Custom Tomb replay) — measured
  **under load** (a game was running, CPU ~68%); the margin to the 500 ms budget is **thin** and **no
  idle re-measurement was made**: we cannot separate machine load from a possible regression (e.g. building
  the task 9 summary). For the record, an earlier run during development (other replays, undocumented conditions)
  gave 178–203 ms for the route; `docs/spec/05-api.md` also cites a `storm-stats-dump --bench` bench (6 Custom
  replays, median 177 ms) — **different** measurements, not comparable with this one.
  **Measure on `127.0.0.1`, never `localhost`**: on Windows, `localhost` tries IPv6 first and adds ~2 s
  of fallback per request, which skews every timing.
- **Verified end to end** (real Custom Tomb of 2026-09-27, real toons): Bo5 Fearless series pool 10, veto out of order,
  game 1 + replay → lineups A→B ("awaiting game data" for unlinked players), post-game slideshow
  matching the raw storm-stats output (takedowns 28/15, damage, bans), live draft + replay, In-game scene on
  `ingame.webp` (anchors 39.20% / 59.65% / 9.90%, 40-character name truncated with "…"), server restart:
  cast **and** draft state identical byte for byte.
- **Fix wave (final review, 2026-10-27)** verified in the browser on 127.0.0.1 (release build):
  R21 "Remove game" then "Change winner" from `/cast` (map launchable again, draft score followed); R22
  Bo3 2-0 → "GAME 2" (In-game and Series); Draft scene rendered with the series inactive, Series hidden; WS reconnection:
  server stopped, scene changed **in the database** (no event), server restarted → `GET /api/cast` sent within the millisecond
  of the WS reopening, overlay updated ≈ 1 s after the restart (over 3 tries, the 10 s safety net / TanStack
  retries sometimes delivered the state before the WS — both paths lead to the same state). Not in OBS.
- **Fix wave 2 ("Fix it all", 2026-10-28)** — everything that was deferred or "ship as is", except the
  R26 design choices (below): **versioned** cast persistence (an old snapshot never overwrites a
  recent one) and `500 persist_failed` if the write fails (R24); draft sync on the **current** cast state; stable
  error codes (`CastError::code`); `store::load` distinguishes DB error / unreadable JSON / newer schema;
  logos: byte signature checked (`415 mime_mismatch`); lineup: toon > 64 bytes → `400 bad_toon`, `EXPLAIN`
  must name `match_players_toon_idx`, test cleanup guaranteed; slideshow validated (R25: existing `game` otherwise
  `409 UnknownGame`, `index` ≤ 99 otherwise `400 bad_index`, control room 1–3600 s); forts·keeps `null` without Fort/Keep;
  `series_id` re-checked after decoding a replay (`409 series_changed`); the draft's replay queue follows the
  format of an active series (M5); replay response with `orientation` and `forced_against_evidence` (note visible
  in the control room); `draft/replay/load` decoded like the cast replay (M2: `spawn_blocking` + guard + `parse_sem`).
  Front: overlay exit fade, strip of 11–14 maps on three rows, preview without image at 0.4, full
  tabs pattern (arrows = focus only), aria for maps/winners, rail resynchronised by the server version seen +
  `TeamEditor`, "last search" guard of the player picker, "loading…"/error in the lineups preview,
  "auto" label of the slideshow, "Swap sides" unticked after drop, Live/Replay styled, shared helpers.
  Wave 3: the overlay's outgoing layer keeps the SAME instance (stable layer id used as `key`,
  `sceneTransition`) — its slideshow tick is no longer reset during the fade; the replay also re-checks the
  map of game `n` (number reused after R21 → `409 series_changed`); `/slide`: `SeriesInactive` guard before
  `bad_index`; `500 pool|io` documented; v2/v1 persistence edge case documented.
  Verified in the browser (release build, 127.0.0.1:8089, DOM): pool of 14 maps = 3 rows without overlap
  (bottom at 589 px / 1080); scene change → outgoing layer kept ≈ 300 ms with the old scene then unmounted
  (opacities not observable: preview pane in the background, animations frozen); tabs (local `aria-selected`,
  tabpanel, → moves the focus without changing the on-air scene); Live/Replay styled in both sub-modes;
  player search: Find disabled during the request, typing during the request → no stale list. Not in OBS.
- **Deliberately not changed (R26, design choices for the operator to decide)**: a click on a scene tab
  puts it on air **and** opens its workspace; the control room's "Start game" does not advance the draft and does not set
  its map; no automatic proposal of the decider when only one map remains; Google Fonts loaded via
  `@import` (Fontsource = a dependency decision, consistent with the draft overlay); R7 logos kept by a
  new series; R18 Blackheart's Bay in the allow list; writes in open mode; `PoolPanel` outside the rail (R11).
- **Not verified / caveats**: scene → overlay time measured in a browser on 127.0.0.1, **not in OBS**;
  the In-game scene was checked on a **still image** in an iframe, not on a live capture; the Fearless
  dimming of the lineups was **not seen on screen** (only the "HEROES LOCKED" banner and the G1 labels in the DOM);
  draft-replay script and orientation **not seen on screen**; **no comparison with a match page** (a
  Custom replay has no `matches` row: compared with the raw storm-stats output; proof of "nothing written to the database" =
  matches/uploads/match_players counts identical before/after two uploads: 6593/2862/51050).
- **Tests**: `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex STORM_CAST_CUSTOM_REPLAY=<a Custom
  .StormReplay> cargo test --workspace` (without `DATABASE_URL`, the DB tests skip silently; without
  `STORM_CAST_CUSTOM_REPLAY`, the real-bytes test is skipped) → **271 passed, 3 ignored** (server: 227); after the
  final review fix wave: server **230 passed, 3 ignored** (`cargo test -p storm-codex-server`),
  web **201**; after wave 2: server **248 passed, 3 ignored** (+ real-bytes test Custom Tomb green with
  `STORM_CAST_CUSTOM_REPLAY`), web **238** (22 files);
  `cd web && npm test && npm run build` → 186 web before this wave; `cargo clippy --all-targets -- -D warnings` clean.
  The repo's DB tests leave fixtures in the dev database (one run: matches +454, uploads +10, match_players
  +2,912): **pre-existing** behaviour (mostly `players/`); the `cast/lineup.rs` tests also use the helper
  `insert_match` (a few rows, exact share not measured). Purge: `cargo test -p storm-codex-server purge_fixtures --
  --ignored`. The cast dev database (`cast_live`, `cast_logo`, `draft_live`) was **cleaned** after the check.
- **Known limits**:
  - *Draft*: **starting a series (`POST /api/cast/series`) resets the draft engine** (format/steps rebuilt,
    cursor 0, series bans emptied, replay game deselected, auto playback stopped) and **so clears a draft in
    progress** (R20); during an active series, the draft's "New series" is **disabled** (the series is managed from the
    rail); "Next game ›" stays active and only acts on the draft engine (Fearless bans accumulate) — the control room's
    "Start game" does not advance the draft.
  - *Replay of a game*: Blue/Red orientation by toon then name (R16); without proof, team 0 = Blue, but an
    entered winner that contradicts it → `409 side_mismatch`; `?swap=1` forced against contrary proof is **accepted
    but flagged** (`forced_against_evidence`, note in the control room); series replaced during decoding →
    `409 series_changed`; forts·keeps removed on maps without Fort/Keep; map objectives = allow list (R17), Blackheart's Bay kept on a thin corpus (1 game) —
    the operator may prefer to remove it (one constant + one doc line, R18).
  - *Slideshow* (R15): slide 0 = map strip, slides 1–4 = breakdown tabs (damage, healing, siege,
    takedowns), only slide 0 exists without a summary; in auto, the preview and the OBS source each have their own timer
    (the control room shows "auto", without claiming to know the slide on air).
  - *Overlay*: "GAME k" badge — R9 (game in progress, otherwise last + 1) and **R22** (series decided, one side at
    ⌈bo/2⌉: number of the last game); the Draft scene shows even without an active series (R19); the WS re-reads
    `cast`/`draft` at each reconnection (+ 10 s safety net); entry and exit fade ≈ 300 ms; In-game anchors =
    constants (`web/src/cast/anchors.ts`) to recalibrate if the game's UI changes; mode B only assigns a hero to a player
    linked by toon or exact name (never by position).
  - *Control room*: recoverable click error (**R21**): "Change winner" on a winner entered without a replay summary,
    "Remove game" (`DELETE /api/cast/games/{n}`) on the last game without a result; a winner coming from a replay
    can only be corrected by reloading the right replay. `POST /series` **keeps the existing logos** (R7, clear via `PUT /teams`); `PoolPanel` under the
    workspace and not in the left rail (R11). Database write failure = `persist_failed` banner (state applied in memory
    only, it will be lost at restart if the database stays unavailable).
- **Remaining**: final review then merge; **re-measure the post-game decoding on an idle machine before deployment**
  (417–444 ms measured under load); purge the fixtures from the dev database (`purge_fixtures`, see Tests); confirm in a real cast "replay team 0 = Blue" when there is no player proof; **deployment on the
  box** (rsync + docker compose build, migrations `0012_cast_live` **and** `0013_cast_logo`) and a trial on a real OBS
  broadcast; decide with the operator on rewriting history for the old `ingame.webp` capture (commit `cf372fa`,
  real names visible; the current version is blurred under the HUD strip).

## Draft from a replay — SHIPPED (2026-10-25, merged into `main` PR #4, not yet deployed)
Spec `docs/specs/2026-10-25-draft-replay-design.md`, plan `docs/plans/2026-10-25-draft-replay.md`, mockups
`docs/draft-replay-mockup.html`. Page `/draft/replay` (remote control) + existing OBS overlay: we load
**Custom** replays (nothing written to the database), the draft is prepared hidden then revealed **phase by phase** (12
Standard phases), by hand (Space) or in auto (**30 s per phase**), with a replay queue and **fearless**.
Verified on 3 real Custom replays (27/09/2026): loading 178–340 ms (release, local upload included, budget
< 500 ms), scripts matching the parser's `turn` (anonymised fixture committed), fearless (10 picks of
game 1 unavailable in game 2), auto-play at 2 s sampled, overlay empty → filled → "ready" after Reset.
Tests: 118 Rust (8 of them integration tests on local Postgres) + 101 web. Assumption to confirm in a real cast:
team 0 = Blue, team 1 = Red. Also fixed along the way: clippy lint `chunks_exact` (storm-replay-viewer) that
was failing `main`'s CI. **Not done**: manual editing of team names from this page (use `/draft`).
Custom replays without picks (games without a draft) → refused (`no_draft`).

## Player search + linked accounts SHIPPED on 2026-10-25 (merged into `main`)
Spec `docs/specs/2026-10-25-recherche-joueurs-design.md` (approved + amended on 2026-10-25) · plan
`docs/plans/2026-10-25-recherche-joueurs.md`.
- **Server** `src/players/`: `names.rs` (**dated** name history — current name = last seen in
  time, not `players.last_name`), `search.rs` (`GET /api/players/search`, old names included, `_`/`%`
  literal, p95 **20–26 ms** on 2,723 games / 7,618 players), `similarity.rs` (**pure** scoring),
  `features.rs` (cosine, talents, overlaps), `identity.rs` (token owners, pre-selection
  bounded to 30, ~400 ms/player), `api.rs` (`GET …/identity`, `PUT …/links/{other}` 🔒). Migration `0011_player_links`.
- **Front**: `/players` page, Stats / Identity tabs on `/player/:toon`, "Also plays as X →" line
  (confirmed link or HIGH suggestion only). Verified in the browser (search for an old name,
  Identity tab, Confirm → confirmed link, suggestion removed).
- **Deviations from the spec, reported to the operator**: "same uploader" signal → "token owner"
  (main ≥ 50% + other accounts ≥ 10% never in a game with them); operator and owners
  excluded from the teammates signal; front route `player/:toon` kept + `players`.
- **Known limits**: players seen *only* in scouting not searchable (isolated tables); search
  not accent-insensitive; no "undo a decision"; weights/thresholds of `similarity.rs` = starting
  values — **to review with the operator** on real cases (many LOW "teammates + hours" suggestions
  at 40–49 in the large social circles; the HIGH ones seen are plausible, e.g. Argas ↔ lilgas 72).
- **Tests**: `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex cargo test -p storm-codex-server`
  (without it the DB tests skip silently). Test fixtures: `cargo test -p storm-codex-server
  purge_fixtures -- --ignored`; measurement: `mesure_p95_recherche`, `sonde_identite_corpus_reel` (`--ignored`).
- **Next step**: review of the thresholds with the operator, merge of the branch.

## Live in-game feed — server + `/live` page SHIPPED on 2026-10-24 (verified by simulator)
Spec `docs/specs/2026-10-24-feed-live-en-partie-design.md` (operator decisions of 2026-10-24) ·
plan `docs/plans/2026-10-24-feed-live.md` · measurements `docs/research/2026-10-24-tempwritereplay-live.md`.
- **storm-replay**: `StreamDecoder` (chunked decoding of the bare streams) + `*_events_raw()`.
  Splitting into 4 KB / random / byte by byte = whole decoding (mini-corpus).
- **Server** `src/live/`: `POST /api/live/chunk` (idempotent per offset, `409` on a gap, `413` beyond
  64 MB), `/end`, `GET /api/live?scope=self|allies|all&me=`, `DELETE` (admin guard), WS
  `live.updated` (stateless signal). Pure reducer; unit → hero table derived from the archive (90/90).
  A replay replayed in 4 KB blocks matches `details` (heroes) and storm-stats (deaths, takedowns,
  talents) on the 3 committed replays.
- **`/live` page** ("Live" nav) = `LiveGame` component: three visibility levels (default "Me +
  scoreboard"), unnamed abilities (key presses), event feed.
- **Criterion 2 measured**: chunk → `live.updated`, p95 **1.7 ms** (budget 200 ms). Operator: lobby
  taken into account only if its hash = session id (`X-Session` = SHA-256 of the battlelobby);
  "single human" inference restrained (tested on each block of a 10-player game).
- **Verified**: `examples/live_sim.rs` replayed the Sandbox game of 2026-10-24 (76 blocks) and a
  10-human game (259 blocks) against the release server + dev Postgres; page verified in the browser
  (idle, in progress, finished, three levels, 375 px). Finding: in Sandbox, the talent
  reset re-emits all the `TalentChosen` → the table keeps the last choice per tier.
- **`client-rs` watcher WRITTEN (2026-10-25)** — Hots-Overlay repo, `client-rs/src/live.rs`, commit
  `ac093e7` (matella/Hots-Overlay `main`): lobby → `/api/lobby`, full 4 KB blocks → `/api/live/chunk`
  (offset held per stream, `409` → resend, box unreachable → catch-up), end → queue + `/end`;
  game already finished at startup → nothing. 12 unit tests + 2 end-to-end runs (including a server
  cut and a watcher restart).
- **Spike done (2026-10-25)** on a 10-human **ARAM** game with the real watcher:
  `docs/research/2026-10-25-feed-live-spike-aram.md`. Orders of the **10** players present during the
  game; median delay **~5 s** for the abilities, **~28 s** for the tracker (the opposite of
  Sandbox); build readable mid-game from the **process path** (`Versions\BaseNNNNN`). **Criterion 3
  held** (final state = projection of the replay, 10/10 players).
- **Remaining**: spike point 4 (clock alignment with the in-game display); re-measure on a QM or
  SL; `X-Build` from the process path in `client-rs`; "in game" state of `/companion`
  once the page exists; naming of the abilities (step 2). **Not yet deployed on the box.**
- **Bug seen during the spike (outside live)**: in the projection of an ARAM replay, the heroes named in
  `deaths[].victim/killers` are not those of the game (`players[].hero` is correct) — to analyse
  in `storm-stats`.
- **Side finding**: the game of 2026-10-24 ran on build **98348** (2.57.0), newer than our
  tables (96477); heroprotocol had not published it (checked on 2026-10-24). Decoded correctly by the
  "last known protocol" fallback.

## "Dossier" design for the whole app — spec APPROVED on 2026-09-27, rolled out in batches
Spec `docs/specs/2026-09-27-design-dossier-app-design.md` (+ mockups). OBS overlays out of scope.
- **Batch 0 (foundations) — done**: self-hosted fonts (Fontsource, 132 KB latin), tokens in
  `theme.css`, global design system `src/ds.css` + `components/ds/`, new top bar,
  Scouting pages migrated (visually identical). `body` unchanged → OBS sources unchanged (only
  nuance: JetBrains Mono is now actually loaded where `.mono` already used it).
- **Batch 1 (core) — done**: Session, Matches, Match detail redone (all functions kept;
  level curve moved from uPlot to a stepped SVG, `LevelChart` deleted; uPlot stays as a
  dependency). Session: the coverage now follows the **operator's last game** (before:
  the archive's last game, even without them). Verified by headless Edge screenshots (real data).
- **Batch 2 (reference data) — done**: Heroes (hero wall, role), Hero (large portrait, maps, builds,
  patches), Player (hero pool, recent games), Maps (tiles on the map art). Patch HTML
  rendered by `SafeHtml` (DOMPurify fragment, no injected HTML string).
- **Batch 3 (analyses) — done**: Synergies, Trends (histogram per patch), Patch notes (dated list +
  detail with a table of contents), Hero changes, Leagues. `SafeHtml` is now the only insertion point
  for external HTML in the app (DOMPurify fragment, no injected HTML string).
- **Batch 4 (operator tools) — done**: Draft console and Admin moved to the design (labels in English,
  all functions kept). The OBS overlay `/draft/overlay` is not touched.
- **Rollout of the "dossier" design finished** (batches 0→4); the OBS overlays stay out of scope.
## Draft bans resolved on the match page — 2026-09-26
storm-stats stores the bans as a raw attribute code (`Crus`, `DEAT`…); the match page
showed them as is (seen on match 143). `dim_heroes` does not carry the `attributeId`
(missing from the HotsPatchNotes list) → new static route `GET /api/dim/hero-attributes`
(`attr.json` from storm-stats, a source already used by `dim.rs`) + `banHero` / `useDimHeroAttributes`
in `api.ts`. Draft block reorganised per team: picks then bans (name + portrait), `no ban` for
a skipped turn. Verified in the browser on 143, 1702, 69 (skipped turns), 1720 (custom), 1 (ARAM: no
block) and at 375 px. **Not yet deployed on the box.**
Side finding (CRLF of the migrations on a Windows checkout): fixed on `main` by `.gitattributes`
(`migrations/*.sql eol=lf`). Merged into `main` on 2026-09-27.

## Scouting reports — SHIPPED on `main` on 2026-09-26 (not yet deployed on the box)
Spec `docs/specs/2026-09-26-rapports-scouting-design.md` · plan `docs/plans/2026-09-26-rapports-scouting.md`.
Pages `/scouting` and `/scouting/:id`; routes `/api/scouting/*`; migration `0010` (dedicated tables:
the opponents' replays never touch `uploads`/`matches` — isolation integration test).
- **Part A** (draft of custom games): 3 real replays of the operator, **confirmed** by them.
- **Verified in the browser on these 3 replays** (release server :8089 + dev Postgres): ambiguous batch
  detected (series against the same opponent) → one anchor (player of the scouted team) → roster of the 5 expected, 3 games by
  the roster rule; facts identical to the confirmed drafts (first pick 2/3, 1st-phase bans
  Johanna/Falstad ×2, bans received Deathwing/Garrosh ×2, openings Dehaka/Hogger/Chromie, W/W/L);
  pack `.md` 19 KB + `.xlsx`; analysis written from the real pack (manual LLM step) imported
  through the import window: 16 claims, 0 without proof; re-import = overwrite.
- **Measured**: `GET /api/scouting/{id}` p95 5.9 ms (contract 100 ms).
- Tests: 25 scouting tests (DB integration included), 77 server, vitest 54; clippy `-D warnings` green.
- Infra fix along the way: `.gitattributes` forces LF on the migrations (a Windows CRLF
  checkout made sqlx fail with `VersionMismatch` against a database migrated with LF).
- **Still owed (operator)**: criterion D — give the real pack to **ChatGPT and Claude** and re-import
  their answers (the import tolerates prose around the JSON); criterion B — validate the detection on
  a larger batch (games of the opponent against several teams); box deployment
  (rsync + `docker compose up -d --build`, migration 0010 applies at startup).
- **Draft refocus (2026-09-26, operator request)**: the analysis becomes a **map-by-map draft
  plan** (`format_version: 2`: map choice, per map bans with phase / picks /
  expected opponent picks with the player / points to prepare, any-map plan); v1 is refused
  at import. New facts: heroes played against them with their record (`draft.faced.*`), player
  behind each pick per map; pack reordered (maps and draft before players). Verified in the
  browser on the series: v2 plan imported, 28 items, 0 without proof.
- **HTML export to share (2026-09-27, part G)**: "Export HTML" button → a standalone `.html` file
  (portraits + minimaps embedded only once as WebP via CSS classes, ~600 KB for the
  3-game series, 33/33 portraits). Pure rendering `web/src/reportHtml.ts` (vitest: escaping of the
  LLM text, data: URIs filtered, names instead of `pN`), loading `reportExport.ts`.
  Verified: real export from the button, rendering checked with headless Edge screenshots at 1400 px and
  in a 390 px iframe (phone). In dev, the portraits come from the box's HotsPatchNotes
  (`HOTSPATCHNOTES_URL` added to the preview config, read-only).
- **Pages in the "dossier" style (2026-09-27)**: `/scouting` and `/scouting/:id` take up the visual
  language of the export (single page with sections, draft tables per map, drop zone/roster restyled).
  Export unchanged. Verified with headless Edge screenshots of the real page (1400 px + 390 px iframe).
- Outside v1 (spec part F): match page / 2D viewer for scouting replays,
  bridge to the draft simulator.
- Local preview: `storm-codex-server` config in `.claude/launch.json` (release, port 8089, serves
  `web/dist`; 8088 may be taken by another session).

## ⚠ AWAITING OPERATOR APPROVAL — two specs dated 2026-09-08
Scope chosen by the operator, specs written and pushed, **nothing is implemented** (rule no. 0:
a design decision is approved before coding).
- `docs/specs/2026-09-08-github-pages-design.md` — 4 parts: doc site (mdBook), rustdoc of the
  crates, **WASM "drop a replay" playground** (100% in-browser decoding), static sharing
  of a match. **Measured technical go**: `cargo build -p storm-stats -p storm-replay-viewer
  --target wasm32-unknown-unknown --release` passes **without any change** (17.6 s) — the whole
  chain is pure Rust (`bzip2-rs`, `miniz_oxide`). Still to measure: the size of the `.wasm`
  (budget ≤ 3 MB gzip) and the decoding time in the browser (budget ≤ 1 s; 133 ms native).
- `docs/specs/2026-09-08-chat-de-match-design.md` — 4 parts: chat ↔ 2D viewer link (the
  pings' `point{x,y}` have always been decoded and never shown), search and
  filters, game context in the feed, cross-match search (migration `0010 match_messages`
  + `pg_trgm`, route `GET /api/chat`).
- **Open questions** listed at the end of each spec (privacy of the static sharing, observer
  messages discarded by the parser).

## Deployed on the box on 2026-09-01 — `/now-playing?reveal` variant
`main` merged and pushed (`642750c`), then rsync + `docker compose up -d --build` on matelab.
Checks from `docs/spec/09-operations.md` passed: containers healthy, `/api/health` 200, bundle
changed from `index-BmQMoQ9z.js` to `index-LjZbPmIC.js` (the hash matches the local build), `npr-card`
present in the served bundle, the box's `.env` intact, no error at startup.
**Still owed**: the browser check on real data required for a front route — it
requires the Spotify OAuth on the Orpheus side (section "Orpheus (music)" further down). The reveal has therefore
never been seen firing for real; first test to do = two skips within 2.6 s
(in-place retargeting, the transition the unit tests cannot judge visually).

## Companion live — plan 2 (server): SHIPPED (branch `feat/companion-live-serveur`, 2026-08-28)
Plan: `docs/plans/2026-08-28-companion-live-2-serveur.md`. The server exposes everything the companion
page will need. **Front = plan 3, `client-rs` watcher = plan 4** (the only piece that needs the gaming
PC). 6 tasks, each reviewed for spec+quality; **10 Important fixes folded in** (counted as 5 then
9 in earlier passes — both counts were wrong), all coming from the code of my own
plan and caught by the reviews — detail below.
- **Map inferred without entry**: `storm-lobby` extracts the `.s2ma` hashes from the blob and resolves them via
  a table derived from the archive (`src/maps.rs`, generated by `examples/derive_maps.rs`).
  **19 maps out of 19 covered, 116 hashes**, over 3,322 replays. Criterion kept: hash never
  observed on another map — cannot produce a wrong association. A stricter criterion
  had been considered (hash present in ALL the replays of its map) then dropped: measured once,
  while tuning the criterion, it only covered 9/19 maps (Blizzard republishes the
  `.s2ma` at each patch) — this criterion no longer exists in the code, so this figure cannot be
  replayed. Circular guarantee by construction (derived and validated on the same archive):
  documented on the `Lobby::map` field.
- **Migration `0009`**: `builds` (talent build library, `picks` in the exact shape of the
  parser) + `lobby_live` (singleton, modelled on `draft_live`) + BattleTag resolution index.
  The invariant "a single default build per hero" is held by a **partial unique index**.
- **Routes**: `POST/GET/DELETE /api/lobby`, `POST /api/lobby/{hero,map,teams}`, CRUD
  `/api/builds` + `POST /api/builds/from-match`. WS `lobby.detected` / `lobby.updated`.
- **Enrichment**: `name#discriminant` → `toon_handle` **against the archive itself**, then
  histories (games with/against you, win rates, favourite heroes) and your hero/map stats.
  **Batched** queries (LEFT JOIN LATERAL + `= ANY($1)`) instead of the plan's per-player loop.
  **Measured** (`/api/lobby` pass added to `backfill_bench.py` in the final review, then fixed in the
  last review before merge: the `GET` was measured *before* the first `POST`, so on empty
  memory — systematic `204`, a figure unrelated to serialising a real enriched state):
  **p95 ≈ 36 ms in steady state on `POST /api/lobby`** (decoding + SQL enrichment, median
  ~31 ms, max ~53 ms over 20 requests), against a 100 ms contract. `GET` serves memory once
  the enriched state is set (ten players, histories, build): p95 ~1.2 ms.
  ⚠️ **The cold path exceeds the 100 ms contractual budget, and this is not a startup
  anomaly to ignore**: measured at 165–169 ms reproducibly over 3 consecutive restarts
  (first `POST /api/lobby` after the server starts, before any other traffic). In real use there
  is exactly **one `POST /api/lobby` per game, with the server idle in between** (the box only runs
  in the evening) — **the cold path is therefore this endpoint's nominal path**, not the very first
  isolated call that a simple warm-up would make disappear. Still to investigate before production (connection
  pool pre-warmed at startup, or a budget to revisit specifically for this route).
- **Degradations held**: unreadable blob → `parse_failed` + manual selector, never an HTTP error
  nor an empty page; player never met → `toon_handle`/`history` at `null`, never a fabricated
  win rate; a transient `parse_failed` (file read while the game writes it) **does not overwrite**
  an already enriched lobby less than 2 minutes old.
- **Important fixes folded in** (all defects of my plan, caught in review — **10**, not
  5 or 9 as wrongly documented in earlier passes): the builds `update` committed the
  unmarking of the default on a 404; `meme_lobby` let an unreadable blob overwrite an enriched
  lobby; the operator was counted "with themself" in their own history; clearing the
  map still marked it "entered by hand"; the 6 h window of the replay↔lobby link
  was promised in prose and missing from the code; **(final branch review)** a rematch between the
  same ten players (league/scrim) was never ingested as long as the previous lobby stayed linked
  to its match; `from_match` never persisted `source_match_id`, making the provenance of any
  imported build unrecoverable; `mes_parties` could count the same opponent both "with"
  and "against" (join fan-out on an archive namesake, `DISTINCT ON` added); a NULL `hero`
  in the database made it answer 500 instead of the 422 already planned for missing talents; **(last
  review before merge)** a simple watcher restart that re-posted the already linked lobby blob
  destroyed the debrief (`match_id` overwritten with `null`, unrecoverable) — fixed by a SHA-256
  hash guard on the raw bytes of the blob, taking priority over all the other idempotence
  rules: a hash identical to the current state is always `unchanged`, linked included.
- **Check**: `cargo test --workspace` green, **42 tests** in `storm-codex-server` including pure
  tests on idempotence by BattleTags and by hash, team reassignment and the time
  window.
  **Clippy, stated exactly**: `-p storm-lobby --all-targets -- -D warnings` clean, and
  `-p storm-codex-server -- -D warnings` (lib + bin) clean. On the other hand `-p storm-codex-server
  --all-targets` **fails** — it includes the test module of `src/draft/mod.rs`, which carries 11
  `unwrap()`s. ⚠️ `clippy --workspace
  --all-targets` fails on 11 `unwrap()`s in the *tests* of `draft/mod.rs` — **pre-existing on
  `main`**, checked, out of scope.
- **Known leftovers**: `/api/admin/reprocess` does not call `lier_match` (exclusion judged legitimate:
  the backfill must not link an old match to today's lobby); no integration test against
  a database (project debt, no crate has any).


## Companion live — plan 1 (`storm-lobby`): SHIPPED + MEASURED (branch `feat/companion-live`, 2026-08-28)
Spec: `docs/specs/2026-08-27-companion-live-design.md` · Plan:
`docs/plans/2026-08-27-companion-live-1-storm-lobby.md`. New pure crate `crates/storm-lobby`:
decodes `replay.server.battlelobby` **alone** (without the `details` stream) to identify the players
of a game **in progress**, before the replay exists. Reopens decision no. 6 of the programme
spec ("no pre-game in V1"), by operator decision.
- **What the format gives, and does not give** (`docs/research/2026-08-27-lobby-format.md`):
  BattleTags **in clear text**, prefixed with a length **in UTF-8 bytes** (a Cyrillic tag is
  invisible to `strings` — an ASCII parser would lose a player without failing). **Missing**: toon
  handle, picked hero, map, mode, team field. Identity is therefore resolved by BattleTag against
  the archive (`match_players.name` + `data->>'tag'`), and hero/map become manual entries.
- **Parity measured on 3,322 replays, 25 builds** (`docs/research/2026-08-27-lobby-parity.md`):
  **GO on the matchmade modes — 100% of BattleTags AND 100% of teams over 2,710 games**
  (Storm League 1978/1978, ARAM 729/729, QM 3/3), i.e. 83.9% of the comparison base
  (2,710 / 3,231). The overall figure of 96.19% was **entirely** produced by the custom
  games (521 evaluated, 16.1% of the base: 398 exact on tags, 76%; and among these 398,
  115 exact on teams, 29%) — observers sit in the lobby and the order there does not carry the team. Failure
  mode checked **exhaustively** over the 3,231 replays: over-capture (3.81%), **never** a
  missing player, never a malformed decode (0% under-capture).
- **Limit to know**: the blob does not carry the mode → the parser cannot know it is reading a
  custom game. On a custom game with exactly 10 occupants and no observer, it will assign
  teams with confidence and be wrong in most of these cases — 283 of the 398 custom games
  with exact BattleTags have a wrong team (~71%). The report's recommendation: keep the
  inference (perfect on 83.9% of the comparison base) and offer a one-click manual swap
  on the product side.
- **Check**: `cargo test -p storm-lobby` green (oracle 3/3, robustness 4/4, team_rule 3/3).
  `cargo clippy -p storm-lobby
  --all-targets -- -D warnings` clean. ⚠️ `cargo clippy --workspace --all-targets` fails on
  `storm-codex-server` (11 `unwrap_used` in its *test target*) — **pre-existing on `main`**,
  checked, out of this branch's scope.
- **Remaining**: plan 2 (migration `0009`, module `lobby.rs`, routes `/api/lobby` + `/api/builds`, pages
  `/companion` and `/builds`) — to write now that hero/map are decided as manual;
  then plan 3, the `client-rs` watcher (Hots-Overlay repo), the only piece that needs the gaming PC.


## 2D viewer — in-game minimap backgrounds + calibration (branch `feat/minimap-backgrounds`, 2026-07-09)
Spec: `docs/specs/2026-07-09-minimap-backgrounds-design.md`. Replaces the painted loading-screen art (busy)
with the **real in-game tactical minimap** (clean top-down) → clearly better readability.
- **Assets**: 15 minimaps extracted from the game by the operator (CascView failed → direct download), downscaled
  to 1400px, slug-named, **committed** in `assets/minimaps/` (5.2 MB) and **baked into the image** (Dockerfile COPY
  + `MINIMAPS_BUNDLE_DIR`); `bundle_minimaps()` copies them into `IMAGES_DIR/minimaps` at startup (idempotent)
  → served at `/images/minimaps/<slug>.jpg`. No runtime fetch (baked fallback).
- **Front**: `minimapImage()`/`mapSlug()`; background **priority minimap → painted art → gradient**. The minimap is
  drawn STRAIGHT in the canvas; the overlays (heroes/structures/minions/deaths) are **PROJECTED** via a
  **per-map similarity** (rotation+scale) anchored on the 2 cores — the game's coordinate frame is rotated/stretched
  vs the image (Cursed Hollow ~20°). Core anchors per map in `MAP_ANCHORS`.
- **Calibration**: **13 battlegrounds calibrated** (Cursed Hollow refined + verified live; 12 measured on
  the image, spot-checked Dragon Shire/Braxis/Towers of Doom — orientation OK, cores near the bases). Haunted
  Mines (1 game) + Blackheart's Bay (5) → full-image fallback. **Refinable** by eye via the dev hook
  `window.__cal("<slug>",[bx,by],[rx,ry])` on a real game (box: all maps present).
- **Check**: local E2E (Cursed Hollow, Braxis, Dragon Shire, Towers of Doom…) — clean minimap + calibration.
  Workspace tests + vitest green. **Remaining: merge + box redeployment** (readability + calibration).
- **Idea (backlog, operator 2026-07-09)**: **interactive in-app calibration mode** — instead of measuring the
  anchors on the image / `window.__cal`, allow **dragging the 2 core points with the mouse** on the minimap
  to place them to the pixel, live preview, then save (persist `MAP_ANCHORS` on the server/settings side).
  Natural extension of the `window.__cal` hook already in place. Not implemented (idea).

## 2D viewer — fast-follows (Phases 2–5): SHIPPED + VERIFIED E2E (branch `feat/replay2d-fastfollows`, 2026-07-09)
Plan: `docs/plans/2026-07-09-visionneuse-2d-fast-follows.md` (9 batches, reviewed 2×). **8 feature batches shipped,
each reviewed for spec+quality (findings folded in)**: animated playback play/pause+speed (US-11); structures
alive/destroyed (US-16); clickable kill-feed → seek (US-20); ability flashes (US-18); talents/levels +
tier strip (US-19/US-27); per-map objectives + isolated framework + gap-map warnings + Braxis waves
(US-21→24); minions/camps toggle (US-26); webm clip export (US-25). `VIEWER_VERSION` 1→7 (extended schema,
backward-compatible: the front ignores unknown fields; `minions` excluded from the golden to avoid bloating it).
Everything stays **100% client-side seek**, no network request per scrub. Crate `storm-replay-viewer` still pure
(geometry/time, zero reference data; hero/talent names resolved on the front via `players[]`+`dim_*`).
- **Real bugs caught along the way**: `levels` determinism (sort (t,team,level) — otherwise a flaky golden under
  `--workspace`, same class as serde_json/preserve_order); talent mislabel in mirror ARAM (key by player
  name, not by hero); MediaRecorder leak on unmount; minion coords clamp; zerg typing via Born
  (SUnitDiedEvent does not carry `m_unitTypeName`).
- **Local E2E check (2026-07-09, read-only box)**: dev PG + server (new crate) + real images +
  Cursed Hollow & Braxis replays. Proven via browser preview: animated playback (dots move), structures
  on the map + destructions in the feed, **clickable kill-feed (seek to 9:56 confirmed)**, talent strip
  ("Blue lvl 11 · Red lvl 10", cells per tier), minion toggle (dull dots), **Braxis objectives in the
  feed** (⚡ Zerg wave 51/61/63/13 at 3:49/11:03/15:36/23:05), clip-export UI (Set in/out/Export), 0 console
  errors. v7 endpoints OK (Braxis: 4 zerg_wave, 0 warnings; Cursed Hollow: 94 structs, 58 events).
- ⚠️ **Known leftovers**: Braxis US-21 = best-effort inference **not covered by an automated test** (no
  Braxis replay committed; validated on 1 real replay). Clip export = real recording not exercised in
  headless (UI + logic tested; MediaRecorder = browser). `generic.rs` inline (no separate file).


## 2D replay viewer — MVP-1: shipped + VERIFIED (calibration confirmed) + merged into main (2026-07-09)
New feature (outside milestones 0→6): **"Replay 2D"** tab in the match detail — replays the
heroes' positions on the minimap with an instant-seek scrub (alive/dead state + death
markers; **no HP/mana** — data missing from the replay; no play/pause animation). Branch
**`feat/replay2d`** (merged into `main`).
- **Spec**: `docs/specs/2026-07-09-visionneuse-2d-replay-design.md` (approved, reviewed). **Plan**:
  `docs/plans/2026-07-09-visionneuse-2d-replay.md` (reviewed in 2 passes).
- **Code shipped (3 batches, each reviewed for spec+quality)**: ① crate `crates/storm-replay-viewer`
  (events→normalised `[0,1]` JSON model extraction, authoritative player mapping via
  `SPlayerSetupEvent`, golden test); ② endpoint `GET /api/matches/{id}/replay2d` (on-demand
  decoding + disk cache, reuses the `raw.rs` path, player meta joined from Postgres);
  ③ front canvas tab (`web/src/components/Replay2D.tsx` + `web/src/replay2d.ts` pure seek tested with
  vitest).
- **Real bugs caught along the way**: `m_firstUnitIndex` seed of the `SUnitPositionsEvent`s; repaint of the
  portraits on load; **golden test fragile to `serde_json/preserve_order`** (failed under
  `cargo test --workspace` through feature unification — fixed with a semantic comparison of `Value`).
- **Local E2E smoke OK (2026-07-09)**: dev Postgres + server + upload of the `silver-city-aram` replay
  → parse → `/replay2d` (HTTP 200, 499 KB, 10 heroes + samples + 21 deaths + 10 named players, coords
  all ∈ [0,1], cache written). Front: the tab renders 10 markers coloured by team + a legend with the real
  names, **the scrub repositions the heroes**, **0 network requests per seek** (100% client seek proven),
  gradient fallback on a map without an image OK, no console error.
- **Calibration verified (2026-07-09)** on a **map with an image** — read-only approach (no change to the box):
  1 Cursed Hollow replay + the 15 map images pulled from the box (`docker cp` → `/tmp` → scp/rsync,
  cleaned up), replayed in a local server. Result: **background = real Cursed Hollow image**, 10 heroes
  **well placed and separated by team** (blue on the left / red on the right at spawn, consistent with the
  map geometry), **orientation/Y flip correct — no correction needed**, the scrub animates
  the positions plausibly (teamfights, Dehaka solo-laning at the bottom…), 0 console errors. Portraits
  as initials only because `dim_heroes` is not replicated locally (no HotsPatchNotes) —
  on the box it is → real portraits. **Feature merged into `main`** (merge `feat/replay2d`).
- **Remaining (operator, optional)**: deploy on the box (rsync + `docker compose up -d --build`,
  `--exclude .env`) to have it live; that is the only "remaining" item. Runbook still valid:
  `docs/runbooks/2026-07-09-visionneuse-2d-verif-box.md`.

## History (2026-06-12 → 2026-08-03)
The sections of this period (milestones 0→5, box deployment, D1/D3, images, widget, test
campaign, August audit, living spec, chat) are in [`STATUS-historique.md`](STATUS-historique.md).

## Milestones (summary — detail and criteria in the spec)
0 spike **GO ✅** → 1 storm-replay **✅** → 2 storm-stats **✅** (+ 2.5 ARAM maps) →
3 server+DB+backfill **✅** → 4 front parity **base ✅** → 5 stream+Jarvis+cutover **code ✅ +
deployed on the box (non-invasive) ✅** → 6 publication **ready** (publish = operator action).

## Locked decisions (do not reopen without the operator)
Rust (**confirmed by the spike** — .NET fallback dropped) · Postgres · Nexus Codex design ·
replacement of the local Node server (Twitch Azure EBS kept in V1, fed by push) · V1 =
full parity · no pre-game · no data migration (backfill + manual recreation)
· name Storm Codex.

## Blockers / operator needs
- ~~Replay sample for milestone 0~~ — resolved: milestone 0 ran on the gaming PC
  (2,652 local replays, reproducible corpus via `spike/sample_corpus.ps1`).
- Creation of the public GitHub repos (storm-replay/storm-stats) at milestone 6 — until then,
  everything lives in this repo.
