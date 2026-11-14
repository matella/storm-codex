//! Postgres lifecycle tests.
//!
//! `wrong_major_version_is_refused` needs no binaries and always runs. Every other test is skipped
//! unless `PG_BIN_DIR` points at a Postgres 17 `bin/` directory (same pattern as the projection
//! test with `DATABASE_URL`). Each test uses its own temporary data root and a free port, and a
//! guard stops the server it started even when an assertion fails.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use storm_codex_supervisor::{
    DataDir, PgError, Postgres, ToolPaths, APP_DATABASE, IMPORT_ROLE, PREVIOUS_DATABASE,
};
use sysinfo::{Pid, ProcessesToUpdate, System};

fn bin_dir() -> Option<PathBuf> {
    match std::env::var_os("PG_BIN_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => {
            eprintln!("PG_BIN_DIR not set: Postgres integration test skipped");
            None
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// Stops the server on drop, so a failing assertion never leaks a `postgres` process.
struct Running<'a>(&'a Postgres);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.0.stop() {
            eprintln!("stop on drop failed: {e}");
        }
    }
}

fn layout(root: &Path, bin: PathBuf) -> (DataDir, Postgres) {
    let data = DataDir::new(root.to_path_buf());
    data.ensure().expect("ensure data dir");
    let pg = Postgres::new(bin, data.pg(), free_port());
    (data, pg)
}

