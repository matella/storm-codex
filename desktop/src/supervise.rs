//! Starts and stops the standalone stack: the bundled Postgres cluster, then
//! `storm-codex-server` in standalone mode on a loopback port.
//!
//! Every side effect goes through the [`Db`], [`ServerChild`] and [`Host`] traits so the
//! ordering and failure paths are unit-tested with fakes; [`Stack::start`] wires the real ones.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use storm_codex_supervisor::{DataDir, PgError, Postgres, APP_DATABASE};

/// Server binary shipped in the resource folder.
pub const SERVER_EXE: &str = if cfg!(windows) {
    "storm-codex-server.exe"
} else {
    "storm-codex-server"
};
/// Preferred HTTP port of the server (the OBS URLs default to it); another free port otherwise.
pub const SERVER_PORT: u16 = 8088;
/// Preferred port of the bundled Postgres; uncommon on purpose so a system Postgres on
/// 5432/5433 is left alone. Another free port is used when taken.
pub const PG_PORT: u16 = 54329;
/// Password file written by `Postgres::init_if_needed`, a sibling of the cluster directory.
const PASSWORD_FILE: &str = "pg-password";

/// Server configuration variables the shell never passes through from its own environment:
/// the ones it sets itself and the ones that must stay unset in standalone mode (no Redis, no
/// admin token, no network reference-data source, no outgoing hooks).
pub const SERVER_ENV_REMOVED: &[&str] = &[
    "REDIS_URL",
    "JARVIS_CHANNEL",
    "ADMIN_TOKEN",
    "HOTSPATCHNOTES_URL",
    "REFERENTIAL_URL",
    "ORPHEUS_URL",
    "PATCH_WEBHOOK_URL",
    "AZURE_PUSH_URL",
    "AZURE_PUSH_TOKEN",
    "RAW_CACHE_MAX_BYTES",
];

