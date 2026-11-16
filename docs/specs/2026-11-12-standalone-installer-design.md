# Standalone installer — design

Status: **approved by the operator, 2026-11-12.** Date: 2026-11-12.

## Intent

Today Storm Codex needs a box: the Axum server, Postgres, Redis, and a separate uploader (`client-rs`) on the
player's PC. That is fine for the operator, not for other players.

Goal: give a player **one installer**. After installing, they start "Storm Codex" and everything works on their own
computer, with nothing to configure and nothing shared with anyone else: the Rust web pages, the server, the
database and the replay uploader. Each install holds only that player's own replays and stats.

## Goals

1. One Windows installer. The user installs, starts the app, and sees their stats.
2. The first launch finds the HotS replay folders by itself and imports everything automatically, in the
   background, with a progress screen. The user can browse as soon as the first replays are in.
3. Updates are one click: the app checks at startup, downloads and verifies the new version in the background,
   then shows a "Restart to update" button.
4. The OBS overlays and sources keep working from the app (they are browser sources at `http://localhost:<port>/...`).
5. Fully offline except for update checks. Nothing leaves the PC (no telemetry).
6. A player's data is never lost by an update, a reinstall, a crash or a PC shutdown.

## Non-goals

- No link with the operator's box: no Redis, no Jarvis events, no shared database, no Twitch Azure EBS.
- No macOS or Linux build in this version.
- OBS always runs on the same PC as the app (operator decision). The app listens on loopback only; there is no LAN
  option and no per-install token.