/// Runs `psql -w` (never prompts) against `url`, with no inherited password.
fn psql_url(bin: &Path, url: &str, sql: &str) -> std::process::Output {
    let exe = bin.join(format!("psql{}", std::env::consts::EXE_SUFFIX));
    Command::new(exe)
        .args(["-X", "-w", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"])
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE")
        .env("PGCONNECT_TIMEOUT", "10")
        .arg("-d")
        .arg(url)
        .arg("-c")
        .arg(sql)
        .output()
        .expect("run psql")
}

/// Runs one SQL statement with the bundled `psql` and returns its unaligned output.
fn psql(bin: &Path, pg: &Postgres, db: &str, sql: &str) -> String {
    let out = psql_url(bin, &pg.database_url(db), sql);
    assert!(
        out.status.success(),
        "psql failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn password_file(data: &DataDir) -> PathBuf {
    data.pg().with_file_name("pg-password")
}

fn read_pid(data_dir: &Path) -> u32 {
    // Bytes: line 2 (data directory) is in the ANSI code page on Windows, not UTF-8.
    let bytes = std::fs::read(data_dir.join("postmaster.pid")).expect("postmaster.pid");
    let first = bytes.split(|&b| b == b'\n').next().expect("first line");
    std::str::from_utf8(first)
        .expect("ascii pid")
        .trim()
        .parse()
        .expect("pid")
}

/// Kills the postmaster and all of its children, as a crash or power loss would, and waits
/// until they are gone.
fn hard_kill(postmaster: u32) {
    let root = Pid::from_u32(postmaster);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let victims: Vec<Pid> = sys
        .processes()
        .values()
        .filter(|p| p.pid() == root || p.parent() == Some(root))
        .map(|p| p.pid())
        .collect();
    assert!(victims.contains(&root), "postmaster {postmaster} not found");
    for pid in &victims {
        if let Some(p) = sys.process(*pid) {
            p.kill();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        sys.refresh_processes(ProcessesToUpdate::Some(&victims), true);
        if victims.iter().all(|pid| sys.process(*pid).is_none()) {
            return;
        }
        assert!(Instant::now() < deadline, "killed processes still alive");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A PID that is not running: the PID of a child process that already exited.
fn dead_pid() -> u32 {
    let mut child = if cfg!(windows) {
        Command::new("cmd").args(["/C", "exit 0"]).spawn()
    } else {
        Command::new("true").spawn()
    }
    .expect("spawn short-lived process");
    let pid = child.id();
    child.wait().expect("wait");
    pid
}

#[test]
fn wrong_major_version_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("pg");
    std::fs::create_dir_all(&data_dir).expect("mkdir");
    std::fs::write(data_dir.join("PG_VERSION"), "16\n").expect("write PG_VERSION");
    // No binaries at all: the version check must come first.
    let pg = Postgres::new(tmp.path().join("no-bin"), data_dir, free_port());

    match pg.init_if_needed() {
        Err(PgError::WrongMajorVersion { found, .. }) => assert_eq!(found, "16"),
        other => panic!("expected WrongMajorVersion, got {other:?}"),
    }
    match pg.start() {
        Err(PgError::WrongMajorVersion { found, .. }) => assert_eq!(found, "16"),
        other => panic!("expected WrongMajorVersion on start, got {other:?}"),
    }
}

#[test]
fn missing_binary_is_reported() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pg = Postgres::new(
        tmp.path().join("no-bin"),
        tmp.path().join("pg"),
        free_port(),
    );
    match pg.init_if_needed() {
        Err(PgError::MissingBinary(path)) => assert!(path.starts_with(tmp.path().join("no-bin"))),
        other => panic!("expected MissingBinary, got {other:?}"),
    }
}

#[test]
fn database_url_is_loopback_with_storm_user() {
    let pg = Postgres::new(PathBuf::from("bin"), PathBuf::from("pg"), 54321);
    assert_eq!(
        pg.database_url("storm_codex"),
        "postgres://storm@127.0.0.1:54321/storm_codex"
    );
}

#[test]
fn invalid_database_name_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pg = Postgres::new(
        tmp.path().join("no-bin"),
        tmp.path().join("pg"),
        free_port(),
    );
    assert!(matches!(
        pg.ensure_database("x'; DROP DATABASE postgres; --"),
        Err(PgError::InvalidDatabaseName(_))
    ));
}

#[test]
fn stop_without_running_server_is_a_no_op() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pg = Postgres::new(
        tmp.path().join("no-bin"),
        tmp.path().join("pg"),
        free_port(),
    );
    pg.stop().expect("stop is idempotent");
}

#[test]
fn clean_stale_removes_pid_file_of_dead_process() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("pg");
    std::fs::create_dir_all(&data_dir).expect("mkdir");
    let pid_file = data_dir.join("postmaster.pid");
    std::fs::write(
        &pid_file,
        format!("{}\n{}\n", dead_pid(), data_dir.display()),
    )
    .expect("write");
    let pg = Postgres::new(tmp.path().join("no-bin"), data_dir, free_port());
    pg.clean_stale().expect("clean_stale");
    assert!(!pid_file.exists(), "stale postmaster.pid must be removed");
    // Nothing to clean is fine too.
    pg.clean_stale().expect("clean_stale without pid file");
}

#[test]
fn init_start_stop_roundtrip() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data, pg) = layout(tmp.path(), bin.clone());

    pg.init_if_needed().expect("init");
    assert_eq!(
        std::fs::read_to_string(data.pg().join("PG_VERSION"))
            .expect("PG_VERSION")
            .trim(),
        "17"
    );
    pg.init_if_needed()
        .expect("init is a no-op the second time");

    pg.start().expect("start");
    let running = Running(&pg);
    assert!(data.logs().join("postgres.log").is_file(), "server log");
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    pg.ensure_database(APP_DATABASE)
        .expect("ensure_database is idempotent");
    assert_eq!(psql(&bin, &pg, APP_DATABASE, "SELECT 1"), "1");
    assert_eq!(
        psql(&bin, &pg, APP_DATABASE, "SHOW listen_addresses"),
        "127.0.0.1"
    );
    drop(running);
    assert!(
        !data.pg().join("postmaster.pid").exists(),
        "stop removes postmaster.pid"
    );
}

#[test]
fn data_dir_with_spaces_and_accents_works() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("Zoë Müller").join("App Data");
    let (data, pg) = layout(&root, bin.clone());

    pg.clean_stale().expect("clean_stale");
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let _running = Running(&pg);
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    assert_eq!(psql(&bin, &pg, APP_DATABASE, "SELECT 40 + 2"), "42");
    // postmaster.pid holds the accented path in the ANSI code page: the live server must
    // still be recognised as ours (kept by clean_stale) and stopped for real.
    pg.clean_stale().expect("clean_stale while running");
    assert!(data.pg().join("postmaster.pid").exists());
    let postmaster = read_pid(&data.pg());
    pg.stop().expect("stop");
    assert!(!data.pg().join("postmaster.pid").exists());
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(postmaster)]), true);
    assert!(
        sys.process(Pid::from_u32(postmaster)).is_none(),
        "postmaster must be gone after stop"
    );
}

