# Standalone Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One Windows installer that gives a player Storm Codex running entirely on their own PC: pages, server, Postgres, replay import and watcher, one-click updates.

**Architecture:** A Tauri v2 shell (`desktop/`) supervises two child processes: a bundled portable Postgres 17 and the existing `storm-codex-server` started in a new *standalone* mode, then opens a window on `http://127.0.0.1:<port>/`. The replay watcher/uploader (ported from `client-rs`) runs **inside the server** in standalone mode, so import progress, folders and settings are ordinary API endpoints consumed by the existing Leptos app; only update/restart/export/autostart need the shell (Tauri commands). A supervisor library crate holds all the testable process/data-dir logic.

**Tech Stack:** Rust, Tauri v2 (`tauri-plugin-updater`, `-single-instance`, `-autostart`, `-dialog`), Postgres 17 portable binaries, `notify`, axum/sqlx (existing), Leptos 0.8 (existing), NSIS installer, GitHub Actions + Releases.

**Spec:** `docs/specs/2026-11-12-standalone-installer-design.md` (approved 2026-11-12). Read it first.

## Global Constraints

- Windows only, one installer, per-user install (no admin rights, no Windows service).
- Everything in English: code, comments, logs, UI, commit messages (project rule).
- Spec-first (rule 0): every task that changes a contract updates the matching `docs/spec/` section in the **same commit**; `docs/STATUS.md` at the end. Mapping: `04-serveur.md` (env, endpoints), `08-frontend.md` (routes), `09-operations.md` (standalone app), `01-architecture.md`, `10-developpement.md`.
- Postgres major version pinned to **17** (same as `docker-compose.yml` and CI). Redis and Jarvis are never configured (`REDIS_URL` unset).
- Server listens on loopback only: `127.0.0.1`. Default port 8088; if taken, a free port. OBS runs on the same PC only; no LAN option, no token.
- No telemetry. Outgoing requests: update check only (plus portrait fetches the pages already make).
- Reference data: bundled `referential.tar.gz` (existing format, `dim::ingest_snapshot`), seeded on first launch with no network. No runtime snapshot fetch in v1.
- Disk gate before backfill: refuse below `2 × replay folders' size + 1 GiB` free, and say why.
- Backfill skips corrupt/partial replays, lists them once, never stops.
- Existing budgets hold: parse < 150 ms/replay, API p95 < 100 ms, 3-year backfill < 5 min (operator's PC).
- Rust rules: `unwrap_used` denied, typed errors (`thiserror`), `cargo clippy --workspace --all-targets -- -D warnings`, conventional commits, commit trailer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Library APIs (Tauri v2 and its plugins, `notify`, free-space crate, Postgres Windows binaries) are checked with Context7 before use.
- Updater key: private key only in a GitHub secret held by the operator; public key committed in `tauri.conf.json`.

## Review Focus

Inputs the spec implies but no obvious task would exercise; each has a test in the owning task.

1. Windows user name or data path with spaces/accents (e.g. `C:\Users\Zoë Müller\AppData\Local\...`): Postgres init/start and the server must still work. (Task 2)
2. Documents folder relocated (OneDrive redirect) or no HotS install at all: detection uses the known-folder API and the empty case shows a clear "no replays found, add a folder" state, not an error. (Task 4, 6)
3. A replay still being written when the app starts or when the watcher fires: no corrupt upload, retried once stable. (Task 4)
4. App killed or PC shut down mid-import, then restarted: Postgres recovers, stale `postmaster.pid` cleaned, import resumes without re-uploading finished files. (Tasks 2, 4, 7)
5. Update requested while the backfill runs: graceful stop of server then Postgres, resume after restart. (Tasks 7, 8)

---

## File Structure

- Create `crates/storm-codex-supervisor/` (workspace member): `ports.rs`, `datadir.rs`, `disk.rs`, `postgres.rs`, `backup.rs`. Pure process/data logic, no Tauri.
- Create `crates/storm-codex-watcher/` (workspace member): `detector.rs`, `ledger.rs`, `upload.rs`, `backfill.rs`, `watch.rs`. Ported from `Hots-Overlay/client-rs/src/{detector,watcher,uploader}.rs` minus UI/tray/updater.
- Modify `crates/storm-codex-server/`: `config.rs`, `dim.rs`, `main.rs`, new `standalone.rs`.
- Modify `web-rs/app/`: new `pages/welcome.rs`, settings section, update banner.
- Create `desktop/` (Tauri v2 app, **excluded from the workspace** like `spike/*`, own lockfile): `src/main.rs`, `src/commands.rs`, `tauri.conf.json`, `capabilities/`.
- Create `scripts/fetch-postgres.ps1`, `scripts/make-referential.sh`, `desktop/referential-pin.txt`, `desktop/postgres-pin.txt`.
- Modify `.github/workflows/ci.yml`; create `.github/workflows/release-desktop.yml`.

---

### Task 1: Supervisor crate — ports, data dir, disk gate

**Files:** Create `crates/storm-codex-supervisor/{Cargo.toml,src/lib.rs,src/ports.rs,src/datadir.rs,src/disk.rs}`; modify root `Cargo.toml` (members).

**Interfaces — Produces:**
- `pub fn pick_port(preferred: u16) -> std::io::Result<u16>` — `preferred` if bindable on 127.0.0.1, else an OS-assigned free port.
- `pub struct DataDir`; `DataDir::new(root: PathBuf) -> Self`; `ensure(&self) -> io::Result<()>`; `pg(&self)`, `archive(&self)`, `raw_cache(&self)`, `images(&self)`, `logs(&self)`, `ledger(&self)` → `PathBuf` (`root/{pg,archive,raw-cache,images,logs,uploaded.json}`).
- `pub fn required_free_bytes(replay_bytes: u64) -> u64` = `2 * replay_bytes + 1_073_741_824`.
- `pub fn check_free_space(path: &Path, replay_bytes: u64) -> Result<(), DiskError>`; `DiskError::Insufficient { free: u64, needed: u64 }`, `DiskError::Io(io::Error)`. Free space read with a crate such as `fs4` (verify with Context7).

- [ ] **Step 1: Write failing tests** in each module: `pick_port_returns_preferred_when_free`, `pick_port_falls_back_when_taken` (hold a listener on the preferred port, assert result differs and is bindable), `datadir_ensure_creates_all_subdirs` (temp dir), `required_free_bytes_is_twice_plus_one_gib` (`required_free_bytes(10) == 20 + 1_073_741_824`), `check_free_space_reports_insufficient` (ask for `u64::MAX / 4` replay bytes → `Insufficient`).
- [ ] **Step 2:** `cargo test -p storm-codex-supervisor` → fails (crate missing).
- [ ] **Step 3:** Implement the signatures above; add the crate to workspace members.
- [ ] **Step 4:** `cargo test -p storm-codex-supervisor` → pass; `cargo clippy -p storm-codex-supervisor --all-targets -- -D warnings` clean.
- [ ] **Step 5:** Commit `feat(supervisor): ports, data dir, disk gate`.

### Task 2: Supervisor — Postgres lifecycle

**Files:** Create `crates/storm-codex-supervisor/src/postgres.rs`; test `crates/storm-codex-supervisor/tests/postgres_it.rs`.

**Interfaces — Consumes:** `DataDir`, `pick_port`. **Produces:**
- `pub struct Postgres { bin_dir: PathBuf, data_dir: PathBuf, port: u16 }`; `Postgres::new(bin_dir, data_dir, port)`.
- `init_if_needed(&self) -> Result<(), PgError>` (`initdb -U storm -A trust -E UTF8 --locale=C`; no-op if `PG_VERSION` exists; refuses a data dir whose `PG_VERSION` is not `17`).
- `clean_stale(&self) -> Result<(), PgError>` — if `postmaster.pid` exists and its PID is not a running `postgres.exe`, delete the file; if running and it is ours, leave it.
- `start(&self) -> Result<(), PgError>` (`pg_ctl start -w -t 60`, logs to `logs/postgres.log`, listens only on 127.0.0.1:`port`), `stop(&self)` (`pg_ctl stop -m fast`), `ensure_database(&self, name: &str)`, `database_url(&self, db: &str) -> String`.
- `dump(&self, out: &Path)` / `restore(&self, file: &Path)` (`pg_dump -Fc` / `pg_restore --clean --if-exists`).
- `PgError` with variants for missing binary, wrong major version, command failure (with stderr), timeout.

Integration tests are skipped unless `PG_BIN_DIR` is set (same pattern as the projection test with `DATABASE_URL`).

- [ ] **Step 1: Write failing tests:** `init_start_stop_roundtrip`, `data_dir_with_spaces_and_accents_works` (root `Zoë Müller/App Data`), `stale_pid_file_is_cleaned_and_start_succeeds` (write a `postmaster.pid` with a dead PID), `start_after_hard_kill_recovers` (kill `postgres.exe` process, then `clean_stale` + `start`, data still there), `dump_then_restore_roundtrip`, `wrong_major_version_is_refused` (unit test, no binaries needed: fake `PG_VERSION` = `16`).
- [ ] **Step 2:** Run with `PG_BIN_DIR=<pg17>/bin cargo test -p storm-codex-supervisor --test postgres_it` → fails.
- [ ] **Step 3:** Implement with `std::process::Command` (no shell); hide console windows on Windows (`CREATE_NO_WINDOW`).
- [ ] **Step 4:** Tests pass; the unit test passes without `PG_BIN_DIR` on Linux CI.
- [ ] **Step 5:** `scripts/fetch-postgres.ps1` downloads the pinned Windows x64 Postgres 17 zip named in `desktop/postgres-pin.txt` (URL + SHA-256), verifies the hash, extracts `bin/ lib/ share/` only. Document it in `docs/spec/09-operations.md` (new section "Standalone desktop app").
- [ ] **Step 6:** Commit `feat(supervisor): bundled Postgres lifecycle`.

### Task 3: Server — standalone mode and bundled snapshot seed

**Files:** Modify `crates/storm-codex-server/src/{config.rs,dim.rs,main.rs}`; docs `docs/spec/04-serveur.md`.

**Interfaces — Produces:**
- `Config.standalone: bool` from `STANDALONE=1`; `Config.referential_file: Option<PathBuf>` from `REFERENTIAL_FILE`.
- `Config::validate(&self) -> Result<(), String>`: in standalone mode, `bind_addr` must parse to a loopback IP, else error `"standalone mode requires a loopback BIND_ADDR"`.
- `dim::ingest_snapshot_file(db: &PgPool, images_dir: &Path, file: &Path) -> Vec<(String, String)>` — same behaviour as `ingest_snapshot` but reads the tarball from disk. Refactor so both share one extraction/ingest path (`ingest_snapshot` keeps its signature).
- Startup: in standalone mode, if `dim_heroes` is empty and `referential_file` is set, seed from it once; the 24 h refresh loop is skipped (no network).

- [ ] **Step 1: Write failing tests** (`#[cfg(test)]` in `config.rs` and `dim.rs`): `standalone_rejects_non_loopback_bind` (`0.0.0.0:8088` → Err; `127.0.0.1:0` → Ok), `non_standalone_allows_any_bind`, `ingest_snapshot_file_seeds_heroes_and_patches` (tiny tarball built in the test with 1 hero, 1 patch; DB test skipped without `DATABASE_URL`), `ingest_snapshot_file_missing_file_returns_empty_without_panic`.
- [ ] **Step 2:** `cargo test -p storm-codex-server standalone ingest_snapshot_file` → fails.
- [ ] **Step 3:** Implement; call `validate()` in `run()` right after `from_env()`.
- [ ] **Step 4:** Tests + clippy pass. Update `04-serveur.md` env table (`STANDALONE`, `REFERENTIAL_FILE`) in the same commit.
- [ ] **Step 5:** Commit `feat(server): standalone mode, seed from a bundled snapshot`.

### Task 4: Watcher crate (port of client-rs)

**Files:** Create `crates/storm-codex-watcher/…`; read-only reference `C:\Users\matth\Desktop\Coding\Hots-Overlay\client-rs\src\{detector,watcher,uploader}.rs`.

**Interfaces — Produces:**
- `detector::detect_replay_dirs() -> Vec<PathBuf>` (uses the Documents known folder via `dirs::document_dir`, so OneDrive-relocated folders work), `detector::find_replay_dirs_under(parent: &Path) -> Vec<PathBuf>` (depth ≤ 8, dirs containing `.StormReplay`).
- `ledger::Ledger::open(path: &Path) -> io::Result<Ledger>`; `contains(&self, name: &str) -> bool`; `record(&mut self, name: &str, outcome: Outcome) -> io::Result<()>` (atomic write: temp file + rename); `Outcome::{Uploaded, Duplicate, Rejected(String)}`; `rejected(&self) -> Vec<(String, String)>`.
- `upload::Uploader::new(base_url: String) -> Self`; `async fn upload(&self, path: &Path) -> UploadResult` where `UploadResult::{Uploaded, Duplicate, Rejected(String), Retryable(String)}`. Protocol (copy from client-rs): `POST {base}/api/upload`, `Content-Type: application/octet-stream`, `X-Filename: <percent-encoded name>`; 2xx → Uploaded; 409 → Duplicate; other 4xx → Rejected(body); network/5xx → Retryable, retried with backoff `2^n` s, max 5 attempts, then reported as failed (not recorded, so the next start retries).
- `backfill::run(dirs: &[PathBuf], uploader: &Uploader, ledger: &mut Ledger, progress: &Progress)`: oldest-first by mtime, skips ledger entries, 4 concurrent uploads, updates `Progress { total, done, duplicates, rejected, failed }` (atomics + mutex'd lists).
- `watch::start(dirs: &[PathBuf], tx: tokio::sync::mpsc::UnboundedSender<PathBuf>) -> Result<WatchHandle, String>`: `notify` recursive; a path is emitted after 1 s without further events and only if its size is stable across two checks 500 ms apart.

- [ ] **Step 1: Write failing tests:** `detector_finds_nested_replay_dirs` (temp tree `Accounts/1/2-Hero-1-3/Replays/Multiplayer`), `detector_returns_empty_when_no_install`, `ledger_roundtrip_survives_reopen`, `ledger_write_is_atomic_on_crash_simulation` (leave a stale `.tmp`, reopen succeeds), `upload_maps_status_codes` (tiny axum test server returning 200/409/422/500), `upload_retries_then_reports_failure_without_recording`, `upload_percent_encodes_non_ascii_filename`, `backfill_skips_already_recorded_and_counts_rejected`, `backfill_continues_after_rejected_file`, `watch_waits_until_file_is_stable` (write in two chunks 1.5 s apart; exactly one event after the second).
- [ ] **Step 2:** `cargo test -p storm-codex-watcher` → fails.
- [ ] **Step 3:** Port and trim the client-rs code; no `eframe`/`tray-icon`/GUI deps; `notify` version per Context7.
- [ ] **Step 4:** Tests and clippy pass.
- [ ] **Step 5:** Commit `feat(watcher): replay detector, uploader, backfill and live watch`.

### Task 5: Server — standalone watcher wiring and status API

**Files:** Create `crates/storm-codex-server/src/standalone.rs`; modify `main.rs` (router + startup); docs `04-serveur.md`.

**Interfaces — Consumes:** Tasks 1, 3, 4 (`check_free_space`, `Uploader`, `backfill::run`, `watch::start`). **Produces** (all only registered when `Config.standalone`, JSON camelCase):
- `GET /api/standalone/status` → `{ phase: "seeding"|"importing"|"ready"|"no-folders"|"disk-full"|"error", folders: [string], total, done, duplicates, rejected: [{file, reason}], failed: [string], freeBytes, neededBytes, dataDir, version }`.
- `PUT /api/standalone/folders` body `{ folders: [string] }` → persists extra folders in `app_settings`, restarts the watcher, triggers a rescan; returns the new status.
- `POST /api/standalone/rescan` → 202.
- Startup sequence in standalone mode: seed (Task 3) → detect folders (`detect_replay_dirs` + saved extra folders) → disk gate (`check_free_space` on `ARCHIVE_DIR` with the folders' total size; on failure phase `disk-full`, no backfill) → backfill in a background task → live watcher. `/api/health` stays 200 throughout so the shell can show the window early.
- The watcher uploads to `http://127.0.0.1:<port>/api/upload` using the bound port.

- [ ] **Step 1: Write failing tests** (router tests with `tower::ServiceExt` as in `front_end_tests`): `status_is_404_when_not_standalone`, `status_reports_no_folders_phase_when_none_found`, `put_folders_persists_and_returns_status`, `disk_full_phase_blocks_backfill` (inject a `required` larger than free via a test seam `StandaloneState::with_free_space(fn)`), `status_counts_follow_progress` (set `Progress` values, read JSON).
- [ ] **Step 2:** `cargo test -p storm-codex-server standalone` → fails.
- [ ] **Step 3:** Implement; keep progress in `Arc<Progress>` inside `AppState`.
- [ ] **Step 4:** Tests + clippy pass; `04-serveur.md` documents the three endpoints and the startup sequence.
- [ ] **Step 5:** Commit `feat(server): standalone import orchestration and status API`.

### Task 6: Front-end — welcome/progress, settings, update banner

**Files:** Create `web-rs/app/src/pages/welcome.rs`; modify `web-rs/app/src/{main.rs,layout.rs}`, `web-rs/core/src/` (new `standalone.rs` types + pure helpers); docs `08-frontend.md`.

**Interfaces — Consumes:** Task 5 endpoints. **Produces:**
- `core::standalone::Status` (serde, camelCase, mirrors the JSON above) and pure `fn progress_percent(done: u64, total: u64) -> u8`, `fn phase_message(&Status) -> String`.
- Route `/welcome` (progress screen, also reachable from the nav while `phase != ready`); the app redirects to it on first load when `phase` is `seeding`/`importing`/`no-folders`/`disk-full`. When `/api/standalone/status` is 404 (box deployment) nothing changes.
- Settings section "This PC": replay folders list with add/remove (text input of a path; native picker only if the shell IPC exists), data location, the sentence "Nothing leaves this PC except the update check.", the OBS source URLs (existing component, must use the running origin).
- Update banner: if `window.__TAURI__` exists, polls the IPC command `update_status` (Task 8); when `ready` shows "Restart to update" → `invoke('install_update')`.

- [ ] **Step 1: Write failing tests** (native `core` tests): `progress_percent_handles_zero_total`, `progress_percent_caps_at_100`, `phase_message_for_no_folders_tells_user_to_add_one`, `status_deserializes_server_json` (fixture JSON from Task 5).
- [ ] **Step 2:** `cargo test` in `web-rs/` → fails.
- [ ] **Step 3:** Implement the pure helpers, then the Leptos components (follow existing page patterns, e.g. `pages/scouting_report.rs`; reactive closures must be `Send + Sync`, use `StoredValue::new_local` for web closures).
- [ ] **Step 4:** `cargo test`, `cargo clippy -- -D warnings`, `cargo clippy --target wasm32-unknown-unknown -- -D warnings`, `trunk build --release` in `web-rs/app`; size budget still passes (`SIZE_CODEC=br sh scripts/check-web-rs-size.sh web-rs/dist/app 1000000`).
- [ ] **Step 5:** Verify in the browser against a standalone-mode server with a temp replay folder: progress moves, `no-folders` state shows the add-folder hint, box mode (no `STANDALONE`) shows nothing new.
- [ ] **Step 6:** Commit `feat(web-rs): welcome screen, standalone settings, update banner`.

### Task 7: Desktop shell — supervise, window, tray, single instance

**Files:** Create `desktop/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json,src/main.rs,src/supervise.rs}`; add `desktop` to workspace `exclude`; docs `09-operations.md`, `01-architecture.md`.

**Interfaces — Consumes:** `storm-codex-supervisor` (path dep). **Produces:**
- `supervise::Stack::start(resource_dir: &Path, data_root: &Path) -> Result<Stack, StartError>`: `DataDir::ensure` → `Postgres::clean_stale` → `init_if_needed` → `start` → `ensure_database("storm_codex")` → `pick_port(8088)` → spawn `storm-codex-server.exe` from resources with env `STANDALONE=1`, `BIND_ADDR=127.0.0.1:<port>`, `DATABASE_URL`, `ARCHIVE_DIR`, `RAW_CACHE_DIR`, `IMAGES_DIR`, `WEB_DIR=<res>/web`, `MINIMAPS_BUNDLE_DIR=<res>/minimaps`, `REFERENTIAL_FILE=<res>/referential.tar.gz`, and **no** `REDIS_URL`/`ADMIN_TOKEN`; poll `/api/health` (timeout 60 s) → `Stack { port, .. }`.
- `Stack::stop(&mut self)`: server gets a graceful stop first (request `/api/standalone/shutdown`-less approach: terminate via the child handle after a 10 s grace using the server's existing Ctrl-C/`shutdown_signal` path on Windows through a console-less `GenerateConsoleCtrlEvent` alternative if Context7/testing shows it works; otherwise add `POST /api/standalone/shutdown` bound to loopback and use it), then `Postgres::stop`.
- Shell: data root = Tauri `app_local_data_dir()` (so the NSIS "delete app data on uninstall" option covers it); main window `WebviewUrl::External(http://127.0.0.1:<port>/)` created once health is OK (a tiny bundled `loading.html` shows meanwhile); close button hides to tray; tray menu: Open, Restart to update (hidden until ready), Quit (runs `Stack::stop`, then exits); `tauri-plugin-single-instance` registered first, focusing the existing window.
- Windows exit hooks: `RunEvent::ExitRequested` and a console-control/`WM_ENDSESSION` handler call `Stack::stop`.

- [ ] **Step 1: Write failing tests** for `supervise.rs` with a fake `Stack` seam (`trait Child { fn kill(); }`): `start_order_is_cleanup_init_start_db_port_server` (record calls), `start_fails_with_clear_error_when_server_never_healthy`, `stop_stops_server_before_postgres`, `env_has_no_redis_and_is_loopback`.
- [ ] **Step 2:** `cargo test --manifest-path desktop/Cargo.toml` → fails.
- [ ] **Step 3:** Implement; configure per Context7 (tray, single-instance, `WebviewUrl::External`); capability file for the main window `remote.urls: ["http://127.0.0.1:*"]` allowing only the commands of Task 8.
- [ ] **Step 4:** Manual check on Windows with a dev resource folder: app starts, window shows the app, closing hides to tray, second launch focuses the first, Quit leaves no `postgres.exe`/`storm-codex-server.exe` (`Get-Process`), `kill -9` of the shell then relaunch recovers (review-focus 4).
- [ ] **Step 5:** Commit `feat(desktop): Tauri shell supervising Postgres and the server`.

### Task 8: Updater, IPC commands, autostart, export/import

**Files:** Create `desktop/src/commands.rs`, `desktop/src/updater.rs`; modify `desktop/{Cargo.toml,tauri.conf.json,capabilities/default.json}`; docs `09-operations.md`.

**Interfaces — Produces** (Tauri commands callable from the page via the remote capability):
- `update_status() -> UpdateStatus` = `{ state: "none"|"downloading"|"ready", version: Option<String> }`. At startup and every 6 h the shell calls `app.updater()?.check()`; if an update exists it `download()`s in the background (not `download_and_install`), keeps the bytes, sets `ready`, un-hides the tray item.
- `install_update()`: `Stack::stop`, then `update.install(bytes)`, then `app.restart()` (restart relaunches the app; migrations run at next start). Installer mode `passive`.
- `export_data(path: String)`: native save dialog by the shell if `path` empty; writes a `.stormcodex` zip containing `db.dump` (`Postgres::dump`) and the `archive/` folder. `import_data(path: String)`: requires the app to have no data yet or explicit confirm; stops the server, `restore`, copies the archive, restarts the stack.
- `set_autostart(enabled: bool)` / `get_autostart()` via `tauri-plugin-autostart`; default **off** (`const AUTOSTART_DEFAULT: bool = false`).
- `tauri.conf.json`: `bundle.createUpdaterArtifacts: true`; `plugins.updater.pubkey` (operator-generated), `endpoints: ["https://github.com/matella/storm-codex/releases/latest/download/latest.json"]`, `windows.installMode: "passive"`.

- [ ] **Step 1: Write failing tests:** `update_state_machine_none_to_downloading_to_ready`, `install_is_refused_unless_ready`, `export_then_import_roundtrip_preserves_a_cast_row` (uses real Postgres when `PG_BIN_DIR` set), `export_zip_contains_db_dump_and_archive`, `autostart_default_is_off`.
- [ ] **Step 2:** `cargo test --manifest-path desktop/Cargo.toml` → fails.
- [ ] **Step 3:** Implement per Context7 (`UpdaterExt`, `download` then `install`, `app.restart()`).
- [ ] **Step 4:** Manual end-to-end with a locally served `latest.json` and two builds N and N+1: banner appears, click installs, data intact, migrations ran. Also trigger the click while the backfill runs (review-focus 5): after restart the import resumes and no file is re-uploaded.
- [ ] **Step 5:** If the remote IPC capability does not accept the dynamic port pattern in practice, fall back to the tray-menu "Restart to update" only and record it in `09-operations.md`.
- [ ] **Step 6:** Commit `feat(desktop): updater, export/import, autostart`.

### Task 9: Packaging and release pipeline

**Files:** Modify `desktop/tauri.conf.json`; create `scripts/make-referential.sh`, `desktop/referential-pin.txt`, `.github/workflows/release-desktop.yml`; modify `.github/workflows/ci.yml`; docs `09-operations.md`, `10-developpement.md`.

- [ ] **Step 1:** `tauri.conf.json`: `bundle.targets: ["nsis"]`, `bundle.windows.nsis.installMode: "currentUser"`, `bundle.resources` mapping `storm-codex-server.exe`, `postgres/` (from `scripts/fetch-postgres.ps1`), `web/` (`web-rs/dist`), `minimaps/` (`assets/minimaps`), `referential.tar.gz`, `loading.html`. Check key names against Context7.
- [ ] **Step 2:** `scripts/make-referential.sh`: builds `referential.tar.gz` (layout of `dim::ingest_snapshot`: `heroes.json`, `hero-details.json`, `patches.json`, `patch-details.json`, `images/**`) from the operator's HotsPatchNotes API; the operator uploads it as a release asset; `desktop/referential-pin.txt` holds its URL and SHA-256. Document the stale-snapshot risk and the follow-up (HotsPatchNotes#1) in `09-operations.md`.
- [ ] **Step 3:** `release-desktop.yml` (windows-latest, on tag `desktop-v*`): build the three `web-rs` bundles, `cargo build --release -p storm-codex-server`, run `fetch-postgres.ps1` and the referential download (both verifying SHA-256), `tauri build` with `TAURI_SIGNING_PRIVATE_KEY` from secrets, upload the installer, `.sig` and `latest.json` to the GitHub Release.
- [ ] **Step 4:** `ci.yml`: add a `desktop` job (windows-latest) running `cargo clippy --manifest-path desktop/Cargo.toml -- -D warnings` and its tests; supervisor and watcher crates already run in the existing `rust` job through `--workspace`.
- [ ] **Step 5:** Dry-run the release workflow on a test tag; record installer size and first-start time in `09-operations.md` (no cap, per spec).
- [ ] **Step 6:** Commit `ci(desktop): installer packaging and signed release workflow`.

### Task 10: Acceptance and docs closure

- [ ] **Step 1:** On a clean Windows 11 user profile (VM): install; start; real replay folder imported with no manual step (acceptance 1).
- [ ] **Step 2:** Play a game; new replay appears without restarting the app (acceptance 2).
- [ ] **Step 3:** All six OBS sources render from `http://localhost:<port>`: `/cast/overlay`, `/draft/overlay`, `/ticker`, `/now-playing`, `/widget`, `/queue` (acceptance 3).
- [ ] **Step 4:** Update N → N+1 keeps data; kill mid-import and relaunch resumes with no data loss (acceptance 4). Test a user profile whose name has spaces/accents (review-focus 1) and a machine with no HotS install (review-focus 2).
- [ ] **Step 5:** Measure perf on the operator's PC (parse, API p95, backfill) and record installer size and first-start time (acceptance 5).
- [ ] **Step 6:** Update `docs/STATUS.md`, `docs/spec/README.md` index; confirm every task's doc diff landed. Commit `docs: standalone installer status`.

---

## Self-Review

- **Spec coverage:** one installer (T9), first-launch auto import + progress (T4–T6), one-click update (T8), OBS on same PC / loopback (T3, T7), offline + no telemetry (T3, T6), data safety (T2, T4, T7), single instance and free port (T1, T7), clean shutdown/stale lock (T2, T7), autostart option (T8), uninstall keep/delete (T7 data root + NSIS option), export/import (T2, T8), disk gate (T1, T5), corrupt replays (T4), bundled snapshot (T3, T9), acceptance (T10). Non-goals (SQLite, signing cert, other OS, runtime snapshot refresh) are not planned.
- **Open points to settle while building (not spec questions):** graceful Windows stop of the server (Task 7 states the fallback endpoint), remote-IPC wildcard port (Task 8 fallback), exact Tauri NSIS key names (Context7).
- **Type consistency:** `Progress`, `Ledger`, `Uploader`, `DataDir`, `Postgres`, `Stack`, `Status` are defined once and referenced by the same names across tasks.
