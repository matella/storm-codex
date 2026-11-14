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

## CI — `ci.yml` (checks, tests, audit, then image publication) and `release-desktop.yml`

`release-desktop.yml` builds and publishes the desktop installer on a `desktop-v*` tag: see "Releasing the desktop
app" below. Everything else is `ci.yml`.

**Jobs** (push to `main`, `v*` tags, PRs, manual run):
- `checks`: `cargo fmt --all -- --check`; **frozen migrations** (`scripts/check-migrations.sh`):
  rejects any change, deletion or rename of a migration already present in the comparison base
  (the target branch for a PR, the previous commit for a push). sqlx checks the
  checksum of every applied migration: rewriting one makes startup fail
  (`VersionMismatch`) for every database that already applied it. A schema change = a
  **new** migration. Modifying a migration added in the same PR is still allowed.
- `rust`: strict clippy (`-D warnings`) + `cargo test --workspace` against a **Postgres 17
  service** (DB-gated integration tests actually run). The supervisor's real-Postgres tests need
  Postgres **binaries** (`PG_BIN_DIR`), not a server: here they skip or stay ignored; the `desktop`
  job runs them.
- `desktop` (windows-latest): the desktop shell (`desktop/`, own `Cargo.lock`) —
  `cargo fmt --manifest-path desktop/Cargo.toml --check`, strict clippy `--all-targets`, then
  `cargo test --manifest-path desktop/Cargo.toml -- --include-ignored` and
  `cargo test -p storm-codex-supervisor -- --include-ignored` with `PG_BIN_DIR` set to the pinned
  Postgres 17 binaries (`scripts/fetch-postgres.ps1`, SHA-256 checked, cached with `actions/cache`
  under a key derived from `desktop/postgres-pin.txt`). So the real-Postgres tests of the
  supervisor and the shell's export → import roundtrip run on every push/PR. Not a `publish` gate
  (the Docker image does not contain the shell).
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
- **Paths outside the ANSI code page (`toolpath.rs`, `ToolPaths`).** Postgres for Windows is a
  narrow-char program: its `argv` is converted to the system ANSI code page and it finds `share/`
  through its own module path, so a Windows user named in a script outside that code page
  (Cyrillic on a Western Windows) put both the installed binaries and the data root out of reach
  (`initdb` failed: "could not open file …\????…\pg-password", or "invalid binary"). Every path
  given to a Postgres tool, **the executable included**, goes through `ToolPaths::map`: (1) kept
  when every character fits the code page (`WideCharToMultiByte`, `WC_NO_BEST_FIT_CHARS`; an
  ANSI code page of UTF-8 fits everything); (2) else its 8.3 short form (`GetShortPathNameW` of
  the deepest existing ancestor + the missing tail); (3) else (8.3 names disabled on the volume,
  or the folder created while they were) a **directory junction** `%ProgramData%\StormCodex-<user
  SID>\<16 hex FNV-1a of the folder>` → the deepest folder whose name does not fit, plus the rest
  of the path. That alias folder is created by the app with owner = the user and a **protected
  DACL** (the user and SYSTEM only), and refused ("not this user's private folder") if it exists
  with another owner or an inherited DACL, so no other account can plant or redirect a junction; a
  junction of ours pointing elsewhere is replaced. The junction holds no data: the cluster, logs,
  dumps and archive stay in the data root, so export/import, the uninstaller's delete-data option
  and a later change between the three forms (e.g. 8.3 names turned off) see the same files.
  Ownership (`postmaster.pid`) compares canonical paths, which resolve short names and junctions:
  a postmaster started through either form is still recognised as ours. Tests: `toolpath`
  unit tests (short form, forced junction, reuse, replacement, refused non-private alias folder)
  and ITs `data_root_outside_the_ansi_code_page_works`, `binaries_outside_the_ansi_code_page_work`
  (binaries copied under a Cyrillic folder), `without_short_names_a_junction_alias_is_used_and_modes_can_switch`.
  The NSIS hooks pass `pg_ctl` and the cluster as short paths too (`GetFullPathName /SHORT`; with
  8.3 names disabled they fall back to the long path, which fails for such names: the app's next
  start still recovers the cluster).
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
  is no console), `PGCONNECT_TIMEOUT=10`, `PGOPTIONS=-c search_path=pg_catalog,pg_temp`.
  A wrong or missing password fails at once with `CommandFailed`.