/// Full cycle (init, start, query, ownership check, stop) on `pg`, whose data root is `data`.
fn full_cycle(bin: &Path, data: &DataDir, pg: &Postgres) {
    pg.clean_stale().expect("clean_stale");
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let running = Running(pg);
    assert!(data.logs().join("postgres.log").is_file(), "server log");
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    assert_eq!(psql(bin, pg, APP_DATABASE, "SELECT 40 + 2"), "42");
    // the running server is recognised as ours whatever path form it was started with
    pg.clean_stale().expect("clean_stale while running");
    assert!(data.pg().join("postmaster.pid").exists());
    let out = data.logs().join("dump.bin");
    pg.dump(&out).expect("dump");
    assert!(out.is_file());
    drop(running);
    assert!(!data.pg().join("postmaster.pid").exists());
}

/// Copies `bin/`, `lib/` and `share/` of the Postgres install holding `bin` under `dest`;
/// returns the copied `bin/`.
fn copy_postgres(bin: &Path, dest: &Path) -> PathBuf {
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("mkdir");
        for entry in std::fs::read_dir(from).expect("read_dir") {
            let entry = entry.expect("entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("type").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("copy");
            }
        }
    }
    let install = bin.parent().expect("bin has a parent");
    for part in ["bin", "lib", "share"] {
        copy_dir(&install.join(part), &dest.join(part));
    }
    dest.join("bin")
}

/// A Windows profile named in a script outside the ANSI code page (Cyrillic on a Western
/// Windows): Postgres' tools receive narrow `argv`, so the data root must reach them in a form
/// that survives the code page.
#[test]
fn data_root_outside_the_ansi_code_page_works() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("Пользователь é").join("Данные");
    let (data, pg) = layout(&root, bin.clone());
    full_cycle(&bin, &data, &pg);
}

/// Same, with the Postgres binaries themselves under such a folder (the installer puts them
/// in the user's `%LOCALAPPDATA%`): Postgres finds `share/` from its own module path.
#[test]
fn binaries_outside_the_ansi_code_page_work() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let profile = tmp.path().join("Пользователь");
    let cyr_bin = copy_postgres(&bin, &profile.join("Programs").join("postgres"));
    let (data, pg) = layout(&profile.join("Data"), cyr_bin);
    full_cycle(&bin, &data, &pg);
}

/// Volumes without 8.3 names: the tools go through a private junction. The cluster stays in
/// the data root (the same files with or without the junction), and a cluster made through the
/// junction is then run through short names, and back, with nothing lost.
#[test]
fn without_short_names_a_junction_alias_is_used_and_modes_can_switch() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let profile = tmp.path().join("Пользователь");
    let cyr_bin = copy_postgres(&bin, &profile.join("Programs").join("postgres"));
    let root = profile.join("Data");
    let alias = ToolPaths::new(false, tmp.path().join("alias"));
    let (data, pg) = layout(&root, cyr_bin.clone());
    let pg = pg.with_tool_paths(alias.clone());
    full_cycle(&bin, &data, &pg);
    assert!(
        data.pg().join("PG_VERSION").is_file(),
        "the cluster lives in the data root, not in the alias folder"
    );
    // the same cluster with the default (short names) mode, then the alias again
    let (_, pg_short) = layout(&root, cyr_bin.clone());
    pg_short.start().expect("start with short names");
    let running = Running(&pg_short);
    assert_eq!(psql(&bin, &pg_short, APP_DATABASE, "SELECT 1"), "1");
    drop(running);
    let (_, pg_alias) = layout(&root, cyr_bin);
    let pg_alias = pg_alias.with_tool_paths(alias);
    pg_alias.start().expect("start through the alias again");
    let _running = Running(&pg_alias);
    assert_eq!(psql(&bin, &pg_alias, APP_DATABASE, "SELECT 1"), "1");
}

#[test]
fn stale_pid_file_is_cleaned_and_start_succeeds() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data, pg) = layout(tmp.path(), bin.clone());
    pg.init_if_needed().expect("init");

    let pid_file = data.pg().join("postmaster.pid");
    std::fs::write(
        &pid_file,
        format!("{}\n{}\n", dead_pid(), data.pg().display()),
    )
    .expect("write stale pid file");
    pg.clean_stale().expect("clean_stale");
    assert!(!pid_file.exists());

    pg.start().expect("start");
    let _running = Running(&pg);
    assert_eq!(psql(&bin, &pg, "postgres", "SELECT 1"), "1");
    // A live server's pid file is ours: clean_stale must leave it alone.
    pg.clean_stale().expect("clean_stale while running");
    assert!(pid_file.exists(), "live postmaster.pid must be kept");
}

