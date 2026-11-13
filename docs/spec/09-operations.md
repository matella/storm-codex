# Operations — dev, deployment, backups

## Local dev (Windows gaming PC)

```bash
docker compose -f docker-compose.dev.yml up -d   # Postgres 17 :5433 + Redis :6380
cp .env.example .env                             # DATABASE_URL etc.
cargo run -p storm-codex-server                  # automatic migrations, :8088
```

Front-end (`web-rs/`, needs `rustup target add wasm32-unknown-unknown` and `trunk`): `trunk build --release` in `web-rs/overlay`,
`web-rs/sources` and `web-rs/app` (output in `web-rs/dist`), then run the server with `WEB_DIR=web-rs/dist`: every page is at `/`
(`/cast/overlay`, `/draft/overlay`, `/ticker`, `/widget`, `/queue`, `/now-playing` for the OBS sources), exactly as in production. The server
reads the pages at each request, so a rebuild needs no restart.

**Production:** the Docker image builds the three bundles (stage `web`, pinned Trunk) and sets `WEB_DIR=/app/web`. React was removed after
the switch of 2026-11-12 (last commit with it: `2134853`), so there is no `WEB_ROOT` rollback switch any more: rolling back means deploying
the previous image tag (`sha-<commit>`).

Tests: `cargo test` (the projection test is skipped without `DATABASE_URL`); parity:
`tools/parity-harness/`; benches: `storm-stats-dump --bench`, `backfill_bench.py`.

⚠️ **The local clone may be behind**: development also happens from the Mac/box.
`git fetch origin` **before** reading the code or planning; after a pull,
rebuild the bundles with `trunk build --release` if `web-rs/` changed.

## Production (box 192.168.129.85, `~/apps/storm-codex`)

- **Stack**: `docker-compose.yml` — dedicated Postgres `storm-codex-pg` (volume `storm-codex-pgdata`)
  + `storm-codex-server` (multi-stage build: Vite SPA → Rust release → Debian slim; volume
  `/data` = archive + caches + images). Host port **5102** (8088 is taken by gluetun).
  Healthcheck on `/api/health`.
- **Networks**: `default` + `jarvis_default` (external) to publish on jarvis-redis.
  `DATABASE_URL` points to the **container name** `storm-codex-pg` (the `postgres` alias collides
  with jarvis-postgres on the shared network).
- **Secrets**: only in the box's `.env` (never committed). `POSTGRES_PASSWORD` initialised
  the volume — recoverable from the running container (`docker inspect`).