- `ensure_database(name)`; `database_url(db)` = `postgres://storm:<password>@127.0.0.1:<port>/<db>`.
- `dump(out)`: `pg_dump -Fc --data-only` of the `storm_codex` database (rows and sequence
  values only, the migration ledger `_sqlx_migrations` included; no schema). There is no plain
  `restore`: a dump is only ever restored through `stage_import` (see Export / import).

**Tests.** `cargo test -p storm-codex-supervisor` always runs the binary-free tests: wrong major
version, missing binary, stale pid cleanup, foreign live PID treated as stale, non-UTF-8 pid
file, password file, URL, name validation. The real-server tests run only when `PG_BIN_DIR` is
set; the import tests are `#[ignore]`d and **fail** (never skip) without it: roundtrip, data
path with spaces and accents, stale pid, hard-kill recovery, foreign Postgres PID never
signalled, password required and reused, wrong password fails fast, data-only dump, staged
import (same, older and newer version, schema objects refused, what the import role cannot do,
large object refused):
`$env:PG_BIN_DIR = "$PWD\target\postgres\bin"; cargo test -p storm-codex-supervisor --test postgres_it -- --include-ignored`.
CI runs them in the `desktop` job (see CI above).
The import tests play the server's `migrate` with psql over `crates/storm-codex-server/migrations`
(same ledger table, deterministic checksums).

**Desktop shell (`desktop/`, Tauri v2).** Not a workspace member (own `Cargo.lock`): checks are
`cargo clippy --manifest-path desktop/Cargo.toml --all-targets -- -D warnings`,
`cargo fmt --manifest-path desktop/Cargo.toml --check` and
`cargo test --manifest-path desktop/Cargo.toml` (supervision order and failure paths, the updater
state machine and install flow, backup validation/zip-slip/import orchestration with fakes; no
Postgres needed). With `PG_BIN_DIR` set and `-- --include-ignored`,
`export_then_import_roundtrip_preserves_a_settings_row` also runs a real Postgres 17 export →
fresh migrated cluster → import.
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
  `GET /api/health` until 200 (60 s). **Port choice (`pick_port`):** the preferred port is kept
  only if 127.0.0.1 can be bound **and** nothing accepts a connection on `[::1]` (250 ms probe);
  otherwise an OS-assigned port passing the same test. Reason: `http://localhost:<port>` resolves
  to `::1` first on Windows, and Windows lets the server bind 127.0.0.1 while another program of
  the same user listens on `[::]` (seen on the operator's PC: qBittorrent's web UI on `[::]:8088`
  answered `http://localhost:8088/cast/overlay` with 404 while the app was on 127.0.0.1:8088).
  The URLs the app shows use `127.0.0.1` (the page origin). Server environment: `STANDALONE=1`,
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
  Commands: `update_status`, `install_update`, `export_data()`, `import_data()`,
  `set_autostart(enabled)`, `get_autostart` (list in `src/command_names.rs`, read by `build.rs`).
  `export_data` and `import_data` take **no argument**: the page can neither choose a path nor
  confirm; the shell opens the native dialogs itself (see Export / import). `build.rs` declares
  the commands with `tauri_build::AppManifest::commands`, so each one is **denied** unless a
  capability grants its generated `allow-<command>` permission (without it every registered
  command is allowed for every window). There is **no capability file**: the shell adds one
  capability at runtime (`Manager::add_capability`, `commands::main_capability`) right before it
  opens the `main` window, once the server port is known: window `main`, remote origin exactly
  `http://127.0.0.1:<port>` (no port wildcard: a page or iframe served on another local port,
  e.g. a dev server, gets no IPC), `local: false`, exactly those six `allow-*` permissions: no
  `core:*`, no plugin permission (the dialog, updater and autostart plugins' own JS commands are
  refused: checked live, e.g. `plugin:updater|check` → "not allowed"). Tauri cannot express the
  exact port in a static file because the port is chosen at startup (`8088` or the next free
  one); the runtime capability needs the default `dynamic-acl` feature. The loading window has
  no capability. `capability_is_the_exact_origin_and_the_shell_commands_only` fails if the grant
  list differs from the command list, if the origin pattern matches another port, host or
  scheme, or if a `capabilities/` folder reappears. Long commands are `async` and run on a
  blocking task (a sync command runs on the UI thread).