#[test]
fn start_after_hard_kill_recovers() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data, pg) = layout(tmp.path(), bin.clone());
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let _running = Running(&pg);
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    psql(
        &bin,
        &pg,
        APP_DATABASE,
        "CREATE TABLE survivor (id int); INSERT INTO survivor VALUES (7)",
    );

    hard_kill(read_pid(&data.pg()));
    assert!(
        data.pg().join("postmaster.pid").exists(),
        "a hard kill leaves postmaster.pid behind"
    );

    pg.clean_stale().expect("clean_stale");
    pg.init_if_needed().expect("init is a no-op");
    pg.start().expect("start after hard kill");
    assert_eq!(
        psql(&bin, &pg, APP_DATABASE, "SELECT id FROM survivor"),
        "7"
    );
}

#[test]
fn dump_holds_data_only() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_data, pg) = layout(&tmp.path().join("src"), bin.clone());
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let _running = Running(&pg);
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    psql(
        &bin,
        &pg,
        APP_DATABASE,
        "CREATE TABLE cast_row (name text); INSERT INTO cast_row VALUES ('Zoe')",
    );
    let dump = tmp.path().join("export dir").join("db.dump");
    std::fs::create_dir_all(dump.parent().expect("parent")).expect("mkdir");
    pg.dump(&dump).expect("dump");
    let out = Command::new(bin.join(format!("pg_restore{}", std::env::consts::EXE_SUFFIX)))
        .arg("-l")
        .arg(&dump)
        .output()
        .expect("pg_restore -l");
    let list = String::from_utf8_lossy(&out.stdout);
    assert!(list.contains("TABLE DATA public cast_row"), "{list}");
    assert!(
        !list.contains(" TABLE public cast_row"),
        "schema in a backup: {list}"
    );
}

#[test]
fn pid_file_of_a_live_process_that_is_not_ours_is_stale() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("pg");
    std::fs::create_dir_all(&data_dir).expect("mkdir");
    let pid_file = data_dir.join("postmaster.pid");
    // The test process is alive but is not our postmaster.
    let foreign = format!("{}\n{}\n", std::process::id(), data_dir.display());

    std::fs::write(&pid_file, &foreign).expect("write");
    let pg = Postgres::new(tmp.path().join("no-bin"), data_dir, free_port());
    pg.clean_stale().expect("clean_stale");
    assert!(!pid_file.exists(), "foreign PID: pid file must be removed");

    // stop must not signal it either (no binaries exist, so any signal attempt would error).
    std::fs::write(&pid_file, &foreign).expect("write");
    pg.stop().expect("stop treats a foreign PID as stale");
    assert!(!pid_file.exists());
}

#[test]
fn foreign_postgres_pid_is_never_signalled() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data_a, a) = layout(&tmp.path().join("a"), bin.clone());
    let (data_b, b) = layout(&tmp.path().join("b"), bin.clone());
    a.init_if_needed().expect("init a");
    b.init_if_needed().expect("init b");
    a.start().expect("start a");
    let _running_a = Running(&a);
    let a_pid = read_pid(&data_a.pg());
    let b_pid_file = data_b.pg().join("postmaster.pid");
    // B's pid file names A's live postmaster (same binaries, other data dir).
    let forged = format!("{a_pid}\n{}\n", data_b.pg().display());

    std::fs::write(&b_pid_file, &forged).expect("forge");
    b.clean_stale().expect("clean_stale b");
    assert!(!b_pid_file.exists(), "A's PID is not B's postmaster");

    std::fs::write(&b_pid_file, &forged).expect("forge again");
    b.start().expect("start b over a forged pid file");
    let _running_b = Running(&b);
    assert_ne!(read_pid(&data_b.pg()), a_pid);
    assert_eq!(read_pid(&data_a.pg()), a_pid, "A untouched");
    assert_eq!(
        psql(&bin, &a, "postgres", "SELECT 1"),
        "1",
        "A still serving"
    );
    assert_eq!(psql(&bin, &b, "postgres", "SELECT 2"), "2");
}

