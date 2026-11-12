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
- `dump(out)` / `restore(file)`: `pg_dump -Fc` / `pg_restore --clean --if-exists --no-owner` on
  the `storm_codex` database (created on restore if missing).

**Tests.** `cargo test -p storm-codex-supervisor` always runs the binary-free tests: wrong major
version, missing binary, stale pid cleanup, foreign live PID treated as stale, non-UTF-8 pid
file, password file, URL, name validation. The real-server tests run only when `PG_BIN_DIR` is
set: roundtrip, data path with spaces and accents, stale pid, hard-kill recovery, foreign
Postgres PID never signalled, password required and reused, wrong password fails fast,
dump/restore:
`$env:PG_BIN_DIR = "$PWD\target\postgres\bin"; cargo test -p storm-codex-supervisor --test postgres_it`.

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