- **Deploy**: rsync the repo → `docker compose up -d --build`. Or, for an isolated
  front-end/server change: fetch the files from GitHub (public repo) then rebuild — the
  Docker cache only rebuilds the affected stage.
  ⚠️ **NEVER `rsync --delete` without `--exclude .env`** (it would delete the box's `.env`).
- **Verify after deployment**: containers healthy, `/api/health` 200, bundle hash
  (`index-*.js` in `/`) changed.
- **Remote access**: Tailscale → 192.168.129.85 (SSH + :5102), in the evening when the box is running.
  The SSH keys exist on the Mac (not on the gaming PC).

## Uploader (gaming PC)

`client-rs` (**Hots-Overlay** repo): root `.env` with `SERVER_URL=http://192.168.129.85:5102`
+ `AUTH_TOKEN=<personal token>`. At launch: **complete** backfill of what has not been uploaded
(oldest-first, idempotent — persisted set + server 409), then watches the replays folder.

## CI — a single workflow (`ci.yml`): checks, tests, audit, then image publication

**Jobs** (push to `main`, `v*` tags, PRs, manual run):
- `checks`: `cargo fmt --all -- --check`; **frozen migrations** (`scripts/check-migrations.sh`):
  rejects any change, deletion or rename of a migration already present in the comparison base
  (the target branch for a PR, the previous commit for a push). sqlx checks the
  checksum of every applied migration: rewriting one makes startup fail
  (`VersionMismatch`) for every database that already applied it. A schema change = a
  **new** migration. Modifying a migration added in the same PR is still allowed.
- `rust`: strict clippy (`-D warnings`) + `cargo test --workspace` against a **Postgres 17
  service** (DB-gated integration tests actually run).
- `audit`: `cargo audit` (RustSec database) on `Cargo.lock`. Ignored advisories are listed in
  `.cargo/audit.toml`, each with its reason; only crates that are in the lockfile but compiled for
  no target qualify.
- `web-rs`: the Rust front-end (`web-rs/`): clippy native (`core`) and wasm32 (every crate), `cargo test` (core), `trunk build --release`
  of the `overlay` and `sources` bundles and their size budget (each <= 300 kB gzip, `scripts/check-web-rs-size.sh`). It is a `publish` gate (the Docker image builds the same bundles); the app bundle budget is <= 1 MB brotli (`SIZE_CODEC=br`).
- `publish`: **only on push** (or manual run) **and only if the four previous jobs
  are green** (`needs`). Pushes the image to `ghcr.io/matella/storm-codex` with the
  tags `sha-<first 12 characters of the commit>` (always — allows rollback),
  `latest` (push to `main`) and the git tag name (`v*` tag). Docker build cache: GitHub
  Actions (`type=gha`).

The box can update itself with `docker pull ghcr.io/matella/storm-codex:latest` (or go back to
a specific version with `:sha-…`) instead of a local build — the box's current compose uses `build: .`.

## Docker image (`Dockerfile`)

- **Rust dependency cache: cargo-chef.** The `planner` stage computes a recipe of the
  workspace's dependencies; the `server` stage compiles them (`cargo chef cook`) in a layer that
  is only rebuilt if `Cargo.toml`/`Cargo.lock` change. Measured on 2026-11-07: after a
  code-only change, full rebuild in **91 s**, `cargo chef cook` layer cached.
- **Non-root user** (`storm`, uid **10001**). `/data` belongs to it in the image. A
  volume created by an older (root) image is handed back to that uid by the
  `storm-codex-init` service in `docker-compose.yml` (busybox, `chown -R 10001:10001 /data`, once per
  startup, before the server). Verified: root-owned volume → non-root server that writes
  to `/data/archive`, `raw-cache`, `images`.
- `stop_grace_period: 20s` in the compose file: `docker stop` lets the server finish in-flight
  requests (graceful shutdown, see 04-serveur.md).

## Backups

`scripts/backup.sh [dest]`: `pg_dump` → timestamped `.sql.gz`, retention 14. The database is in any
case **rebuildable from the raw archive** (re-upload / reprocess) — the backup just avoids
a re-backfill. The raw archive (the volume's `/data/archive`) is the only irreplaceable data.

## Standalone desktop app

Design: `docs/specs/2026-11-12-standalone-installer-design.md`. The Windows app bundles its own
Postgres; the supervisor crate `crates/storm-codex-supervisor` drives it.

**Bundled Postgres binaries.** `desktop/postgres-pin.txt` pins the EnterpriseDB Windows x64
Postgres 17 binaries zip (`url=`, `sha256=`). `pwsh scripts/fetch-postgres.ps1 [-Destination
<dir>] [-ZipPath <zip>]` downloads it (or takes a local copy), refuses it unless the SHA-256
matches, and extracts only `bin/`, `lib/`, `share/` and `server_license.txt` into `<dir>`
(default `target/postgres`). It installs nothing, creates no service and leaves `PATH` alone.
EDB publishes no checksum: the pin was computed on first download (trust-on-first-use).
Upgrading the minor version = change both lines in the same commit; the major stays **17**.

**Lifecycle (`Postgres`, `postgres.rs`).** All tools run from the bundled `bin/` through
`std::process::Command` (no shell, no console window on Windows, inherited `PG*` variables removed,
`LC_MESSAGES=C`), each with a wall-clock limit (pg_ctl 90 s, psql/createdb 60 s, dump/restore
30 min) after which the process is killed and `PgError::Timeout` is returned.
- **Authentication: generated password.** On first init a random password (32 bytes from the OS
  CSPRNG, hex) is written to `<data root>/pg-password`, next to (not inside) the cluster
  directory, and reused afterwards. `initdb -U storm -A scram-sha-256 --pwfile=<that file>
  -E UTF8 --locale=C`: every connection, local or TCP, needs the password, so other local
  accounts cannot log in as superuser. The password is a secret: child tools get it through
  `PGPASSWORD` (never on the command line), and it never appears in errors or logs.
  `database_url` contains it, so do not log that URL.
- Cluster in `<data root>/pg`: init skipped when `PG_VERSION` exists; a cluster of another major
  version is refused (`WrongMajorVersion`). `postgresql.conf` gets
  `listen_addresses = '127.0.0.1'` and no Unix socket.
- Pid-file ownership: `postmaster.pid` is "ours" only if its PID is alive, runs
  `<bin>/postgres.exe` and was started with `-D <our pg dir>`. Anything else (dead PID, PID
  reused by another program or by **another Postgres**) is stale: it is deleted, never signalled.
  Only the first line (PID) is read, as bytes: on Windows line 2 holds the data directory in the
  ANSI code page, which is not UTF-8 for accented paths.
- `clean_stale`: deletes a stale `postmaster.pid`; ours is left alone.
- `start`: removes a stale pid file, stops our own running server (so it comes back on the
  requested port), then `pg_ctl start -w -t 60 -o "-p <port> -c listen_addresses=127.0.0.1"`.
  The server log goes to `<data root>/logs/postgres.log` and pg_ctl's own output to
  `logs/pg_ctl.log`.
- `stop`: `pg_ctl stop -m fast -w -t 60` for our server only; a stale pid file is removed; no-op
  when there is none.
- Client tools (`psql`, `createdb`, `pg_dump`, `pg_restore`) run with `-w` (never prompt: there
  is no console), `PGCONNECT_TIMEOUT=10`; `restore` also sets `PGOPTIONS=-c lock_timeout=30s`.
  A wrong or missing password fails at once with `CommandFailed`.
- `ensure_database(name)`; `database_url(db)` = `postgres://storm:<password>@127.0.0.1:<port>/<db>`.
- `dump(out)` / `restore(file)`: `pg_dump -Fc` / `pg_restore --clean --if-exists --no-owner
  --exit-on-error --single-transaction` on the `storm_codex` database (created on restore if
  missing). One transaction: a failed restore leaves the database as it was.

**Tests.** `cargo test -p storm-codex-supervisor` always runs the binary-free tests: wrong major
version, missing binary, stale pid cleanup, foreign live PID treated as stale, non-UTF-8 pid
file, password file, URL, name validation. The real-server tests run only when `PG_BIN_DIR` is
set: roundtrip, data path with spaces and accents, stale pid, hard-kill recovery, foreign
Postgres PID never signalled, password required and reused, wrong password fails fast,
dump/restore:
`$env:PG_BIN_DIR = "$PWD\target\postgres\bin"; cargo test -p storm-codex-supervisor --test postgres_it`.

**Desktop shell (`desktop/`, Tauri v2).** Not a workspace member (own `Cargo.lock`): checks are
`cargo clippy --manifest-path desktop/Cargo.toml --all-targets -- -D warnings`,
`cargo fmt --manifest-path desktop/Cargo.toml --check` and
`cargo test --manifest-path desktop/Cargo.toml` (supervision order and failure paths, the updater
state machine and install flow, backup validation/zip-slip/import orchestration with fakes; no
Postgres needed). With `PG_BIN_DIR` set, `export_then_import_roundtrip_preserves_a_cast_row`
also runs a real Postgres 17 export → fresh cluster → import.
- **Folders.** Data root = Tauri `app_local_data_dir()` (`%LOCALAPPDATA%\com.matella.stormcodex`,
  covered by the uninstaller's "delete app data" option): `pg/`, `pg-password`, `archive/`,
  `raw-cache/`, `images/`, `logs/`, `uploaded.json`. Resources (next to the exe once packaged):
  `storm-codex-server.exe`, `postgres/{bin,lib,share}`, `web/` (= `web-rs/dist`), `minimaps/`
  (= `assets/minimaps`), `referential.tar.gz`. The Tauri identifier `com.matella.stormcodex`
  determines the data root `%LOCALAPPDATA%\com.matella.stormcodex`; changing it orphans the
  users' data, so it must stay stable (packaging keeps it).
- **Start (`Stack::start`).** `DataDir::ensure` → Postgres port (`pick_port(54329)`) →
  `clean_stale` → password check → `init_if_needed` → `start` → `ensure_database("storm_codex")`
  → server port (`pick_port(8088)`) → spawn the server (no console window, cwd = data root,
  stdout/stderr → `logs/server.log`, the previous run kept as `logs/server.prev.log`) → poll
  `GET /api/health` until 200 (60 s). Server environment: `STANDALONE=1`,
  `BIND_ADDR=127.0.0.1:<port>`, `DATABASE_URL` (holds the password: never logged),
  `ARCHIVE_DIR`, `RAW_CACHE_DIR`, `IMAGES_DIR`, `LEDGER_FILE=<data>/uploaded.json`,
  `WEB_DIR=<res>/web`, `MINIMAPS_BUNDLE_DIR=<res>/minimaps`,
  `REFERENTIAL_FILE=<res>/referential.tar.gz`. **Allow-list:** the child starts from an empty
  environment (`env_clear`) and gets only these plus, when the shell has them, `SystemRoot` and
  `windir` (Winsock/CNG/COM need them), `TEMP`, `TMP`, `USERPROFILE` (`std::env::temp_dir()`; the
  server extracts the referential and decodes cast replays there), `PATH` (system DLL search)
  and `RUST_LOG`. Nothing else of the shell's environment reaches it (`REDIS_URL`, `ADMIN_TOKEN`,
  `AZURE_PUSH_*`, a parent `DATABASE_URL`, ...); `LOCALAPPDATA`/`APPDATA` are not passed (every
  data path is explicit; the replay folders come from the Documents known-folder API). Tested by
  `server_env_is_an_allow_list` and `child_process_inherits_no_unlisted_variable` (runs
  `cmd /C set` with the server's command while the test process holds secrets). Any failure stops what was started (server
  killed, then Postgres stopped) and shows a native error dialog naming the log folder; the app
  then exits. A cluster whose `pg-password` is missing is refused with a clear error (never
  silently re-initialised).
- **Stop (`Stack::stop_with(budget)`, idempotent).** `POST /api/standalone/shutdown` on
  `http://127.0.0.1:<port>`, wait for the server to exit, kill it only as a last resort,
  **then** `pg_ctl stop -m fast`. Budget `Full` (tray **Quit**, `RunEvent::ExitRequested`/`Exit`):
  up to 20 s (its import task flushes the ledger first, up to 10 s), request I/O limited to 5 s.
  Budget `SessionEnd` (Windows shutdown/logoff, which force-closes apps after a few seconds): up
  to 3 s counted from before the request (request I/O 1 s), then kill; Postgres crash recovery
  covers what is cut short. Session end is seen through `WM_QUERYENDSESSION`/`WM_ENDSESSION` on
  a hidden top-level window (a GUI process gets no console-control event); from
  `WM_QUERYENDSESSION` on, every stop uses the `SessionEnd` budget (cleared if the session end
  is cancelled).
- **One stop, every caller waits (`lifecycle.rs`).** The stack is stopped once; any other caller
  (startup failure, Quit, exit hooks, session end, which run on different threads) blocks until
  that stop has finished, so the process never exits mid-stop. A stop requested while the stack
  is still starting waits for the start, then stops what was started, and the window is not
  opened.
- **Hard kill of the shell.** The server is in a kill-on-close job object, so it dies with the
  shell (no orphan holding the port or the ledger). Postgres keeps running until the next launch,
  where `Postgres::start` finds it is ours and restarts it (the new port may differ).
- **Windows and tray.** A small bundled `ui/loading.html` window shows during startup; once
  healthy the `main` window opens on `http://127.0.0.1:<port>/` (`WebviewUrl::External`) and the
  loading window is destroyed. The close button hides to the tray (the server keeps running for
  OBS). Tray: left click shows/focuses the window; menu Open Storm Codex, Restart to update
  (disabled until the updater enables it), Quit. One instance per user
  (`tauri-plugin-single-instance`, registered first): a second launch focuses the first.
- **IPC (`commands.rs`).** `app.withGlobalTauri: true` (the web UI detects `window.__TAURI__`).
  Commands: `update_status`, `install_update`, `export_data(path)`, `import_data(path, confirm?)`,
  `set_autostart(enabled)`, `get_autostart` (list in `src/command_names.rs`, read by `build.rs`).
  `build.rs` declares them with `tauri_build::AppManifest::commands`, so each one is **denied**
  unless a capability grants its generated `allow-<command>` permission (without it every
  registered command is allowed for every window). `capabilities/default.json` grants the `main`
  window, from `http://127.0.0.1:*` only, exactly those six `allow-*` permissions: no `core:*`,
  no plugin permission (the dialog, updater and autostart plugins' own JS commands are refused:
  checked live, e.g. `plugin:updater|check` → "not allowed"). The loading window has no
  capability. `capability_grants_only_the_shell_commands` fails if the grant list differs from
  the command list or a second capability file appears; `tauri-build` rejects unknown
  permission identifiers at compile time. Long commands are `async` and run on a blocking task
  (a sync command runs on the UI thread).
- **Navigation guard.** The `main` window's `on_navigation` lets only `http://127.0.0.1:<port>`
  (same scheme, host, port; no userinfo) load in the window, the only origin the capability
  trusts, so another page can never reach the IPC. Other `http(s)` links open in the system
  browser (`open::that_detached`, Rust side only: no opener plugin, no JS command); other schemes
  are dropped and logged. `window.open` / `target=_blank` (`on_new_window`) never create a
  webview: same treatment, then `Deny`.
- **CSP.** `app.security.csp` is strict (`default-src 'self'; script-src 'self'; style-src 'self'
  'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none';
  base-uri 'none'; form-action 'none'; frame-ancestors 'none'`). Tauri injects it only into the
  assets it serves itself (the bundled `ui/loading.html`, with hashes/nonces for its inline
  code); it does **not** apply to a `WebviewUrl::External` page, so the app page on
  `http://127.0.0.1:<port>` is governed by the server's own headers, and protected here by the
  navigation guard and the command-only capability.
- **Updater (`updater.rs`, `tauri-plugin-updater`).** State machine `none → downloading →
  ready(version)` behind the `UpdateSource` trait (fakes in tests). The first check runs once the
  main window is up, then every 6 h on an `updater` thread: `check()`; an announced update is
  `download()`ed (signature verified; **not** `download_and_install`) and its bytes kept in
  memory; then `ready` and the tray item **Restart to update** is enabled. A check or download
  failure returns to `none` with one log line and is retried at the next tick. `update_status`
  answers `{state: "none"|"downloading"|"ready", version}` (camelCase JSON, the web banner's
  contract in `08-frontend.md`). `install_update` (banner or tray item, same path) is refused
  with a clear message unless `ready`, and a second install while one runs is refused; otherwise:
  stop the stack (`Lifecycle::stop`, `Full` budget: server then Postgres, waiting for an export
  or import in progress), `Update::install(bytes)` (NSIS `passive` mode: progress bar, no
  questions; on Windows it launches the installer and exits the process, the installer
  relaunches the app with the same arguments), `app.restart()` on other platforms. Migrations
  run at the next start. **Install failure** (or a failed stop): the stack stays stopped, a
  native error dialog says the update could not be installed, that the data is unchanged and to
  start the app again, then the app exits with code 1 (simplest safe state: no half-restarted
  stack against binaries the installer may have touched; the next launch starts normally).
  `tauri.conf.json`: `bundle.createUpdaterArtifacts: true`; `plugins.updater.endpoints:
  ["https://github.com/matella/storm-codex/releases/latest/download/latest.json"]`,
  `windows.installMode: "passive"`, `pubkey` = a **placeholder** until the operator generates
  the key pair (`tauri signer generate`; the private key and its password are GitHub secrets,
  never committed; the public key replaces the placeholder; release signing is documented with
  packaging, Task 9). With the placeholder (or any value that is not base64 of a minisign
  public key, `pubkey_is_usable`) the shell never checks: `update_status` stays `none`, one log
  line "updates disabled: this build has no updater public key".
- **Updater manual E2E (two signed builds; done with packaging).** Build N and N+1 with the real
  key (`TAURI_SIGNING_PRIVATE_KEY` set, `createUpdaterArtifacts` gives the `.exe` + `.sig`);
  install N; serve a `latest.json` naming N+1 (url + signature) from a local static server and
  point a test build's endpoint at it (or publish a pre-release); start N: within seconds the
  log shows "update N+1 downloaded … ready", the banner shows **Restart to update** and the tray
  item is enabled; click: the app stops the stack, the passive installer runs, the app restarts
  on N+1, the data is intact and the migrations ran (server log). Repeat with the click during a
  backfill: after the restart the import resumes and no replay is uploaded twice.
- **Export / import (`backup.rs`).** File `storm-codex-backup-YYYY-MM-DD.stormcodex` (UTC date),
  a zip with `db.dump` (`pg_dump -Fc`) and `archive/<relative path>` for every file of the
  archive folder, all `Stored` (replays and the custom-format dump are already compressed; zip64
  for files ≥ 4 GiB), streamed (never loaded in memory). `export_data(path)`: an empty `path`
  opens a native save dialog (cancel → "export cancelled"). Postgres and the server keep running:
  the dump is taken first, then the archive copied (a replay imported meanwhile is an orphan
  file, harmless); written to `<out>.partial`, renamed when complete. `import_data(path,
  confirm)`: refused when the archive already holds a file unless `confirm: true`. Then, before
  anything changes: the zip is validated — exactly one `db.dump` starting with `PGDMP`; every
  other entry under `archive/`; no `\`, `:`, NUL, absolute path, empty/`.`/`..` component,
  Windows device name (`CON`, `nul.txt`) or trailing dot/space; no symlink or encrypted entry;
  ≤ 2,000,000 entries, ≤ 256 GiB per entry, deflated entries ≤ 200× expansion — and the data
  drive must have the unpacked size + 1 GiB free. It is unpacked into `<data>/import-work`
  (files created new, exact declared sizes). Then the server is stopped (graceful, `Full`
  budget; Postgres stays up: `pg_restore --clean` takes ACCESS EXCLUSIVE locks), the dump
  restored (one transaction), `archive/` swapped by renames (`archive` → `archive.old`, new →
  `archive`, old removed; put back if the rename fails), `import-work` removed, and the server
  restarted on the same stack and port (`Stack::restart_server`); the command returns once
  `/api/health` answers. A failed restore leaves the database and the archive as they were, and
  the server is restarted either way. The ledger and the raw cache are kept (both keyed by
  content). Export and import run through `Lifecycle::with_running`: one at a time, and a Quit
  or session end waits for them to finish. Not exposed in the web UI yet.
- **Autostart.** `tauri-plugin-autostart` (Windows: per-user `HKCU\…\Run` value "Storm Codex",
  launched with `--minimized`). `AUTOSTART_DEFAULT = false`: on the first launch with a data
  folder the shell sets autostart to that default (also clearing a `Run` value an earlier
  install left behind) and writes `<data>/autostart-initialised`; later launches keep the user's
  choice (`set_autostart` / `get_autostart`). `--minimized` (parsed by `starts_minimized`, the
  program name excluded): the loading window is created hidden, the stack starts, and the main
  window opens hidden in the tray.
- **Logs.** `logs/desktop.log` (shell), `logs/server.log`, `logs/postgres.log`,
  `logs/pg_ctl.log`.
- **Dev run (debug builds only, compiled out of release).** `STORM_CODEX_RESOURCE_DIR` replaces
  the resource folder and `STORM_CODEX_DATA_ROOT` the data root, e.g. a scratch folder holding
  the server exe, `postgres/` from `scripts/fetch-postgres.ps1`, `web/` and `minimaps/`:
  `$env:STORM_CODEX_RESOURCE_DIR="<res>"; $env:STORM_CODEX_DATA_ROOT="<tmp>"; desktop\target\debug\storm-codex-desktop.exe`.
  Icons in `desktop/icons/` are placeholders until packaging.

## External dependencies (all best-effort)

| Service | Where | If missing |
|---|---|---|
| HotsPatchNotes | box :5001 (`host.docker.internal`) | no dim_* / image refresh (already replicated data kept) |
| Jarvis Redis | `jarvis_default` network | no event emission (parse unaffected) |
| Orpheus | box :3010 | `/api/now-playing` empty (music widget idle) |

## Known risks & debt (audit 2026-08-03 — to redo periodically)

Accepted as is (context: self-hosted, LAN/Tailscale, one operator):
- **Empty `ADMIN_TOKEN` = open mode** — intended locally; set it if exposed (reverse proxy).
- **Init panics** in storm-stats (`constants.rs`, regexes in `src/process/`) — on data
  **embedded** at compile time: a failure = invalid build, a crash at startup is acceptable.
- Admin auth: factored into `auth.rs` (digest comparison, no early exit);
  cast/draft writes are guarded when `ADMIN_TOKEN` is set (2026-11-07).

Dependencies (operator decision of 2026-11-07: "fix all of it"):
- `cargo audit` runs in CI (`audit` job).

## Runbooks (detailed operator procedures)

`docs/runbooks/`: switchover/decommissioning of the Node overlay · reverse proxy + ntfy · crates.io
publication (`2026-06-13-publication-crates.md`).
