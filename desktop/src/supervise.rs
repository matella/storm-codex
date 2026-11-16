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

use storm_codex_supervisor::{DataDir, Migrate, PgError, Postgres, APP_DATABASE};

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

/// The only variables of the shell's own environment passed to the server (allow-list: the
/// child starts from an empty environment). Everything else, the server configuration
/// variables included (`REDIS_URL`, `ADMIN_TOKEN`, `AZURE_PUSH_*`, `DATABASE_URL`, ...), never
/// reaches it.
/// - `SystemRoot`, `windir`: Windows system components (Winsock, CNG/crypto, COM) locate
///   their files through them; networking fails without `SystemRoot`.
/// - `TEMP`, `TMP`, `USERPROFILE`: `std::env::temp_dir()` (`GetTempPath2`) reads them in
///   this order; the server extracts the referential and decodes cast replays there.
///   Without them it would fall back to the Windows folder, which a user cannot write.
/// - `PATH`: the system DLL search order and any helper a dependency launches.
/// - `RUST_LOG`: log level for diagnosis (a log filter, no secret).
///
/// Not passed: `LOCALAPPDATA`/`APPDATA` (every data path is given explicitly; the replay
/// folders come from the Documents known-folder API, which does not read the environment).
pub const SERVER_ENV_PASSTHROUGH: &[&str] = &[
    "SystemRoot",
    "windir",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "PATH",
    "RUST_LOG",
];

/// How long `/api/health` may take to answer 200 after the server is spawned.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the server gets to exit after `POST /api/standalone/shutdown` (its import task
/// alone may take 10 s to flush the ledger) before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(20);
/// Windows force-closes applications a few seconds after `WM_ENDSESSION`: at session end the
/// server gets this long (counted from the shutdown request) before it is killed. Postgres is
/// then stopped fast; its crash recovery covers anything cut short.
const SESSION_END_STOP_TIMEOUT: Duration = Duration::from_secs(3);
/// Connect/read/write limit of the shutdown request (full budget / session end).
const SHUTDOWN_IO_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_END_IO_TIMEOUT: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Connect/read/write limit of one `/api/health` probe.
const HEALTH_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// How much time [`Stack::stop_with`] may spend on the server before killing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBudget {
    /// Quit / app exit: up to `stop_timeout` (20 s) so the import task can flush the ledger.
    Full,
    /// Windows session end: up to `session_end_stop_timeout` (3 s), then kill.
    SessionEnd,
}

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
    #[error("an interrupted import could not be finished or rolled back: {0}")]
    ImportRecovery(String),
}