#[test]
fn password_is_generated_once_and_required() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data, pg) = layout(tmp.path(), bin.clone());
    pg.init_if_needed().expect("init");

    let pw_path = password_file(&data);
    let password = std::fs::read_to_string(&pw_path).expect("password file next to pg/");
    let password = password.trim().to_string();
    assert!(password.len() >= 64, "at least 32 random bytes, hex");
    assert!(password.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(
        !data.pg().join("pg-password").exists(),
        "not inside the cluster"
    );

    pg.start().expect("start");
    let running = Running(&pg);
    let url = pg.database_url("postgres");
    assert!(url.contains(&password), "database_url carries the password");
    let without = url.replace(&format!(":{password}@"), "@");
    let out = psql_url(&bin, &without, "SELECT 1");
    assert!(!out.status.success(), "no password must be refused");
    assert_eq!(psql(&bin, &pg, "postgres", "SELECT 1"), "1");
    drop(running);

    pg.init_if_needed().expect("init again is a no-op");
    pg.start().expect("restart");
    let _running = Running(&pg);
    assert_eq!(
        std::fs::read_to_string(&pw_path).expect("reread").trim(),
        password,
        "password file is reused"
    );
    assert_eq!(psql(&bin, &pg, "postgres", "SELECT 1"), "1");
}

#[test]
fn wrong_password_fails_fast_instead_of_prompting() {
    let Some(bin) = bin_dir() else { return };
    let tmp = tempfile::tempdir().expect("tempdir");
    let (data, pg) = layout(tmp.path(), bin);
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let _running = Running(&pg);

    let pw_path = password_file(&data);
    let good = std::fs::read_to_string(&pw_path).expect("password");
    std::fs::write(&pw_path, "00".repeat(32)).expect("tamper");
    let t0 = Instant::now();
    let result = pg.ensure_database(APP_DATABASE);
    let elapsed = t0.elapsed();
    std::fs::write(&pw_path, &good).expect("restore password");

    match result {
        Err(PgError::CommandFailed { stderr, .. }) => {
            assert!(
                !stderr.contains(good.trim()),
                "the secret never appears in errors"
            );
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
    assert!(elapsed < Duration::from_secs(15), "took {elapsed:?}");
    pg.ensure_database(APP_DATABASE)
        .expect("works again with the right password");
}

// ---- Import of an untrusted backup (data only, schema from the app's migrations) ----
//
// Security tests: `#[ignore]`d so a run without Postgres never reports them as passed; run
// with `PG_BIN_DIR=<Postgres 17 bin/> cargo test -p storm-codex-supervisor -- --include-ignored`.

fn required_bin() -> PathBuf {
    bin_dir().expect("PG_BIN_DIR must point at Postgres 17's bin/ for this test")
}

/// The server's migrations (`crates/storm-codex-server/migrations/NNNN_*.sql`), by version.
fn migration_files() -> Vec<(i64, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../storm-codex-server/migrations");
    let mut files: Vec<(i64, PathBuf)> = std::fs::read_dir(&dir)
        .expect("migrations dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .map(|p| {
            let name = p.file_name().expect("name").to_string_lossy().to_string();
            let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
            (digits.parse().expect("version prefix"), p)
        })
        .collect();
    files.sort();
    assert!(!files.is_empty());
    files
}

fn psql_ok(bin: &Path, url: &str, sql: &str) -> String {
    let out = psql_url(bin, url, sql);
    assert!(
        out.status.success(),
        "{sql}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// What the server's `migrate [--to N]` does (sqlx semantics: same ledger table, versions
/// applied in order up to `target`; a target above the newest migration is refused), with
/// psql and a deterministic checksum per version.
fn psql_migrate(bin: PathBuf) -> impl Fn(&str, Option<i64>) -> Result<(), String> {
    move |url: &str, target: Option<i64>| {
        let files = migration_files();
        let newest = files.last().map(|(v, _)| *v).unwrap_or_default();
        if let Some(t) = target.filter(|t| *t > newest) {
            return Err(format!(
                "database version {t} is newer than this build's ({newest})"
            ));
        }
        psql_ok(
            &bin,
            url,
            "CREATE TABLE IF NOT EXISTS _sqlx_migrations (version BIGINT PRIMARY KEY, \
             description TEXT NOT NULL, installed_on TIMESTAMPTZ NOT NULL DEFAULT now(), \
             success BOOLEAN NOT NULL, checksum BYTEA NOT NULL, execution_time BIGINT NOT NULL)",
        );
        let applied = psql_ok(&bin, url, "SELECT version FROM _sqlx_migrations");
        let applied: Vec<i64> = applied.lines().filter_map(|l| l.parse().ok()).collect();
        for (v, file) in files {
            if target.is_some_and(|t| v > t) || applied.contains(&v) {
                continue;
            }
            let out = Command::new(bin.join(format!("psql{}", std::env::consts::EXE_SUFFIX)))
                .args(["-X", "-w", "-q", "-v", "ON_ERROR_STOP=1", "-d"])
                .arg(url)
                .arg("-f")
                .arg(&file)
                .env_remove("PGPASSWORD")
                .env("PGCLIENTENCODING", "UTF8")
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).into_owned());
            }
            psql_ok(
                &bin,
                url,
                &format!(
                    "INSERT INTO _sqlx_migrations (version, description, success, checksum, \
                     execution_time) VALUES ({v}, 'm{v}', true, decode(md5('m{v}'), 'hex'), 1)"
                ),
            );
        }
        Ok(())
    }
}