- No move to SQLite now (tracked in issue #31).
- No rework of the reference-data source (HotsPatchNotes). It is a separate project, done after this one (see
  "Reference data").
- No Windows code-signing certificate in v1.

## Anti-goals (what would make this a failure even if it technically works)

- The user must open a terminal, edit a file, install Postgres, or choose a port.
- A second launch starts a second server and Postgres.
- An update or a crash corrupts or empties the user's database.
- The OBS URLs shown in the app do not match the running port.

## Decisions taken with the operator

- **Shape:** a Windows-only Tauri desktop app, one installer. It bundles the Rust web pages (`web-rs/dist`), the
  server, a portable Postgres, and the replay watcher.
- **Closing the window** sends the app to the tray; the server keeps running so OBS keeps working.
- **Database:** Postgres stays, bundled and managed by the app (no Windows service, no admin rights). SQLite is
  deferred (issue #31): a move would rewrite most of the ~174 queries (JSONB, `LATERAL`, `DISTINCT ON`, array
  parameters, `UUID`).
- **Postgres major version is pinned.** A later major bump needs a dump-and-restore step and is separate work.
- **Not included in the standalone build:** Redis and the Jarvis events.
- **First launch:** automatic replay-folder detection and import, with a progress screen. The folders can be
  changed later in settings.
- **Updates:** the Tauri updater. Check at startup, download and verify in the background, "Restart to update"
  button. User data lives outside the install folder and is never touched; migrations run at startup.
- **Hosting of releases and update manifest:** GitHub Releases (guess, see "Left to the builder").
- **Reference data (heroes, talents, patches, portraits, map images):** the installer ships a bundled snapshot in
  the format the server already ingests (`referential.tar.gz`, `dim::ingest_snapshot`), made once at release time
  from the current HotsPatchNotes API. The app seeds from it on first launch with no network. Newer data arrives
  with each app update. A later project (HotsPatchNotes rewritten in Rust, with a nightly build that publishes a
  new snapshot when a game build or patch notes change) will publish a snapshot at a stable URL; the app will then
  be able to fetch fresher snapshots in the background without an app update. That runtime fetch is not part of v1.

## Behaviour

### First launch
1. The app creates its data folder, initialises Postgres, runs the migrations, seeds the reference data from the
   bundled snapshot.
2. It looks for the replay folders (`Documents/Heroes of the Storm/Accounts/*/*/Replays/Multiplayer`, including a
   relocated Documents folder) and starts the backfill (~3,000 files, 2–3 GB on the operator's PC).
3. A progress screen shows files done, skipped, and remaining. The user can already open the pages.
4. Corrupt or partial replays are skipped, listed once at the end, and never stop the import.
5. Free disk space is checked before the backfill; the screen shows where the data lives.

### Normal run
- The watcher picks up new games and uploads them to the local server (what `client-rs` does today).
- The server picks a free port if the default is taken. The OBS URLs shown in the app are built from the running
  port.
- Only one instance runs; a second launch focuses the existing window.
- Postgres is stopped cleanly when the app quits, the PC shuts down, or the app is killed. After an unclean exit,
  the next start cleans a leftover lock file or orphaned Postgres process before starting.
- Optional setting (default decided by the operator): start with Windows, so the watcher catches replays even when
  the app was not opened.

### Update
- At startup the app checks the update manifest. If a newer signed version exists it downloads it in the
  background and shows "Restart to update". The click stops the server and Postgres, installs, and restarts.
- Running a newer installer over an older install also works and keeps the data.
- When the replay parser changes (`parser_version`), the stored replays are re-processed in the background with the
  existing idempotent re-process.

### Uninstall and backup
- Uninstall asks whether to keep or delete the data folder.
- A one-click "export my data" produces a file that can be imported on a new PC.

### Privacy
- No telemetry. The only outgoing requests are the update check and the optional portrait/image fetches the pages
  already make. A short statement in the app says so.

## Constraints

- Everything is in English (code, docs, UI), per the project rules.
- Rust first. The Tauri shell, the embedded server and the watcher are Rust. The existing front-end bundles are
  reused as they are.
- Spec-first: this document, then the matching `docs/spec/` sections (operations, architecture, front-end) in the
  same commits as the code, and `docs/STATUS.md`.
- Existing perf budgets still hold (parse < 150 ms/replay, API p95 < 100 ms, backfill 3 years < 5 min).
- Library APIs (Tauri v2, its updater plugin, any embedded-Postgres crate or portable binaries) are checked with
  Context7 before use.
- The updater needs a signing key pair; the private key is a GitHub secret held by the operator.

## Risks

- Windows SmartScreen will warn on the first install (no code-signing certificate in v1). Updates after that are
  not affected.
- Bundled Postgres adds about 60–100 MB to the installer and slows the first start.
- Postgres major-version bump needs a dump-and-restore.
- The release process needs access to the HotsPatchNotes API to produce the bundled snapshot; without it the
  snapshot would be stale.

## What already exists (checked 2026-11-12)

- **Uploader:** `client-rs` (package `hots-replay-client`) lives in the operator's private Hots-Overlay repo, not in
  this one. It is a binary, not a library, with modules `watcher`, `uploader`, `detector`, `live`, `tray`, `updater`,
  `settings`, `state`, and an Inno Setup installer. The builder brings the watcher/uploader/detector code into this
  repo (the operator owns both repos) and drops its own tray, updater and installer in favour of the Tauri app's.
- **Redis and Jarvis are already optional:** `REDIS_URL` is an `Option`, so the server runs without them. The
  standalone build simply never sets it.
- **Local binding and auth:** the server binds `127.0.0.1:8088` by default and `ADMIN_TOKEN` empty means the writes
  are open. On loopback only, that is acceptable for a standalone install.
- **No Postgres extensions:** the migrations use no `CREATE EXTENSION`; `gen_random_uuid()` is core Postgres, so any
  portable build of the pinned major (13 or later) works.
- **Pages and port:** the pages resolve their base at runtime (`route::init_base`) and call the API on the same
  origin, so a runtime port needs no front-end change.
- **The box keeps working from the same codebase:** the standalone app is a mode of this repo (a Tauri shell around
  the same server and bundles), not a fork. The Docker image and the box deployment are unchanged.

## Calls made while writing this (the operator can overrule)

- **Several accounts or regions:** same behaviour as today. Every replay found under the Accounts folder is imported
  and shown in one view; no per-account separation is added.
- **Replays on network or removable drives:** not detected automatically. Extra folders can be added in settings.
- **Disk space:** the backfill refuses to start below 2 × the replay folders' size plus 1 GB free, and says why.
- **Antivirus false positives:** a known risk of an unsigned bundle with a bundled Postgres; no mitigation in v1
  beyond documenting it.
- **Acceptance (v1 is done when):**
  1. On a clean Windows 11 user profile (VM), the installer runs, the app starts, and a real replay folder is
     imported with no manual step.
  2. A replay played afterwards appears in the app without restarting it.
  3. All six OBS sources render from `http://localhost:<port>`.
  4. An update from version N to N+1 keeps all data and the migrations run; killing the app or the PC mid-import
     loses no data and the import resumes.
  5. The existing perf budgets hold on the operator's PC; first-start time and installer size are measured and
     recorded, with no hard cap in v1.

## Left to the builder

- How Postgres is obtained and embedded (portable binaries vs a crate), and its version pin.
- Whether the server runs in the Tauri process or as a sidecar process.
- The default port, how the free port is chosen, and how the front-end learns the port.
- Where `client-rs`'s watcher code lives (merged into this repo vs a shared crate) and how it talks to the local
  server.
- Release pipeline details (GitHub Actions job, artifact names, manifest format), within the decisions above.
- Whether "start with Windows" is on or off by default (operator decides).
- The exact layout of the progress screen and settings page, within the existing Nexus Codex design system.

## Out of scope, follow-up projects

1. HotsPatchNotes rewritten in Rust (most of its current code dropped), with a nightly build that publishes a new
   reference snapshot at a stable URL: tracked in matella/HeroesOfTheStormPatchNotes#1.
2. Runtime snapshot refresh in the app, once that URL exists.
3. SQLite instead of bundled Postgres (matella/storm-codex#31).
4. A code-signing certificate for the installer.