#[derive(Debug, thiserror::Error)]
pub enum StopError {
    #[error("the server could not be stopped: {0}")]
    Server(#[source] io::Error),
    #[error("Postgres did not stop: {0}")]
    Postgres(#[source] PgError),
}

#[derive(Debug, thiserror::Error)]
pub enum RestartError {
    #[error("the server is still running")]
    StillRunning,
    #[error("the stack is stopped")]
    Stopped,
    #[error(transparent)]
    Start(#[from] StartError),
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
    /// `pg_dump -Fc --data-only` of the app database to `out`.
    fn dump(&self, out: &Path) -> Result<(), PgError>;
    /// Restores the data of an untrusted data-only dump into a staging database whose schema
    /// `migrate` created (TOC allow-list, unprivileged role: [`Postgres::stage_import`]). The
    /// app database is not touched.
    fn stage_import(&self, file: &Path, migrate: Migrate<'_>) -> Result<(), PgError>;
    /// True when a staged import exists.
    fn staging_exists(&self) -> Result<bool, PgError>;
    /// Swaps the staged database in (atomic), then [`finish_import`](Self::finish_import).
    fn promote_import(&self) -> Result<(), PgError>;
    /// Post-promote cleanup (idempotent).
    fn finish_import(&self) -> Result<(), PgError>;
    /// Drops the staging database, if any.
    fn discard_import(&self) -> Result<(), PgError>;
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
    fn dump(&self, out: &Path) -> Result<(), PgError> {
        Postgres::dump(self, out)
    }
    fn stage_import(&self, file: &Path, migrate: Migrate<'_>) -> Result<(), PgError> {
        Postgres::stage_import(self, file, migrate)
    }
    fn staging_exists(&self) -> Result<bool, PgError> {
        Postgres::staging_exists(self)
    }
    fn promote_import(&self) -> Result<(), PgError> {
        Postgres::promote_import(self)
    }
    fn finish_import(&self) -> Result<(), PgError> {
        Postgres::finish_import(self)
    }
    fn discard_import(&self) -> Result<(), PgError> {
        Postgres::discard_import(self)
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
    /// `POST /api/standalone/shutdown` (the server answers 202 and exits on its own), each
    /// network step limited to `io_timeout`.
    fn request_shutdown(&self, port: u16, io_timeout: Duration) -> io::Result<()>;
    /// Runs `<server> migrate [--to <target>]` on `database_url` (the server's environment
    /// otherwise) and waits for it. The message of an error is shown to the user.
    fn migrate(
        &self,
        cmd: &ServerCommand,
        database_url: &str,
        target: Option<i64>,
    ) -> Result<(), String>;
}

/// Exit code of `storm-codex-server migrate` when the target version is newer than the build.
const MIGRATE_EXIT_NEWER: i32 = 3;
/// Upper bound of one `migrate` run (a large database migrated forward).
const MIGRATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How the server is launched.
#[derive(Debug, Clone)]
pub struct ServerCommand {
    pub exe: PathBuf,
    /// The child's **whole** environment (it inherits nothing else). Values may hold secrets:
    /// `DATABASE_URL` carries the password.
    pub env: Vec<(&'static str, OsString)>,
    pub cwd: PathBuf,
    /// stdout and stderr go there.
    pub log: PathBuf,
}

/// Wait limits (shortened in tests).
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    pub health_timeout: Duration,
    pub stop_timeout: Duration,
    pub session_end_stop_timeout: Duration,
    pub poll: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            health_timeout: HEALTH_TIMEOUT,
            stop_timeout: STOP_TIMEOUT,
            session_end_stop_timeout: SESSION_END_STOP_TIMEOUT,
            poll: POLL_INTERVAL,
        }
    }
}

/// The server's whole environment: standalone mode, loopback bind, data paths under the data
/// root, bundled resources, plus the [`SERVER_ENV_PASSTHROUGH`] variables found by `inherited`
/// (the shell's environment in production). `REDIS_URL` and `ADMIN_TOKEN` are never set.
pub fn server_env(
    resource_dir: &Path,
    data: &DataDir,
    port: u16,
    database_url: String,
    inherited: impl Fn(&str) -> Option<OsString>,
) -> Vec<(&'static str, OsString)> {
    let mut env = vec![
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
    ];
    for &name in SERVER_ENV_PASSTHROUGH {
        if let Some(value) = inherited(name) {
            env.push((name, value));
        }
    }
    env
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
    /// How the server was launched (reused by [`Stack::restart_server`]).
    cmd: Option<ServerCommand>,
    data: DataDir,
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
            cmd: None,
            data: data.clone(),
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
        // Before the server touches the data: finish or roll back an import a crash cut short.
        crate::backup::recover_import(&data, db).map_err(StartError::ImportRecovery)?;
        let port = host.pick_port(SERVER_PORT).map_err(StartError::Port)?;
        let cmd = ServerCommand {
            exe: resource_dir.join(SERVER_EXE),
            env: server_env(
                resource_dir,
                &data,
                port,
                db.database_url(APP_DATABASE),
                |name| std::env::var_os(name),
            ),
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
        let log = cmd.log.clone();
        stack.cmd = Some(cmd);
        stack.wait_healthy(&log)?;
        Ok(stack)
    }

    /// Starts the server again after [`stop_server`](Self::stop_server) (Postgres kept
    /// running), same command and port, and waits until it is healthy.
    pub fn restart_server(&mut self) -> Result<(), RestartError> {
        if self.stopped {
            return Err(RestartError::Stopped);
        }
        if self.server.is_some() {
            return Err(RestartError::StillRunning);
        }
        let Some(cmd) = self.cmd.clone() else {
            return Err(RestartError::Stopped);
        };
        let child = self
            .host
            .spawn_server(&cmd)
            .map_err(|source| StartError::Spawn {
                exe: cmd.exe.clone(),
                source,
            })?;
        self.server = Some(child);
        self.wait_healthy(&cmd.log)?;
        Ok(())
    }

    /// The data folder layout of this stack.
    pub fn data(&self) -> &DataDir {
        &self.data
    }

    /// `pg_dump` of the app database (Postgres must be running; the server may be).
    pub fn dump(&self, out: &Path) -> Result<(), PgError> {
        self.db.dump(out)
    }

    /// Restores an untrusted data-only dump into the staging database (the server may keep
    /// running); the staging schema comes from the server binary's own migrations.
    pub fn stage_import(&self, file: &Path) -> Result<(), PgError> {
        let Some(cmd) = self.cmd.clone() else {
            return Err(PgError::Migrate("the stack is stopped".into()));
        };
        let host = self.host.clone();
        let migrate = move |url: &str, target: Option<i64>| host.migrate(&cmd, url, target);
        self.db.stage_import(file, &migrate)
    }

    /// True when a staged import exists.
    pub fn staging_exists(&self) -> Result<bool, PgError> {
        self.db.staging_exists()
    }

    /// Post-promote cleanup (idempotent).
    pub fn finish_import(&self) -> Result<(), PgError> {
        self.db.finish_import()
    }

    /// Swaps the staged import in. Stop the server first (nothing may be connected).
    pub fn promote_import(&self) -> Result<(), PgError> {
        self.db.promote_import()
    }

    /// Drops the staged import, if any.
    pub fn discard_import(&self) -> Result<(), PgError> {
        self.db.discard_import()
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

    /// [`stop_with`](Self::stop_with) the full budget.
    pub fn stop(&mut self) -> Result<(), StopError> {
        self.stop_with(StopBudget::Full)
    }

    /// Stops the server (graceful shutdown request, wait bounded by `budget`, kill as a last
    /// resort), then Postgres (`pg_ctl stop -m fast`). Idempotent.
    pub fn stop_with(&mut self, budget: StopBudget) -> Result<(), StopError> {
        if self.stopped {
            return Ok(());
        }
        let server = self.stop_server(budget);
        // Postgres is stopped even when the server stop reported an error: the server process
        // is gone either way (killed), and leaving Postgres running helps nobody.
        self.db.stop().map_err(StopError::Postgres)?;
        self.stopped = true;
        server
    }

    /// Stops the server only (Postgres keeps running): `POST /api/standalone/shutdown`, wait
    /// for the exit until the budget's deadline (counted from before the request), then kill.
    pub fn stop_server(&mut self, budget: StopBudget) -> Result<(), StopError> {
        let Some(mut server) = self.server.take() else {
            return Ok(());
        };
        let (limit, io_timeout) = match budget {
            StopBudget::Full => (self.timings.stop_timeout, SHUTDOWN_IO_TIMEOUT),
            StopBudget::SessionEnd => (
                self.timings.session_end_stop_timeout,
                SESSION_END_IO_TIMEOUT,
            ),
        };
        let deadline = std::time::Instant::now() + limit;
        // A failed request (server already gone or wedged) falls through to the bounded wait
        // and the kill.
        let _ = self.host.request_shutdown(self.port, io_timeout.min(limit));
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
        let mut command = server_command(cmd);
        command.stdout(out).stderr(err);
        let child = command.spawn()?;
        // Killed with the shell if the shell dies without stopping it (hard kill, crash).
        let job = crate::job::KillOnClose::adopt(&child);
        Ok(Box::new(RealChild { child, _job: job }))
    }
    fn health_ok(&self, port: u16) -> bool {
        http_status(port, "GET", "/api/health", HEALTH_IO_TIMEOUT).is_ok_and(|s| s == 200)
    }
    fn request_shutdown(&self, port: u16, io_timeout: Duration) -> io::Result<()> {
        match http_status(port, "POST", "/api/standalone/shutdown", io_timeout)? {
            200..=299 => Ok(()),
            s => Err(io::Error::other(format!("shutdown request answered {s}"))),
        }
    }
    fn migrate(
        &self,
        cmd: &ServerCommand,
        database_url: &str,
        target: Option<i64>,
    ) -> Result<(), String> {
        let log = cmd.log.with_file_name("migrate.log");
        let out = std::fs::File::create(&log)
            .map_err(|e| format!("could not create {}: {e}", log.display()))?;
        let err = out
            .try_clone()
            .map_err(|e| format!("could not open {}: {e}", log.display()))?;
        let mut command = server_command(cmd);
        // Overrides the app database URL of the server environment.
        command
            .env("DATABASE_URL", database_url)
            .arg("migrate")
            .stdout(out)
            .stderr(err);
        if let Some(t) = target {
            command.arg("--to").arg(t.to_string());
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", cmd.exe.display()))?;
        let _job = crate::job::KillOnClose::adopt(&child);
        let deadline = std::time::Instant::now() + MIGRATE_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(POLL_INTERVAL)
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "the database migration did not finish within {} min; see {}",
                        MIGRATE_TIMEOUT.as_secs() / 60,
                        log.display()
                    ));
                }
                Err(e) => return Err(format!("waiting for the database migration: {e}")),
            }
        };
        match status.code() {
            Some(0) => Ok(()),
            Some(MIGRATE_EXIT_NEWER) => Err(
                "this backup was made by a newer version of Storm Codex; update Storm Codex, \
                 then import it again"
                    .into(),
            ),
            _ => {
                let tail = std::fs::read_to_string(&log)
                    .map(|t| {
                        let lines: Vec<&str> = t.lines().rev().take(5).collect();
                        lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                    })
                    .unwrap_or_default();
                Err(format!(
                    "the database migration failed ({status}): {tail}; see {}",
                    log.display()
                ))
            }
        }
    }
}