/// A running cluster with the app database, for the import tests.
fn cluster(root: &Path, bin: &Path) -> Postgres {
    let (_data, pg) = layout(root, bin.to_path_buf());
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    pg
}

fn db_exists(bin: &Path, pg: &Postgres, name: &str) -> bool {
    psql(
        bin,
        pg,
        "postgres",
        &format!("SELECT count(*) FROM pg_database WHERE datname = '{name}'"),
    ) == "1"
}

fn role_exists(bin: &Path, pg: &Postgres) -> bool {
    psql(
        bin,
        pg,
        "postgres",
        &format!("SELECT count(*) FROM pg_roles WHERE rolname = '{IMPORT_ROLE}'"),
    ) == "1"
}

/// Backup (data-only dump) of a source app database migrated up to `version` (all if
/// `None`), after `sql`.
fn backup_of(bin: &Path, root: &Path, version: Option<i64>, sql: &str) -> PathBuf {
    let pg = cluster(&root.join("src"), bin);
    let _running = Running(&pg);
    psql_migrate(bin.to_path_buf())(&pg.database_url(APP_DATABASE), version).expect("migrate");
    psql(bin, &pg, APP_DATABASE, sql);
    let dump = root.join("db.dump");
    pg.dump(&dump).expect("dump");
    dump
}