- **Navigation guard.** The `main` window's `on_navigation` lets only `http://127.0.0.1:<port>`
  (same scheme, host, port; no userinfo) load in the window, the only origin the capability
  trusts. This keeps other sites out of the top-level window; it does not make the app page
  safe by itself: any script running in that origin (an XSS in the web UI, or content the
  server serves there) can call the six commands, which is why they take no path or
  confirmation from the page. Other `http(s)` links open in the system
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
  memory (the installer's size, tens of MB, held until install or exit: accepted; streaming it
  to a file is deferred); then `ready` and the tray item **Restart to update** is enabled. A check or download
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
  the key pair (`cargo tauri signer generate`; the private key and its password are GitHub
  secrets, never committed; the public key replaces the placeholder; see "Releasing the desktop
  app"). With the placeholder (or any value that is not base64 of a minisign
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
  a zip with `db.dump` (`pg_dump -Fc --data-only`: rows only, with the migration ledger that
  says which schema version they belong to) and `archive/<relative path>` for every file of the
  archive folder, all `Stored` (replays and the custom-format dump are already compressed; zip64
  for files ≥ 4 GiB), streamed (never loaded in memory). Export and import run through
  `Lifecycle::with_running`: one at a time, and a Quit or session end waits for them to finish.
  Web UI: "Backup" block of the "This PC" settings (`08-frontend.md`), which calls both commands
  without arguments.
  - **Export (`export_data()`).** The shell opens the native save dialog (default name above;
    cancel → "export cancelled"). Windows' save dialog asks before replacing an existing file
    (`FOS_OVERWRITEPROMPT`, the default of the dialog plugin's `rfd` backend); nothing else
    ever replaces a file. UNC, device (`\\?\`, `\\.\`), relative paths and mapped network
    drives (`GetDriveTypeW` = remote) are refused (`backup::check_local_path`). Postgres and the
    server keep running: the dump is taken first, then the archive copied (a replay imported
    meanwhile is an orphan file, harmless); written to `<out>.partial` (created new: an
    existing file of that name is never overwritten, the export fails instead), renamed to the
    chosen path when complete.
  - **Import (`import_data()`).** The shell opens the native open dialog, refuses a non-local
    path as above, then always asks for an explicit OK in a native confirmation ("replaces all
    Storm Codex data on this PC", naming the replays already there if any). Then
    (`backup::import_into`):
    1. **Private copy.** The chosen file is copied (exact size read first; refused if it
       changes meanwhile; free space ≥ its size + 1 GiB) to `<data>/import-work/backup.zip`;
       everything below reads only that copy, so the file cannot be swapped between the check
       and the use.
    2. **Validation** of the copy — exactly one `db.dump` starting with `PGDMP`; every other
       entry under `archive/`; no `\`, `:`, NUL, absolute path, empty/`.`/`..` component,
       Windows device name (`CON`, `nul.txt`, `con .txt`, `CONIN$`, `CONOUT$`, `CLOCK$`,
       `COM1`–`9`, `LPT1`–`9`, `COM¹`–`³`, `LPT¹`–`³`) or trailing dot/space; no symlink or
       encrypted entry; ≤ 2,000,000 entries, ≤ 256 GiB per entry, deflated entries ≤ 200×
       expansion — and free space ≥ unpacked size + 3 × dump size (the staging database
       beside the current one) + 1 GiB.
    3. **Unpack** into `import-work` within the validated budget (entry count, total bytes,
       file count, one dump of the validated size; symlink/encryption re-checked; files created
       new with exact declared sizes).
    4. **Staged restore** while the server keeps running (`Postgres::stage_import`, see below).
       A refused or failed restore (a backup from a newer app included) changes nothing.
    5. **Commit.** `<data>/import-journal` written, server stopped (graceful, `Full` budget;
       Postgres stays up), `archive` → `archive.old` and the unpacked folder → `archive`
       (renames, retried briefly; refused if an `archive.old` already exists, which is never
       deleted blindly), then `promote_import`: staging database settings reset, staging check
       run again, `storm_codex` → `storm_codex_previous` and `storm_codex_import` →
       `storm_codex` in **one transaction**, then `finish_import` (previous database and any
       leftover `storm_import` role dropped). Then the upload **ledger (`uploaded.json`) is
       reset**: it lists what the replaced database had imported, and a stale entry would make
       the watcher skip a replay the new database lacks (re-scanning is safe: uploads are
       deduplicated by content hash); the raw cache is kept (keyed by content). `archive.old`
       and the journal are removed and the server restarted on the same stack and port
       (`Stack::restart_server`); the command returns once `/api/health` answers.
    6. **Failure during the commit.** Which side of the rename transaction a `promote_import`
       error happened on is read back (`staging_exists`):
       - **before the rename** (staging database still there): `archive.old` is put back, the
         staging database dropped, the journal removed, the server restarted; the message says
         nothing was changed. If putting `archive.old` back fails, the server stays stopped and
         the error says to restart Storm Codex (the journal makes the next start roll back);
       - **after the rename committed** (the imported database is the app's): **never rolled
         back**. The import is completed (`finish_import`, ledger reset, `archive.old` and
         journal removed) and the command succeeds; if that cleanup fails too, the server is
         restarted on the imported data and the error says the backup was imported and is in
         use and that the cleanup is completed at the next start (the journal is kept);
       - **state unreadable**: everything (journal, staging, `archive.old`) is left as is, the
         server stays stopped, and the error says to restart Storm Codex, whose recovery
         decides.
       A swap whose rollback rename fails too (the replays only in `archive.old`) also keeps
       the journal and the staging database and leaves the server stopped: the next start
       rolls back. Only the cases that really changed nothing say "nothing was changed".
    7. **Crash recovery** (`backup::recover_import`, every start, after Postgres and before
       the server): journal + staging database → not promoted: roll back (`archive.old` put
       back, journal removed, staging dropped); journal without staging database → promoted:
       complete it (`finish_import`, ledger reset, `archive.old` and journal removed); no
       journal → a staging database is an orphan (dropped), and an `archive.old` next to an
       **empty** `archive` is put back (one next to a non-empty `archive` is left alone and must
       be removed by hand). `import-work` is removed. A recovery failure stops the startup with
       "an interrupted import could not be finished or rolled back".
    8. **Port during the commit.** While the server is stopped (seconds) another local program
       could bind the server port; the restart then fails its health check and the error names
       the log. Accepted: the window is short, the data is already committed, and the next start
       picks a free port.
  - **Untrusted dump: data only (`Postgres::stage_import`).** A backup may come from anyone,
    and the shell drives Postgres as a superuser. No schema object from the file is ever used:
    a backup holds **rows only**, and the schema they go into is built by the app's own
    migrations.
    1. **TOC filter.** `pg_restore -l` lists the entries; each must be exactly (kind compared
       as a whole, not as a prefix) `ENCODING`, `STDSTRINGS` or `SEARCHPATH` (schema `-`), or
       `TABLE DATA` / `SEQUENCE SET` in schema `public`. Anything else (a `TABLE`, `FUNCTION`,
       `DOMAIN`, `TYPE`, `TRIGGER`, `CONSTRAINT TRIGGER`, `DEFAULT ACL`, `BLOB`, `ACL`, an entry
       in another schema, an unparsable line) refuses the file before anything runs. The
       checked list, minus the migration ledger's own data, is passed to the restore with `-L`.
    2. **Version from the ledger.** The dump's `_sqlx_migrations` rows are read as text
       (`pg_restore --data-only -t _sqlx_migrations -f -`, parsed, nothing executed); every
       row must be successful; the highest version is the backup's version `N`.
    3. **Schema from the app.** A fresh `storm_codex_import` database (owned by the superuser,
       `CONNECT` revoked from `PUBLIC`) is migrated **by the server binary itself** up to `N`:
       `storm-codex-server migrate --to N` (`DATABASE_URL` = the staging database; exit 0 done,
       2 usage, **3 = `N` is newer than this build**, 1 other failure; output in
       `logs/migrate.log`, 30 min limit). Exit 3 refuses the import with "this backup was made
       by a newer version of Storm Codex; update Storm Codex, then import it again". The
       staging ledger must then equal the dump's (versions and checksums), else the backup is
       refused as incompatible.
    4. **Restore as a throw-away role.** The rows the migrations seeded are truncated, `CREATE`
       on schema `public` is revoked from `PUBLIC`, and a fresh role `storm_import` is created
       (`LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT`, its
       **own** random password, never the superuser's; the `CREATE ROLE` goes through a private
       file in a session with `log_statement = none` and `log_min_error_statement = panic`, so
       the password reaches neither a command line nor the server log). It gets `CONNECT`,
       `USAGE` on `public`, `INSERT` on the app tables (not on `_sqlx_migrations`) and `UPDATE`
       on their sequences: no ownership, no `CREATE`, no `TEMP`, no `DELETE` or `TRUNCATE`.
       `pg_restore --data-only --no-owner --no-acl --exit-on-error --single-transaction -L`
       runs connected as that role.
    5. **Check, then the role is gone.** As the superuser, from the catalogs only: refused if
       anything is owned by `storm_import` (`pg_shdepend`), if any large object exists, or if
       the staging database has a setting. Then `ALTER ROLE storm_import NOLOGIN PASSWORD
       NULL`, `DROP OWNED BY storm_import` in the staging database and `DROP ROLE`.
    6. **Forward.** `storm-codex-server migrate` (no target) applies this build's remaining
       migrations: a backup from an older app is brought up to date like any database at
       server start.
    Any failure drops the staging database and the role. Tests (supervisor ITs): same-version
    import and swap, older backup migrated forward, newer backup refused, a full (schema) dump
    with a `DOMAIN … DEFAULT pg_read_file(…)` refused by the TOC filter before anything runs,
    SQL run as the import role (as a crafted entry would) failing to create a domain with a
    `lo_export` default, a function, a schema, a table or a temp table, to alter a table or
    the database, to write the ledger, `COPY … PROGRAM`/file, `pg_read_file`, `ALTER SYSTEM`
    and `CREATE ROLE`, its `GRANT … TO PUBLIC` having no effect, and the role gone (login
    refused) afterwards; a large object created during the restore refuses the import.
    **Residual risk** (accepted, documented): the SQL text of an allowed entry is not parsed,
    so a crafted backup can run arbitrary SQL **as `storm_import`**, which can only insert rows
    into the app tables and set sequence values in the staging database (and change its own
    role settings, discarded with the role). So a malicious backup can put arbitrary **data**
    in the app tables (wrong stats, very large tables, strings the web UI must escape like any
    other data), make the forward migrations process that data (this build's own static SQL,
    which treats rows as data), and use disk space and time during the restore. No function,
    type, domain, trigger, default or setting from the file ever exists in the database. This
    rests on Postgres enforcing an ordinary role's privileges; a Postgres privilege-escalation
    bug reachable by such a role would bypass it. Only import backups from a source you trust.
    **Compatibility:** backups in the earlier full-schema format are refused by the TOC filter;
    none were released.
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
  Icons in `desktop/icons/` are still placeholders (packaging uses them as they are; replace the
  files, same names, before a public release).

### Packaging (NSIS installer)

- **Config.** `desktop/tauri.conf.json` (always read): `bundle.targets: ["nsis"]`,
  `bundle.publisher`, `bundle.windows.nsis.installMode: "currentUser"` (installs under
  `%LOCALAPPDATA%`, no administrator prompt; per-user `HKCU` uninstall entry),
  `bundle.windows.nsis.installerHooks: "./windows/hooks.nsh"`, `bundle.createUpdaterArtifacts:
  true`, `bundle.active: false`. `desktop/tauri.bundle.conf.json` (passed only to the release build,
  `cargo tauri build --config tauri.bundle.conf.json`, merged over the main file) sets
  `bundle.active: true` and `bundle.resources`. They are kept apart because `tauri-build` copies
  `bundle.resources` next to the binary on **every** `cargo build` and fails when one is missing:
  in the main file, `cargo clippy`/`cargo test` of `desktop/` would need the whole release payload.
- **Resources** (map form, source relative to `desktop/` → path under the resource folder), exactly
  what `supervise.rs` resolves: `../target/release/storm-codex-server.exe` →
  `storm-codex-server.exe`; `../target/postgres/` (from `scripts/fetch-postgres.ps1`) →
  `postgres/`; `../web-rs/dist/` → `web/` (`app/`, `overlay/`, `sources/`); `../assets/minimaps/` →
  `minimaps/`; `../target/referential/referential.tar.gz` (from `scripts/fetch-referential.ps1`)
  → `referential.tar.gz`. `loading.html` is not a resource: it is `build.frontendDist` (`desktop/ui`),
  embedded in the executable.
- **Uninstall: keep or delete the data.** Tauri's NSIS template already provides it: the
  uninstaller's confirmation page has a **"Delete the application data"** checkbox, **unticked by
  default**; only when ticked does it remove `%LOCALAPPDATA%\com.matella.stormcodex` (the data root:
  database cluster, password file, archived replays, logs) and `%APPDATA%\com.matella.stormcodex`.
  The template skips that deletion in update mode (`/UPDATE`, used when an update replaces the
  installation), and a silent uninstall (`/S`) shows no page so the box stays unticked: **an
  update or a silent uninstall never deletes data**. No custom code deletes data.
- **Hooks (`desktop/windows/hooks.nsh`).** `NSIS_HOOK_PREINSTALL` and `NSIS_HOOK_PREUNINSTALL`
  stop the bundled Postgres of the current user's data root (`pg_ctl stop -D <data>\pg -m fast -w
  -t 30`, through `nsExec`, only when `postgres\bin\pg_ctl.exe` and `pg\postmaster.pid` exist,
  result ignored). Postgres may outlive a hard-killed shell and would keep `postgres\` locked, so
  an upgrade could not replace those files and an uninstall could not remove them. pg_ctl only
  signals the postmaster named in that cluster's own pid file. **Ask first:** the template runs
  these hooks *before* its own "app is running" check, so in an interactive run (not `/S`, not `/P`
  passive, not `/UPDATE`) the hook first looks for a running `<main binary>.exe` (`tasklist /FO
  CSV`); if found it shows "Storm Codex is running. Quit it first (tray icon, then Quit)…" and
  aborts **without stopping anything** (tray Quit stops the server, then Postgres, cleanly). An
  update started by the app (`/P /UPDATE`) skips that question: the app has already stopped its
  stack and is exiting (the template closes it if it is still there), so stopping Postgres is a
  no-op or finishes what the app started.
- **SmartScreen.** The installer is **not code-signed** (no certificate, a non-goal of the spec):
  Windows SmartScreen shows "Windows protected your PC" on first download; users click **More
  info → Run anyway**. The updater signature (minisign, below) is unrelated: it protects updates,
  not the first install.

### Reference-data snapshot (`referential.tar.gz`)

- **Build.** `sh scripts/make-referential.sh <hpn-base-url> [out]` (or `HPN_BASE_URL=<url>`;
  default out `target/referential/referential.tar.gz`; needs `curl`, `jq`, `tar`, `sha256sum` or
  `shasum`) downloads from the operator's HotsPatchNotes API: `heroes.json` (`/api/heroes`),
  `hero-details.json` (`{shortName: /api/heroes/<shortName>}`), `patches.json`
  (`/api/patches?page=1&pageSize=200`, the same page `sync_patches` reads), `patch-details.json`
  (`{internalId: /api/patches/<internalId>}`) and `images/**` (the `/images/...` files named by
  `heroes[].icon` and `/api/battlegrounds` `imageUrl`; a 404 is skipped like the server's own
  vendoring). That is exactly the layout `dim::ingest_snapshot_file` reads. It fails on an empty
  hero or patch list, on a key or image path with unexpected characters, and prints the SHA-256.
  No host or secret is built in. Checked locally against a fake API (2026-11-14); never run
  against the real HotsPatchNotes yet.
- **Publish and pin.** The operator uploads the file as an asset of a dedicated **pre-release**
  that is never "latest" (e.g. `gh release create referential-2026-11-14 referential.tar.gz
  --prerelease --latest=false --title "Reference data 2026-11-14"`; a "latest" release would hijack
  the updater URL `releases/latest/download/latest.json`), then commits its asset URL and SHA-256 in
  `desktop/referential-pin.txt`. `scripts/fetch-referential.ps1` (used by the release workflow)
  refuses a placeholder pin, a non-https URL or a hash mismatch. **The pin is a placeholder today:
  the release workflow fails until the operator uploads the first snapshot.**
- **Stale snapshot (accepted risk).** A standalone install never contacts HotsPatchNotes: heroes,
  talents and patches released after the snapshot are unknown until a new installer ships with a
  newer snapshot (new hero = generic portrait and missing talent names; replays still import and
  every stat is computed from the replay itself). Refreshing = rebuild, upload, re-pin, release:
  the updated app sees a bundled snapshot whose SHA-256 differs from the one it last ingested
  and ingests it at the next start (`04-serveur.md`, Reference data).
  Follow-up: a public snapshot endpoint/publication on the HotsPatchNotes side
  (matella/HeroesOfTheStormPatchNotes#1), so the snapshot can be refreshed without the operator's
  private API; a runtime refresh stays out of scope (spec non-goal).

### Releasing the desktop app (`release-desktop.yml`)

**One-time setup (operator).**
1. Generate the updater key pair on a trusted machine: `cargo tauri signer generate -w
   ~/.tauri/storm-codex.key` (asks for a password). It writes the private key and
   `storm-codex.key.pub`. Keep the private key and its password offline as well: **losing either
   means installed apps can never be updated again**.
2. GitHub → repository → Settings → Secrets and variables → Actions: secrets
   `TAURI_SIGNING_PRIVATE_KEY` (the private key file's content) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never committed, never in logs.
3. Replace `plugins.updater.pubkey` in `desktop/tauri.conf.json` with the **content** of
   `storm-codex.key.pub` (one base64 line), commit. Builds made before that never check for updates.
4. Build, upload and pin the reference-data snapshot (above).

**Dry run** (no tag, nothing reaches users): Actions → `release-desktop` → Run workflow. Same build,
published as a **draft** release `desktop-v<version>` targeting the commit (drafts are never
"latest" and are invisible to installed apps). Download the installer from the draft, install it,
check, fill in the measurements below, then delete the draft. Do it before tagging that version
(a draft with the tag of an existing release cannot be created). Dry-run checklist: the workflow's
`cargo tauri build` compiled `windows/hooks.nsh` (proves `nsExec`, `${FileExists}`, `${Silent}`,
`$PassiveMode`/`$UpdateMode` and `${MAINBINARYNAME}` resolve); installing while the app runs shows
the "Quit it first" message and leaves it running; installing over an app killed from Task Manager
(its Postgres still running) stops that Postgres (`logs/postgres.log` shows a fast shutdown) and
replaces `postgres\`; the uninstaller shows the "Delete the application data" box unticked and
keeps `%LOCALAPPDATA%\com.matella.stormcodex` when left unticked; installer size and first start
measured.

**Supply-chain rules of the release job** (it holds `contents: write` and the signing key):
- every action pinned to a full commit SHA with the version in a comment (`actions/checkout`
  v4.4.0; `dtolnay/rust-toolchain` at the commit of its `stable` branch tip on 2026-11-14 — the
  toolchain itself is pinned by `toolchain: 1.94.0`, env `RUST_TOOLCHAIN`, the version the build is
  checked with locally). Bumping an action = resolve the new tag's commit
  (`gh api repos/<owner>/<repo>/git/ref/tags/<tag>`, dereferencing an annotated tag) and replace
  SHA and comment together;
- **no build cache** (no `rust-cache`): a restored `~/.cargo/bin` or `target/` could supply a
  `cargo-tauri` or a build output to the step that holds the key. Clean build, releases are rare.
  `cargo install tauri-cli … --locked --force` never reuses an existing binary;
- Trunk is checked against the SHA-256 **committed** in the workflow (`TRUNK_SHA256`, next to
  `TRUNK_VERSION`; computed by downloading the pinned asset once, 2026-11-14), not a hash from the
  same release. Bump both together;
- `actions/checkout` with `persist-credentials: false` (the token is not left in `.git/config`;
  `gh` gets it through `GH_TOKEN`);
- event values (`github.ref_name`, `github.event_name`, `github.sha`, `github.repository`) reach the
  scripts through `env:`, never interpolated inside `run:` (a tag name can contain `$(…)`).
Deferred hardening: `ci.yml` (no signing key; only `publish` writes) still uses movable action refs
(`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`,
`taiki-e/install-action@v2`, `docker/*@v3`/`@v6`) and the default `persist-credentials`; only its
new `actions/cache` is SHA-pinned.

**Cutting a release.**
1. Bump `version` in `desktop/tauri.conf.json` (`X.Y.Z`; also `desktop/Cargo.toml` for clarity),
   commit, push.
2. `git tag desktop-vX.Y.Z && git push origin desktop-vX.Y.Z` (the tag must equal the config
   version, else the workflow fails).
3. The workflow (windows-latest): refuses a placeholder updater pubkey, a missing signing secret
   or a placeholder referential pin; fetches the snapshot and Postgres 17 (both SHA-256 checked);
   installs pinned tools (Rust `1.94.0` + `wasm32-unknown-unknown`; Trunk prebuilt `0.22.0-beta.1`,
   SHA-256 checked against the committed `TRUNK_SHA256`; `cargo install tauri-cli --version 2.12.4
   --locked --force`; NSIS is downloaded by the Tauri bundler itself); builds the three `web-rs` bundles, `cargo build --release --locked -p
   storm-codex-server`, then `cargo tauri build --config tauri.bundle.conf.json` with the signing
   secrets in the environment (gives `…-setup.exe` and its `.sig`).
4. It publishes, with `gh release create --latest`, the release `desktop-vX.Y.Z` with
   `StormCodex_X.Y.Z_x64-setup.exe` (renamed: no spaces, so the URL is stable), its `.sig` and
   `latest.json`:
   `{"version": "X.Y.Z", "notes": "...", "pub_date": "<RFC 3339>", "platforms": {"windows-x86_64":
   {"signature": "<content of the .sig>", "url": "https://github.com/<repo>/releases/download/desktop-vX.Y.Z/StormCodex_X.Y.Z_x64-setup.exe"}}}`
   (static updater manifest: `version`, `platforms.<target>.url` and `.signature` required; the
   signature is the file's content, not a path). Installed apps read
   `releases/latest/download/latest.json`, so the desktop release must stay the repository's
   **latest** release (the repository had no GitHub Release on 2026-11-14; any other release, the
   snapshot's included, is created with `--latest=false`).
5. The job summary prints the installer size.

**Measurements** (no cap, per the spec; to fill in at the first dry run / Task 10):

| Measure | Value | Date / build |
|---|---|---|
| Installer size (`StormCodex_X.Y.Z_x64-setup.exe`) | not measured yet (no installer built) | — |
| First start, install → app window (clean profile, empty data) | not measured yet (installed build) | — |
| First start, empty data root → first page served (dev run: **debug** shell, release server, initdb + migrations + a tiny test snapshot) | 7.7 s and 8.8 s (2 runs) | 2026-11-14, `feat/standalone-installer` |
| Restart with existing data → first page served (same dev run) | 3.5 s and 3.6 s (2 runs; one after a hard kill) | 2026-11-14 |
| First start → import `ready`, operator's replays (3,427 files, 3.1 GB, 2024-08 → 2026-11) | ≈ 2.7 min of import time (63 s + 100 s, interrupted once by a hard kill) | 2026-11-14 |

Method and the other figures (parse, API p95): [`docs/standalone-acceptance.md`](../standalone-acceptance.md).

**Not verified yet** (no tag pushed, no workflow run): the real `cargo tauri build`, the NSIS
hooks at install/uninstall time, the keep/delete prompt, the release workflow end to end and the
`latest.json` consumed by a real update.

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