/// The child process: empty environment plus exactly `cmd.env`, no console window.
fn server_command(cmd: &ServerCommand) -> Command {
    let mut command = Command::new(&cmd.exe);
    command
        .env_clear()
        .envs(cmd.env.iter().map(|(k, v)| (k, v)))
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null());
    crate::job::hide_console(&mut command);
    command
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
fn http_status(port: u16, method: &str, path: &str, io_timeout: Duration) -> io::Result<u16> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, io_timeout.min(Duration::from_secs(1)))?;
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
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

/// Test fakes shared with the lifecycle tests.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod fakes {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    pub type Log = Arc<Mutex<Vec<String>>>;

    pub fn push(log: &Log, s: impl Into<String>) {
        log.lock().expect("log").push(s.into());
    }

    pub fn entries(log: &Log) -> Vec<String> {
        log.lock().expect("log").clone()
    }

    pub fn fast() -> Timings {
        Timings {
            health_timeout: Duration::from_millis(300),
            stop_timeout: Duration::from_millis(300),
            session_end_stop_timeout: Duration::from_millis(50),
            poll: Duration::from_millis(10),
        }
    }

    pub struct FakeDb {
        pub log: Log,
        pub fail_stage: bool,
        pub fail_promote: bool,
        /// The rename commits, then the cleanup fails (the staging database is gone).
        pub fail_promote_after_rename: bool,
        /// Whether the staging database exists (shared with the host, so a test can set it).
        pub staged: Arc<AtomicBool>,
    }

    fn fake_failure(command: &str) -> PgError {
        PgError::CommandFailed {
            command: command.into(),
            status: "exit code: 1".into(),
            stderr: "fake failure".into(),
        }
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
        fn dump(&self, out: &Path) -> Result<(), PgError> {
            push(&self.log, "db.dump");
            std::fs::write(out, b"PGDMP-fake-db").map_err(|source| PgError::Io {
                path: out.to_path_buf(),
                source,
            })
        }
        fn stage_import(&self, file: &Path, migrate: Migrate<'_>) -> Result<(), PgError> {
            let body = std::fs::read(file).unwrap_or_default();
            push(
                &self.log,
                format!("db.stage({})", String::from_utf8_lossy(&body)),
            );
            migrate(&self.database_url("storm_codex_import"), Some(1)).map_err(PgError::Migrate)?;
            if self.fail_stage {
                return Err(fake_failure("pg_restore"));
            }
            self.staged.store(true, Ordering::SeqCst);
            Ok(())
        }
        /// Not logged: every start asks.
        fn staging_exists(&self) -> Result<bool, PgError> {
            Ok(self.staged.load(Ordering::SeqCst))
        }
        fn promote_import(&self) -> Result<(), PgError> {
            push(&self.log, "db.promote");
            if self.fail_promote {
                return Err(fake_failure("psql"));
            }
            self.staged.store(false, Ordering::SeqCst);
            if self.fail_promote_after_rename {
                return Err(fake_failure("dropdb"));
            }
            Ok(())
        }
        fn finish_import(&self) -> Result<(), PgError> {
            push(&self.log, "db.finish");
            Ok(())
        }
        fn discard_import(&self) -> Result<(), PgError> {
            push(&self.log, "db.discard");
            self.staged.store(false, Ordering::SeqCst);
            Ok(())
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

    pub struct FakeHost {
        log: Log,
        pub healthy: bool,
        pub honours_shutdown: bool,
        pub exits_at_once: bool,
        pub fail_stage: bool,
        pub fail_promote: bool,
        pub fail_promote_after_rename: bool,
        pub fail_migrate: Option<String>,
        pub staged: Arc<AtomicBool>,
        /// Shutdown flag of the most recently spawned child.
        shutdown_requested: Mutex<Arc<AtomicBool>>,
        pub last_cmd: Mutex<Option<ServerCommand>>,
    }

    impl FakeHost {
        pub fn new(log: &Log) -> Self {
            Self {
                log: log.clone(),
                healthy: true,
                honours_shutdown: true,
                exits_at_once: false,
                fail_stage: false,
                fail_promote: false,
                fail_promote_after_rename: false,
                fail_migrate: None,
                staged: Arc::new(AtomicBool::new(false)),
                shutdown_requested: Mutex::new(Arc::new(AtomicBool::new(false))),
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
                fail_stage: self.fail_stage,
                fail_promote: self.fail_promote,
                fail_promote_after_rename: self.fail_promote_after_rename,
                staged: self.staged.clone(),
            })
        }
        fn spawn_server(&self, cmd: &ServerCommand) -> io::Result<Box<dyn ServerChild>> {
            push(&self.log, "server.spawn");
            *self.last_cmd.lock().expect("cmd") = Some(cmd.clone());
            let flag = Arc::new(AtomicBool::new(false));
            *self.shutdown_requested.lock().expect("flag") = flag.clone();
            Ok(Box::new(FakeChild {
                log: self.log.clone(),
                shutdown_requested: flag,
                honours_shutdown: self.honours_shutdown,
                exits_at_once: self.exits_at_once,
                killed: false,
            }))
        }
        fn health_ok(&self, _port: u16) -> bool {
            self.healthy
        }
        fn request_shutdown(&self, port: u16, _io_timeout: Duration) -> io::Result<()> {
            push(&self.log, format!("server.shutdown({port})"));
            self.shutdown_requested
                .lock()
                .expect("flag")
                .store(true, Ordering::SeqCst);
            Ok(())
        }
        fn migrate(
            &self,
            _cmd: &ServerCommand,
            _database_url: &str,
            _target: Option<i64>,
        ) -> Result<(), String> {
            push(&self.log, "server.migrate");
            match &self.fail_migrate {
                Some(message) => Err(message.clone()),
                None => Ok(()),
            }
        }
    }

    pub fn start_with_timings(
        host: FakeHost,
        data: &Path,
        timings: Timings,
    ) -> (Result<Stack, StartError>, Arc<FakeHost>) {
        let host = Arc::new(host);
        let res = Stack::start_with(host.clone(), Path::new("C:/res"), data, timings);
        (res, host)
    }

    pub fn start(host: FakeHost, data: &Path) -> (Result<Stack, StartError>, Arc<FakeHost>) {
        start_with_timings(host, data, fast())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::fakes::*;
    use super::*;

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
    fn session_end_stop_never_waits_longer_than_its_budget() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.honours_shutdown = false; // the child never exits on its own
        let timings = Timings {
            stop_timeout: Duration::from_secs(30),
            session_end_stop_timeout: Duration::from_millis(100),
            ..fast()
        };
        let (stack, _host) = start_with_timings(host, tmp.path(), timings);
        let mut stack = stack.expect("start");
        log.lock().expect("log").clear();
        let began = std::time::Instant::now();
        stack.stop_with(StopBudget::SessionEnd).expect("stop");
        let took = began.elapsed();
        assert!(took >= Duration::from_millis(100), "{took:?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
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
        let status = http_status(
            port,
            "POST",
            "/api/standalone/shutdown",
            Duration::from_secs(5),
        )
        .expect("status");
        assert_eq!(status, 202);
        let request = server.join().expect("join");
        assert!(request.starts_with("POST /api/standalone/shutdown HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        // Nothing listens any more: an error, not a hang.
        assert!(http_status(port, "GET", "/api/health", Duration::from_secs(1)).is_err());
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
        assert_eq!(cmd.log, data.logs().join("server.log"));
    }

    /// Every name the shell may set: its own contract variables plus the allow-list.
    const CONTRACT: &[&str] = &[
        "STANDALONE",
        "BIND_ADDR",
        "DATABASE_URL",
        "ARCHIVE_DIR",
        "RAW_CACHE_DIR",
        "IMAGES_DIR",
        "LEDGER_FILE",
        "WEB_DIR",
        "MINIMAPS_BUNDLE_DIR",
        "REFERENTIAL_FILE",
    ];

    #[test]
    fn server_env_is_an_allow_list() {
        let data = DataDir::new(PathBuf::from("C:/data"));
        // The parent "has" every variable, hostile ones included.
        let env = server_env(
            Path::new("C:/res"),
            &data,
            8088,
            "postgres://storm:pw@127.0.0.1:1/storm_codex".into(),
            |name| Some(OsString::from(format!("parent-{name}"))),
        );
        for (k, v) in &env {
            assert!(
                CONTRACT.contains(k) || SERVER_ENV_PASSTHROUGH.contains(k),
                "unexpected {k}"
            );
            // The contract values are the shell's own, never the parent's.
            if CONTRACT.contains(k) {
                assert!(!v.to_string_lossy().starts_with("parent-"), "{k}");
            }
        }
        for leak in [
            "REDIS_URL",
            "ADMIN_TOKEN",
            "AZURE_PUSH_TOKEN",
            "JARVIS_CHANNEL",
        ] {
            assert!(env.iter().all(|(k, _)| *k != leak), "{leak} leaked");
        }
        assert_eq!(
            env.iter().filter(|(k, _)| *k == "DATABASE_URL").count(),
            1,
            "DATABASE_URL is set once, by the shell"
        );
        for name in SERVER_ENV_PASSTHROUGH {
            assert!(env.iter().any(|(k, _)| k == name), "{name} not passed");
        }
    }

    /// The real child sees exactly the allow-list: run `cmd /C set` with the command the
    /// server would get, while the test process holds secrets in its own environment.
    #[cfg(windows)]
    #[test]
    fn child_process_inherits_no_unlisted_variable() {
        use std::process::Stdio;
        // SAFETY (edition 2021: safe fn): test-only variables, no other test reads them.
        for k in [
            "REDIS_URL",
            "ADMIN_TOKEN",
            "AZURE_PUSH_TOKEN",
            "STORM_CODEX_LEAK_PROBE",
        ] {
            std::env::set_var(k, "leaked-secret");
        }
        let system_root = std::env::var_os("SystemRoot").expect("SystemRoot");
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = DataDir::new(tmp.path().to_path_buf());
        let cmd = ServerCommand {
            exe: PathBuf::from(&system_root).join("System32").join("cmd.exe"),
            env: server_env(
                Path::new("C:/res"),
                &data,
                8088,
                "postgres://storm:pw@127.0.0.1:1/storm_codex".into(),
                |name| std::env::var_os(name),
            ),
            cwd: tmp.path().to_path_buf(),
            log: tmp.path().join("unused.log"),
        };
        let out = server_command(&cmd)
            .args(["/D", "/C", "set"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("run cmd");
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("leaked-secret"), "{text}");
        // cmd.exe adds a few of its own.
        let cmd_own = ["COMSPEC", "PATHEXT", "PROMPT"];
        for line in text.lines().filter(|l| !l.is_empty()) {
            let name = line.split('=').next().unwrap_or_default().to_uppercase();
            let allowed = CONTRACT.iter().any(|k| k.eq_ignore_ascii_case(&name))
                || SERVER_ENV_PASSTHROUGH
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(&name))
                || cmd_own.contains(&name.as_str());
            assert!(allowed, "unlisted variable reached the child: {name}");
        }
    }
}