/// Destination cluster with a migrated app database holding the marker setting `old`.
fn destination(bin: &Path, root: &Path) -> Postgres {
    let pg = cluster(&root.join("dst"), bin);
    psql_migrate(bin.to_path_buf())(&pg.database_url(APP_DATABASE), None).expect("migrate");
    psql(
        bin,
        &pg,
        APP_DATABASE,
        "INSERT INTO app_settings VALUES ('marker', '\"old\"') \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    );
    pg
}

fn marker(bin: &Path, pg: &Postgres) -> String {
    psql(
        bin,
        pg,
        APP_DATABASE,
        "SELECT value #>> '{}' FROM app_settings WHERE key = 'marker'",
    )
}

fn assert_untouched(bin: &Path, pg: &Postgres) {
    assert_eq!(marker(bin, pg), "old", "the app database was touched");
    assert!(
        !pg.staging_exists().expect("staging"),
        "staging left behind"
    );
    assert!(!role_exists(bin, pg), "import role left behind");
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn import_restores_data_into_a_migrated_staging_database_then_swaps() {
    let bin = required_bin();
    let tmp = tempfile::tempdir().expect("tempdir");
    let dump = backup_of(
        &bin,
        tmp.path(),
        None,
        "INSERT INTO app_settings VALUES ('marker', '\"new\"'); \
         UPDATE app_settings SET value = '[\"zoe\"]' WHERE key = 'operator_names'",
    );
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    let migrate = psql_migrate(bin.clone());

    pg.stage_import(&dump, &migrate).expect("stage");
    assert!(pg.staging_exists().expect("staging"));
    assert!(
        !role_exists(&bin, &pg),
        "the role is dropped right after the restore"
    );
    assert_eq!(marker(&bin, &pg), "old", "untouched until the swap");
    pg.promote_import().expect("promote");
    assert!(!pg.staging_exists().expect("staging"));
    assert!(!db_exists(&bin, &pg, PREVIOUS_DATABASE));
    assert_eq!(marker(&bin, &pg), "new");
    // The seeded row was replaced by the backup's, not duplicated.
    assert_eq!(
        psql(
            &bin,
            &pg,
            APP_DATABASE,
            "SELECT value::text FROM app_settings WHERE key = 'operator_names'"
        ),
        "[\"zoe\"]"
    );
    // Schema and ledger are the app's own; everything belongs to the superuser.
    assert_eq!(
        psql(
            &bin,
            &pg,
            APP_DATABASE,
            "SELECT count(*) FROM _sqlx_migrations"
        ),
        migration_files().len().to_string()
    );
    assert_eq!(
        psql(
            &bin,
            &pg,
            APP_DATABASE,
            "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tableowner <> 'storm'"
        ),
        "0"
    );
    pg.finish_import().expect("finish again (idempotent)");
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn an_older_backup_is_migrated_forward() {
    let bin = required_bin();
    let files = migration_files();
    let older = files[files.len() - 2].0;
    let tmp = tempfile::tempdir().expect("tempdir");
    let dump = backup_of(
        &bin,
        tmp.path(),
        Some(older),
        "INSERT INTO app_settings VALUES ('marker', '\"new\"')",
    );
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    pg.stage_import(&dump, &psql_migrate(bin.clone()))
        .expect("stage");
    pg.promote_import().expect("promote");
    assert_eq!(marker(&bin, &pg), "new");
    assert_eq!(
        psql(
            &bin,
            &pg,
            APP_DATABASE,
            "SELECT max(version) FROM _sqlx_migrations"
        ),
        files.last().expect("last").0.to_string()
    );
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn a_backup_from_a_newer_app_is_refused() {
    let bin = required_bin();
    let tmp = tempfile::tempdir().expect("tempdir");
    let dump = backup_of(
        &bin,
        tmp.path(),
        None,
        "INSERT INTO _sqlx_migrations VALUES (99999, 'future', now(), true, '\\x00', 1)",
    );
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    let err = pg
        .stage_import(&dump, &psql_migrate(bin.clone()))
        .expect_err("newer");
    assert!(matches!(err, PgError::Migrate(_)), "{err:?}");
    assert!(err.to_string().contains("newer"), "{err}");
    assert_untouched(&bin, &pg);
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn a_dump_with_any_schema_object_is_refused_before_anything_runs() {
    let bin = required_bin();
    let tmp = tempfile::tempdir().expect("tempdir");
    // A full (schema + data) dump, as a crafted backup would hold, with a domain whose
    // default runs a superuser-only function (`lo_export` would work the same way).
    let src = cluster(&tmp.path().join("src"), &bin);
    let full = {
        let _running = Running(&src);
        psql_migrate(bin.clone())(&src.database_url(APP_DATABASE), None).expect("migrate");
        psql(
            &bin,
            &src,
            APP_DATABASE,
            "CREATE DOMAIN d2 AS text DEFAULT pg_read_file('PG_VERSION'); \
             ALTER TABLE app_settings ADD COLUMN y d2",
        );
        let full = tmp.path().join("full.dump");
        let out = Command::new(bin.join(format!("pg_dump{}", std::env::consts::EXE_SUFFIX)))
            .args(["-Fc", "-f"])
            .arg(&full)
            .arg("-d")
            .arg(src.database_url(APP_DATABASE))
            .env_remove("PGPASSWORD")
            .output()
            .expect("pg_dump");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        full
    };
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    let err = pg
        .stage_import(&full, &|_, _| panic!("must be refused before migrating"))
        .expect_err("schema objects");
    assert!(matches!(err, PgError::UnsafeDump(_)), "{err:?}");
    assert!(err.to_string().contains("DOMAIN public d2"), "{err}");
    assert!(
        err.to_string().contains("TABLE public app_settings"),
        "{err}"
    );
    assert_untouched(&bin, &pg);
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn sql_run_as_the_import_role_cannot_create_code_settings_or_grants() {
    let bin = required_bin();
    let tmp = tempfile::tempdir().expect("tempdir");
    let dump = backup_of(
        &bin,
        tmp.path(),
        None,
        "INSERT INTO app_settings VALUES ('marker', '\"new\"')",
    );
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    let pwned = tmp.path().join("pwned.txt");
    let pwned_sql = pwned
        .display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let role_url = std::cell::RefCell::new(String::new());
    let succeeded = std::cell::RefCell::new(Vec::new());
    let hook = |url: &str| {
        *role_url.borrow_mut() = url.to_string();
        // What SQL hidden in a dump entry would try, as the import role.
        for sql in [
            format!(
                "CREATE DOMAIN public.d2 AS int DEFAULT pg_catalog.lo_export(1, '{pwned_sql}')"
            ),
            "CREATE FUNCTION public.lower(varchar) RETURNS text LANGUAGE sql AS 'SELECT 1'".into(),
            "CREATE SCHEMA evil".into(),
            "CREATE TABLE public.extra (x int)".into(),
            "CREATE TEMP TABLE t (x int)".into(),
            "ALTER TABLE public.app_settings ADD COLUMN y int".into(),
            "ALTER DATABASE storm_codex_import SET search_path = evil, pg_catalog".into(),
            "INSERT INTO public._sqlx_migrations VALUES (99999, 'x', now(), true, '\\x00', 1)"
                .into(),
            format!("COPY (SELECT 1) TO PROGRAM 'cmd /c echo x > \"{pwned_sql}\"'"),
            format!("COPY (SELECT 1) TO '{pwned_sql}'"),
            "SELECT pg_read_file('PG_VERSION')".into(),
            "ALTER SYSTEM SET work_mem = '1MB'".into(),
            "CREATE ROLE sneaky SUPERUSER".into(),
            "CREATE ROLE sneaky".into(),
        ] {
            if psql_url(&bin, url, &sql).status.success() {
                succeeded.borrow_mut().push(sql);
            }
        }
        // Grants without the privilege only warn ("no privileges were granted"): checked by
        // their effect below.
        for sql in [
            "GRANT ALL ON public.app_settings TO PUBLIC",
            "GRANT CREATE ON SCHEMA public TO PUBLIC",
            "GRANT CONNECT, TEMP ON DATABASE storm_codex_import TO PUBLIC",
        ] {
            let _ = psql_url(&bin, url, sql);
        }
        // What it is for: inserting rows.
        let out = psql_url(
            &bin,
            url,
            "INSERT INTO public.app_settings VALUES ('probe', '1')",
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = psql_url(
            &bin,
            url,
            "DELETE FROM public.app_settings WHERE key = 'probe'",
        );
        assert!(!out.status.success(), "the role can only insert");
    };
    pg.stage_import_with_hook(&dump, &psql_migrate(bin.clone()), &hook)
        .expect("stage: the role could only insert rows");
    assert!(succeeded.borrow().is_empty(), "{:?}", succeeded.borrow());
    assert_eq!(
        psql(
            &bin,
            &pg,
            storm_codex_supervisor::STAGING_DATABASE,
            "SELECT has_table_privilege('public', 'public.app_settings', 'INSERT') \
             OR has_table_privilege('public', 'public.app_settings', 'SELECT') \
             OR has_schema_privilege('public', 'public', 'CREATE') \
             OR has_database_privilege('public', 'storm_codex_import', 'CONNECT') \
             OR has_database_privilege('public', 'storm_codex_import', 'TEMP')"
        ),
        "f",
        "PUBLIC got a privilege"
    );
    assert!(!pwned.exists());
    // The role is gone and cannot log in any more.
    assert!(!role_exists(&bin, &pg));
    assert!(!psql_url(&bin, &role_url.borrow(), "SELECT 1")
        .status
        .success());
    pg.discard_import().expect("discard");
}

#[test]
#[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --include-ignored"]
fn a_large_object_created_during_the_restore_refuses_the_import() {
    let bin = required_bin();
    let tmp = tempfile::tempdir().expect("tempdir");
    let dump = backup_of(&bin, tmp.path(), None, "SELECT 1");
    let pg = destination(&bin, tmp.path());
    let _running = Running(&pg);
    let hook = |url: &str| {
        // Any role may create a large object: the ready-made payload for `lo_export`.
        psql_ok(&bin, url, "SELECT lo_from_bytea(0, 'echo pwned')");
    };
    let err = pg
        .stage_import_with_hook(&dump, &psql_migrate(bin.clone()), &hook)
        .expect_err("large object");
    assert!(matches!(err, PgError::UnsafeDump(_)), "{err:?}");
    assert!(err.to_string().contains("large object"), "{err}");
    assert_untouched(&bin, &pg);
}