/// How long `/api/health` may take to answer 200 after the server is spawned.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the server gets to exit after `POST /api/standalone/shutdown` (its import task
/// alone may take 10 s to flush the ledger) before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("could not create the data folder {path}: {source}")]
    DataDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "the database password file {password_file} is missing although a database exists in \
         {cluster}; the database cannot be opened without it"
    )]
    PasswordMissing {
        password_file: PathBuf,
        cluster: PathBuf,
    },
    #[error("Postgres: {0}")]
    Postgres(#[from] PgError),
    #[error("no free local port: {0}")]
    Port(#[source] io::Error),
    #[error("could not start {exe}: {source}")]
    Spawn {
        exe: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the server exited during startup ({status}); see {log}")]
    ServerExited { status: String, log: PathBuf },
    #[error("the server did not answer /api/health within {secs} s; see {log}")]
    NotHealthy { secs: u64, log: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub enum StopError {
    #[error("the server could not be stopped: {0}")]
    Server(#[source] io::Error),
    #[error("Postgres did not stop: {0}")]
    Postgres(#[source] PgError),
}

/// The Postgres operations the stack needs (implemented by [`Postgres`]).
pub trait Db: Send {
    fn clean_stale(&self) -> Result<(), PgError>;
    fn init_if_needed(&self) -> Result<(), PgError>;
    fn start(&self) -> Result<(), PgError>;
    fn ensure_database(&self, name: &str) -> Result<(), PgError>;
    fn stop(&self) -> Result<(), PgError>;
    /// Secret (holds the password): never log it.
    fn database_url(&self, db: &str) -> String;
}

impl Db for Postgres {
    fn clean_stale(&self) -> Result<(), PgError> {
        Postgres::clean_stale(self)
    }
    fn init_if_needed(&self) -> Result<(), PgError> {
        Postgres::init_if_needed(self)
    }
    fn start(&self) -> Result<(), PgError> {
        Postgres::start(self)
    }
    fn ensure_database(&self, name: &str) -> Result<(), PgError> {
        Postgres::ensure_database(self, name)
    }
    fn stop(&self) -> Result<(), PgError> {
        Postgres::stop(self)
    }
    fn database_url(&self, db: &str) -> String {
        Postgres::database_url(self, db)
    }
}

/// A running server process.
pub trait ServerChild: Send {
    /// `Some(exit status)` once the process has exited.
    fn try_wait(&mut self) -> io::Result<Option<String>>;
    /// Hard kill (last resort), then reap.
    fn kill(&mut self) -> io::Result<()>;
}

/// Everything the stack needs from the machine besides Postgres.
pub trait Host: Send + Sync {
    fn ensure_dirs(&self, data: &DataDir) -> io::Result<()>;
    fn pick_port(&self, preferred: u16) -> io::Result<u16>;
    fn open_db(&self, bin_dir: PathBuf, cluster: PathBuf, port: u16) -> Box<dyn Db>;
    fn spawn_server(&self, cmd: &ServerCommand) -> io::Result<Box<dyn ServerChild>>;
    /// True when `GET /api/health` answers 200.
    fn health_ok(&self, port: u16) -> bool;
    /// `POST /api/standalone/shutdown` (the server answers 202 and exits on its own).
    fn request_shutdown(&self, port: u16) -> io::Result<()>;
}

/// How the server is launched.
#[derive(Debug, Clone)]
pub struct ServerCommand {
    pub exe: PathBuf,
    /// Set on the child (values may hold secrets: `DATABASE_URL` carries the password).
    pub env: Vec<(&'static str, OsString)>,
    /// Removed from the inherited environment.
    pub env_removed: &'static [&'static str],
    pub cwd: PathBuf,
    /// stdout and stderr go there.
    pub log: PathBuf,
}

/// Wait limits (shortened in tests).
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    pub health_timeout: Duration,
    pub stop_timeout: Duration,
    pub poll: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            health_timeout: HEALTH_TIMEOUT,
            stop_timeout: STOP_TIMEOUT,
            poll: POLL_INTERVAL,
        }
    }
}

/// The server environment: standalone mode, loopback bind, data paths under the data root,
/// bundled resources. `REDIS_URL` and `ADMIN_TOKEN` are never set.
pub fn server_env(
    resource_dir: &Path,
    data: &DataDir,
    port: u16,
    database_url: String,
) -> Vec<(&'static str, OsString)> {
    vec![
        ("STANDALONE", "1".into()),
        ("BIND_ADDR", format!("127.0.0.1:{port}").into()),
        ("DATABASE_URL", database_url.into()),
        ("ARCHIVE_DIR", data.archive().into()),
        ("RAW_CACHE_DIR", data.raw_cache().into()),
        ("IMAGES_DIR", data.images().into()),
        ("LEDGER_FILE", data.ledger().into()),
        ("WEB_DIR", resource_dir.join("web").into()),
        ("MINIMAPS_BUNDLE_DIR", resource_dir.join("minimaps").into()),
        (
            "REFERENTIAL_FILE",
            resource_dir.join("referential.tar.gz").into(),
        ),
    ]
}

/// A cluster without its password file cannot be opened (scram auth with a generated
/// password): fail clearly instead of re-initialising. Recovery is a separate, explicit step.
fn check_password_file(data: &DataDir) -> Result<(), StartError> {
    let cluster = data.pg();
    let password_file = cluster.with_file_name(PASSWORD_FILE);
    if cluster.join("PG_VERSION").exists() && !password_file.exists() {
        return Err(StartError::PasswordMissing {
            password_file,
            cluster,
        });
    }
    Ok(())
}

pub struct Stack {
    port: u16,
    host: Arc<dyn Host>,
    db: Box<dyn Db>,
    server: Option<Box<dyn ServerChild>>,
    timings: Timings,
    stopped: bool,
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack")
            .field("port", &self.port)
            .field("running", &self.server.is_some())
            .finish()
    }
}

impl Stack {
    /// Starts Postgres from `<resource_dir>/postgres/bin` with its cluster in
    /// `<data_root>/pg`, then the server from `<resource_dir>`, and waits until it is healthy.
    pub fn start(resource_dir: &Path, data_root: &Path) -> Result<Stack, StartError> {
        Self::start_with(
            Arc::new(RealHost),
            resource_dir,
            data_root,
            Timings::default(),
        )
    }

    pub fn start_with(
        host: Arc<dyn Host>,
        resource_dir: &Path,
        data_root: &Path,
        timings: Timings,
    ) -> Result<Stack, StartError> {
        let data = DataDir::new(data_root.to_path_buf());
        host.ensure_dirs(&data)
            .map_err(|source| StartError::DataDir {
                path: data_root.to_path_buf(),
                source,
            })?;
        let pg_port = host.pick_port(PG_PORT).map_err(StartError::Port)?;
        let db = host.open_db(
            resource_dir.join("postgres").join("bin"),
            data.pg(),
            pg_port,
        );
        let mut stack = Stack {
            port: 0,
            host: host.clone(),
            db,
            server: None,
            timings,
            stopped: false,
        };
        // On any failure below, `stack` is dropped and its `Drop` stops whatever was started.
        let db = &*stack.db;
        db.clean_stale()?;
        check_password_file(&data)?;
        db.init_if_needed()?;
        db.start()?;
        db.ensure_database(APP_DATABASE)?;
        let port = host.pick_port(SERVER_PORT).map_err(StartError::Port)?;
        let cmd = ServerCommand {
            exe: resource_dir.join(SERVER_EXE),
            env: server_env(resource_dir, &data, port, db.database_url(APP_DATABASE)),
            env_removed: SERVER_ENV_REMOVED,
            cwd: data_root.to_path_buf(),
            log: data.logs().join("server.log"),
        };
        let child = host
            .spawn_server(&cmd)
            .map_err(|source| StartError::Spawn {
                exe: cmd.exe.clone(),
                source,
            })?;
        stack.port = port;
        stack.server = Some(child);
        stack.wait_healthy(&cmd.log)?;
        Ok(stack)
    }

    /// Polls `/api/health` until 200, the server exits, or the timeout. On failure the server
    /// is killed at once (it never became healthy: there is nothing to flush).
    fn wait_healthy(&mut self, log: &Path) -> Result<(), StartError> {
        let deadline = std::time::Instant::now() + self.timings.health_timeout;
        let result = loop {
            let Some(server) = self.server.as_mut() else {
                break Err(StartError::ServerExited {
                    status: "not running".into(),
                    log: log.to_path_buf(),
                });
            };
            match server.try_wait() {
                Ok(Some(status)) => {
                    self.server = None;
                    break Err(StartError::ServerExited {
                        status,
                        log: log.to_path_buf(),
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    break Err(StartError::ServerExited {
                        status: e.to_string(),
                        log: log.to_path_buf(),
                    })
                }
            }
            if self.host.health_ok(self.port) {
                break Ok(());
            }
            if std::time::Instant::now() >= deadline {
                break Err(StartError::NotHealthy {
                    secs: self.timings.health_timeout.as_secs(),
                    log: log.to_path_buf(),
                });
            }
            std::thread::sleep(self.timings.poll);
        };
        if result.is_err() {
            if let Some(mut server) = self.server.take() {
                let _ = server.kill();
            }
        }
        result
    }

    /// HTTP port of the running server (loopback).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops the server (graceful shutdown request, bounded wait, kill as a last resort), then
    /// Postgres (`pg_ctl stop -m fast`). Idempotent.
    pub fn stop(&mut self) -> Result<(), StopError> {
        if self.stopped {
            return Ok(());
        }
        let server = self.stop_server();
        // Postgres is stopped even when the server stop reported an error: the server process
        // is gone either way (killed), and leaving Postgres running helps nobody.
        self.db.stop().map_err(StopError::Postgres)?;
        self.stopped = true;
        server
    }

    /// `POST /api/standalone/shutdown`, wait up to `stop_timeout` for the exit, then kill.
    fn stop_server(&mut self) -> Result<(), StopError> {
        let Some(mut server) = self.server.take() else {
            return Ok(());
        };
        // A failed request (server already gone or wedged) falls through to the bounded wait
        // and the kill.
        let _ = self.host.request_shutdown(self.port);
        let deadline = std::time::Instant::now() + self.timings.stop_timeout;
        loop {
            match server.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(self.timings.poll)
                }
                _ => return server.kill().map_err(StopError::Server),
            }
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// The real machine: `DataDir`, `pick_port`, `Postgres`, a child process, plain HTTP/1.1 on
/// loopback.
struct RealHost;

impl Host for RealHost {
    fn ensure_dirs(&self, data: &DataDir) -> io::Result<()> {
        data.ensure()
    }
    fn pick_port(&self, preferred: u16) -> io::Result<u16> {
        storm_codex_supervisor::pick_port(preferred)
    }
    fn open_db(&self, bin_dir: PathBuf, cluster: PathBuf, port: u16) -> Box<dyn Db> {
        Box::new(Postgres::new(bin_dir, cluster, port))
    }
    fn spawn_server(&self, cmd: &ServerCommand) -> io::Result<Box<dyn ServerChild>> {
        // Keep the previous run's log for diagnosis, then start a fresh one.
        if cmd.log.exists() {
            let _ = std::fs::rename(&cmd.log, cmd.log.with_extension("prev.log"));
        }
        let out = std::fs::File::create(&cmd.log)?;
        let err = out.try_clone()?;
        let mut command = Command::new(&cmd.exe);
        for var in cmd.env_removed {
            command.env_remove(var);
        }
        command
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&cmd.cwd)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        crate::job::hide_console(&mut command);
        let child = command.spawn()?;
        // Killed with the shell if the shell dies without stopping it (hard kill, crash).
        let job = crate::job::KillOnClose::adopt(&child);
        Ok(Box::new(RealChild { child, _job: job }))
    }
    fn health_ok(&self, port: u16) -> bool {
        http_status(port, "GET", "/api/health").is_ok_and(|s| s == 200)
    }
    fn request_shutdown(&self, port: u16) -> io::Result<()> {
        match http_status(port, "POST", "/api/standalone/shutdown")? {
            200..=299 => Ok(()),
            s => Err(io::Error::other(format!("shutdown request answered {s}"))),
        }
    }
}

struct RealChild {
    child: std::process::Child,
    /// Dropped after the child is reaped; closing it kills the process if still alive.
    _job: Option<crate::job::KillOnClose>,
}

impl ServerChild for RealChild {
    fn try_wait(&mut self) -> io::Result<Option<String>> {
        Ok(self.child.try_wait()?.map(|s| s.to_string()))
    }
    fn kill(&mut self) -> io::Result<()> {
        // Already exited: `kill` fails with InvalidInput, which is not an error here.
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait().map(|_| ())
    }
}

/// One HTTP/1.1 request to `127.0.0.1:port` (empty body, `Connection: close`); returns the
/// status code. The `Host` header is `127.0.0.1:<port>`, which the standalone routes accept.
fn http_status(port: u16, method: &str, path: &str) -> io::Result<u16> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    // One write: the request reaches the server in a single segment.
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    // "HTTP/1.1 200 OK"
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP status line"))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    type Log = Arc<Mutex<Vec<String>>>;

    fn push(log: &Log, s: impl Into<String>) {
        log.lock().expect("log").push(s.into());
    }

    fn entries(log: &Log) -> Vec<String> {
        log.lock().expect("log").clone()
    }

    fn fast() -> Timings {
        Timings {
            health_timeout: Duration::from_millis(300),
            stop_timeout: Duration::from_millis(300),
            poll: Duration::from_millis(10),
        }
    }

    struct FakeDb {
        log: Log,
    }

    impl Db for FakeDb {
        fn clean_stale(&self) -> Result<(), PgError> {
            push(&self.log, "db.clean_stale");
            Ok(())
        }
        fn init_if_needed(&self) -> Result<(), PgError> {
            push(&self.log, "db.init_if_needed");
            Ok(())
        }
        fn start(&self) -> Result<(), PgError> {
            push(&self.log, "db.start");
            Ok(())
        }
        fn ensure_database(&self, name: &str) -> Result<(), PgError> {
            push(&self.log, format!("db.ensure_database({name})"));
            Ok(())
        }
        fn stop(&self) -> Result<(), PgError> {
            push(&self.log, "db.stop");
            Ok(())
        }
        fn database_url(&self, db: &str) -> String {
            format!("postgres://storm:secret@127.0.0.1:1/{db}")
        }
    }

    struct FakeChild {
        log: Log,
        shutdown_requested: Arc<AtomicBool>,
        honours_shutdown: bool,
        exits_at_once: bool,
        killed: bool,
    }

    impl ServerChild for FakeChild {
        fn try_wait(&mut self) -> io::Result<Option<String>> {
            let exited = self.killed
                || self.exits_at_once
                || (self.honours_shutdown && self.shutdown_requested.load(Ordering::SeqCst));
            Ok(exited.then(|| "exit code: 0".to_string()))
        }
        fn kill(&mut self) -> io::Result<()> {
            push(&self.log, "server.kill");
            self.killed = true;
            Ok(())
        }
    }

    struct FakeHost {
        log: Log,
        healthy: bool,
        honours_shutdown: bool,
        exits_at_once: bool,
        shutdown_requested: Arc<AtomicBool>,
        last_cmd: Mutex<Option<ServerCommand>>,
    }

    impl FakeHost {
        fn new(log: &Log) -> Self {
            Self {
                log: log.clone(),
                healthy: true,
                honours_shutdown: true,
                exits_at_once: false,
                shutdown_requested: Arc::new(AtomicBool::new(false)),
                last_cmd: Mutex::new(None),
            }
        }
    }

    impl Host for FakeHost {
        fn ensure_dirs(&self, data: &DataDir) -> io::Result<()> {
            push(&self.log, "ensure_dirs");
            data.ensure()
        }
        fn pick_port(&self, preferred: u16) -> io::Result<u16> {
            push(&self.log, format!("pick_port({preferred})"));
            Ok(preferred)
        }
        fn open_db(&self, _bin: PathBuf, _cluster: PathBuf, port: u16) -> Box<dyn Db> {
            push(&self.log, format!("open_db({port})"));
            Box::new(FakeDb {
                log: self.log.clone(),
            })
        }
        fn spawn_server(&self, cmd: &ServerCommand) -> io::Result<Box<dyn ServerChild>> {
            push(&self.log, "server.spawn");
            *self.last_cmd.lock().expect("cmd") = Some(cmd.clone());
            Ok(Box::new(FakeChild {
                log: self.log.clone(),
                shutdown_requested: self.shutdown_requested.clone(),
                honours_shutdown: self.honours_shutdown,
                exits_at_once: self.exits_at_once,
                killed: false,
            }))
        }
        fn health_ok(&self, _port: u16) -> bool {
            self.healthy
        }
        fn request_shutdown(&self, port: u16) -> io::Result<()> {
            push(&self.log, format!("server.shutdown({port})"));
            self.shutdown_requested.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn start(host: FakeHost, data: &Path) -> (Result<Stack, StartError>, Arc<FakeHost>) {
        let host = Arc::new(host);
        let res = Stack::start_with(host.clone(), Path::new("C:/res"), data, fast());
        (res, host)
    }

    #[test]
    fn start_order_is_cleanup_init_start_db_port_server() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let (stack, _host) = start(FakeHost::new(&log), tmp.path());
        let stack = stack.expect("start");
        assert_eq!(stack.port(), SERVER_PORT);
        assert_eq!(
            entries(&log),
            [
                "ensure_dirs".to_string(),
                format!("pick_port({PG_PORT})"),
                format!("open_db({PG_PORT})"),
                "db.clean_stale".into(),
                "db.init_if_needed".into(),
                "db.start".into(),
                format!("db.ensure_database({APP_DATABASE})"),
                format!("pick_port({SERVER_PORT})"),
                "server.spawn".into(),
            ]
        );
    }

    #[test]
    fn start_fails_with_clear_error_when_server_never_healthy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.healthy = false;
        host.honours_shutdown = false;
        let (res, _host) = start(host, tmp.path());
        let err = res.expect_err("must fail");
        assert!(matches!(err, StartError::NotHealthy { .. }), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("/api/health"), "{msg}");
        assert!(msg.contains("server.log"), "{msg}");
        // Nothing is left running: the server is killed, then Postgres stopped.
        let log = entries(&log);
        let kill = log.iter().position(|e| e == "server.kill").expect("kill");
        let stop = log.iter().position(|e| e == "db.stop").expect("db.stop");
        assert!(kill < stop, "{log:?}");
    }

    #[test]
    fn start_fails_when_server_exits_during_startup() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.healthy = false;
        host.exits_at_once = true;
        let (res, _host) = start(host, tmp.path());
        let err = res.expect_err("must fail");
        assert!(matches!(err, StartError::ServerExited { .. }), "{err:?}");
        assert!(entries(&log).contains(&"db.stop".to_string()));
    }

    #[test]
    fn start_fails_clearly_when_password_file_missing_but_cluster_exists() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cluster = tmp.path().join("pg");
        std::fs::create_dir_all(&cluster).expect("mkdir");
        std::fs::write(cluster.join("PG_VERSION"), "17\n").expect("write");
        let log = Log::default();
        let (res, _host) = start(FakeHost::new(&log), tmp.path());
        let err = res.expect_err("must fail");
        assert!(matches!(err, StartError::PasswordMissing { .. }), "{err:?}");
        assert!(err.to_string().contains("pg-password"), "{err}");
        // No init (which would silently create a new password) and no start.
        let log = entries(&log);
        assert!(!log
            .iter()
            .any(|e| e == "db.init_if_needed" || e == "db.start"));
    }

    #[test]
    fn stop_stops_server_before_postgres() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let (stack, _host) = start(FakeHost::new(&log), tmp.path());
        let mut stack = stack.expect("start");
        log.lock().expect("log").clear();
        stack.stop().expect("stop");
        assert_eq!(
            entries(&log),
            [
                format!("server.shutdown({SERVER_PORT})"),
                "db.stop".to_string()
            ]
        );
        // Idempotent.
        stack.stop().expect("stop again");
        assert_eq!(entries(&log).len(), 2);
    }

    #[test]
    fn stop_kills_a_server_that_ignores_shutdown_then_stops_postgres() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.honours_shutdown = false;
        let (stack, _host) = start(host, tmp.path());
        let mut stack = stack.expect("start");
        log.lock().expect("log").clear();
        stack.stop().expect("stop");
        assert_eq!(
            entries(&log),
            [
                format!("server.shutdown({SERVER_PORT})"),
                "server.kill".to_string(),
                "db.stop".to_string()
            ]
        );
    }

    #[test]
    fn http_status_reads_the_status_line_and_sends_a_loopback_host() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let mut n = 0;
            while !buf[..n].ends_with(b"\r\n\r\n") {
                n += conn.read(&mut buf[n..]).expect("read");
            }
            conn.write_all(b"HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\n\r\n")
                .expect("write");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        let status = http_status(port, "POST", "/api/standalone/shutdown").expect("status");
        assert_eq!(status, 202);
        let request = server.join().expect("join");
        assert!(request.starts_with("POST /api/standalone/shutdown HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        // Nothing listens any more: an error, not a hang.
        assert!(http_status(port, "GET", "/api/health").is_err());
    }

    #[test]
    fn env_has_no_redis_and_is_loopback() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let (stack, host) = start(FakeHost::new(&log), tmp.path());
        let _stack = stack.expect("start");
        let cmd = host.last_cmd.lock().expect("cmd").clone().expect("spawned");
        let get = |k: &str| {
            cmd.env
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(cmd.exe, Path::new("C:/res").join(SERVER_EXE));
        assert_eq!(get("STANDALONE").as_deref(), Some("1"));
        assert_eq!(get("BIND_ADDR"), Some(format!("127.0.0.1:{SERVER_PORT}")));
        assert!(get("REDIS_URL").is_none());
        assert!(get("ADMIN_TOKEN").is_none());
        assert!(cmd.env_removed.contains(&"REDIS_URL"));
        assert!(cmd.env_removed.contains(&"ADMIN_TOKEN"));
        let data = DataDir::new(tmp.path().to_path_buf());
        let path = |p: PathBuf| Some(p.to_string_lossy().into_owned());
        assert_eq!(get("ARCHIVE_DIR"), path(data.archive()));
        assert_eq!(get("RAW_CACHE_DIR"), path(data.raw_cache()));
        assert_eq!(get("IMAGES_DIR"), path(data.images()));
        assert_eq!(get("LEDGER_FILE"), path(data.ledger()));
        assert_eq!(get("WEB_DIR"), path(Path::new("C:/res").join("web")));
        assert_eq!(
            get("MINIMAPS_BUNDLE_DIR"),
            path(Path::new("C:/res").join("minimaps"))
        );
        assert_eq!(
            get("REFERENTIAL_FILE"),
            path(Path::new("C:/res").join("referential.tar.gz"))
        );
        assert!(get("DATABASE_URL").is_some_and(|u| u.contains("@127.0.0.1:")));
        // No variable is both set and removed.
        for (k, _) in &cmd.env {
            assert!(!cmd.env_removed.contains(k), "{k}");
        }
        assert_eq!(cmd.log, data.logs().join("server.log"));
    }
}
