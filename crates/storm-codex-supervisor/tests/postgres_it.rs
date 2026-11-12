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

use storm_codex_supervisor::{DataDir, PgError, Postgres, APP_DATABASE};
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

/// Runs one SQL statement with the bundled `psql` and returns its unaligned output.
fn psql(bin: &Path, pg: &Postgres, db: &str, sql: &str) -> String {
    let exe = bin.join(format!("psql{}", std::env::consts::EXE_SUFFIX));
    let out = Command::new(exe)
        .args(["-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"])
        .arg("-d")
        .arg(pg.database_url(db))
        .arg("-c")
        .arg(sql)
        .output()
        .expect("run psql");
    assert!(
        out.status.success(),
        "psql failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn read_pid(data_dir: &Path) -> u32 {
    let text = std::fs::read_to_string(data_dir.join("postmaster.pid")).expect("postmaster.pid");
    text.lines()
        .next()
        .expect("first line")
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
    let (_data, pg) = layout(&root, bin.clone());

    pg.clean_stale().expect("clean_stale");
    pg.init_if_needed().expect("init");
    pg.start().expect("start");
    let _running = Running(&pg);
    pg.ensure_database(APP_DATABASE).expect("ensure_database");
    assert_eq!(psql(&bin, &pg, APP_DATABASE, "SELECT 40 + 2"), "42");
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
fn dump_then_restore_roundtrip() {
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
        // U&'Zo\00EB' = 'Zoë': psql receives argv in the ANSI code page on Windows, so the
        // non-ASCII value is written as a Unicode escape.
        r"CREATE TABLE cast_row (name text); INSERT INTO cast_row VALUES (U&'Zo\00EB')",
    );

    let dump = tmp.path().join("export dir").join("db.dump");
    std::fs::create_dir_all(dump.parent().expect("parent")).expect("mkdir");
    pg.dump(&dump).expect("dump");
    assert!(std::fs::metadata(&dump).expect("dump file").len() > 0);

    // Restore over modified data in the same cluster: --clean puts the dump back.
    psql(&bin, &pg, APP_DATABASE, "DELETE FROM cast_row");
    pg.restore(&dump).expect("restore in place");
    assert_eq!(
        psql(
            &bin,
            &pg,
            APP_DATABASE,
            r"SELECT count(*) FROM cast_row WHERE name = U&'Zo\00EB'"
        ),
        "1"
    );

    // Restore into a fresh cluster (the import path): the database is created if needed.
    let (_data2, pg2) = layout(&tmp.path().join("dst"), bin.clone());
    pg2.init_if_needed().expect("init 2");
    pg2.start().expect("start 2");
    let _running2 = Running(&pg2);
    pg2.restore(&dump).expect("restore into fresh cluster");
    assert_eq!(
        psql(&bin, &pg2, APP_DATABASE, "SELECT count(*) FROM cast_row"),
        "1"
    );
}
