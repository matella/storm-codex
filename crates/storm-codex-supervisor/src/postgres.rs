//! Lifecycle of the bundled Postgres 17 cluster: init, stale-lock cleanup, start/stop,
//! database creation, dump/restore. Every tool is run with `std::process::Command` (no shell)
//! from `bin_dir`; on Windows no console window is shown.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sysinfo::{Pid, ProcessesToUpdate, System};

/// Postgres major version the bundled binaries and the data directory must share.
pub const PG_MAJOR: &str = "17";
/// Database used by the Storm Codex server; `dump`/`restore` operate on it.
pub const APP_DATABASE: &str = "storm_codex";
/// Superuser created by `initdb` (trust auth, loopback only).
const PG_USER: &str = "storm";
/// Seconds `pg_ctl` waits for start/stop.
const PG_CTL_TIMEOUT_SECS: &str = "60";
/// Inherited libpq/server variables that could redirect or alter our commands.
const PG_ENV_VARS: &[&str] = &[
    "PGDATA",
    "PGHOST",
    "PGHOSTADDR",
    "PGPORT",
    "PGUSER",
    "PGDATABASE",
    "PGPASSWORD",
    "PGPASSFILE",
    "PGSERVICE",
    "PGSERVICEFILE",
    "PGOPTIONS",
    "PGSSLMODE",
    "PGTZ",
    "PGCLIENTENCODING",
];

#[derive(Debug, thiserror::Error)]
pub enum PgError {
    #[error("Postgres binary not found: {0}")]
    MissingBinary(PathBuf),
    #[error(
        "data directory {data_dir} belongs to Postgres {found}, but the bundled Postgres is {PG_MAJOR}"
    )]
    WrongMajorVersion { data_dir: PathBuf, found: String },
    #[error("`{command}` failed ({status}): {stderr}")]
    CommandFailed {
        command: String,
        status: String,
        stderr: String,
    },
    #[error("`{command}` timed out: {detail}")]
    Timeout { command: String, detail: String },
    #[error("invalid database name {0:?} (allowed: lowercase letters, digits, underscore)")]
    InvalidDatabaseName(String),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> PgError + '_ {
    move |source| PgError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A Postgres cluster in `data_dir`, run by the binaries in `bin_dir`, listening on
/// 127.0.0.1:`port` only.
#[derive(Debug, Clone)]
pub struct Postgres {
    bin_dir: PathBuf,
    data_dir: PathBuf,
    port: u16,
}

impl Postgres {
    pub fn new(bin_dir: PathBuf, data_dir: PathBuf, port: u16) -> Self {
        Self {
            bin_dir,
            data_dir,
            port,
        }
    }

    /// Runs `initdb` unless the data directory already holds a cluster. Refuses a cluster of
    /// another major version (checked before any binary is needed).
    pub fn init_if_needed(&self) -> Result<(), PgError> {
        if self.check_version()? {
            return Ok(());
        }
        let mut cmd = self.command("initdb")?;
        cmd.arg("-D").arg(&self.data_dir).args([
            "-U",
            PG_USER,
            "-A",
            "trust",
            "-E",
            "UTF8",
            "--locale=C",
        ]);
        run(&mut cmd, "initdb")?;
        // No Unix-domain socket: the server is reached over TCP loopback only.
        let conf = self.data_dir.join("postgresql.conf");
        let mut text = std::fs::read_to_string(&conf).map_err(io_err(&conf))?;
        text.push_str(
            "\n# Storm Codex standalone\nlisten_addresses = '127.0.0.1'\nunix_socket_directories = ''\n",
        );
        std::fs::write(&conf, text).map_err(io_err(&conf))
    }

    /// Removes `postmaster.pid` when the PID it names is not a running `postgres` process
    /// (crash, power loss, PID reused by another program). A live `postgres` is left alone.
    pub fn clean_stale(&self) -> Result<(), PgError> {
        let pid_file = self.pid_file();
        let text = match std::fs::read_to_string(&pid_file) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_err(&pid_file)(e)),
        };
        let live = text
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u32>().ok())
            .is_some_and(is_postgres_process);
        if live {
            return Ok(());
        }
        match std::fs::remove_file(&pid_file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(&pid_file)(e)),
            _ => Ok(()),
        }
    }

    /// Starts the server (`pg_ctl start -w`), logging to `<data root>/logs/postgres.log`.
    /// A server already running on this data directory is restarted so it uses `port`.
    pub fn start(&self) -> Result<(), PgError> {
        self.check_version()?;
        if self.is_running()? {
            self.stop()?;
        }
        let logs = self.logs_dir();
        std::fs::create_dir_all(&logs).map_err(io_err(&logs))?;
        // pg_ctl's own output goes to a file, not a pipe: the detached postmaster may inherit
        // the handles, and a pipe would then never reach EOF.
        let out_path = logs.join("pg_ctl.log");
        let out = File::create(&out_path).map_err(io_err(&out_path))?;
        let err = out.try_clone().map_err(io_err(&out_path))?;

        let mut cmd = self.command("pg_ctl")?;
        cmd.args(["start", "-w", "-t", PG_CTL_TIMEOUT_SECS, "-D"])
            .arg(&self.data_dir)
            .arg("-l")
            .arg(logs.join("postgres.log"))
            .arg("-o")
            .arg(format!("-p {} -c listen_addresses=127.0.0.1", self.port))
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        let status = cmd.status().map_err(io_err(&self.bin_dir))?;
        if status.success() {
            return Ok(());
        }
        let detail = std::fs::read_to_string(&out_path).unwrap_or_default();
        Err(pg_ctl_error("pg_ctl start", status.to_string(), detail))
    }

    /// Fast shutdown (`pg_ctl stop -m fast`). No-op when no server runs on this data directory.
    pub fn stop(&self) -> Result<(), PgError> {
        if !self.pid_file().exists() {
            return Ok(());
        }
        let mut cmd = self.command("pg_ctl")?;
        cmd.args(["stop", "-m", "fast", "-w", "-t", PG_CTL_TIMEOUT_SECS, "-D"])
            .arg(&self.data_dir);
        let out = output(&mut cmd)?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        // A stale pid file left by a crash: nothing is running, which is what stop wants.
        if !self.is_running()? {
            return Ok(());
        }
        Err(pg_ctl_error("pg_ctl stop", out.status.to_string(), stderr))
    }

    /// Creates database `name` if it does not exist.
    pub fn ensure_database(&self, name: &str) -> Result<(), PgError> {
        validate_db_name(name)?;
        let mut cmd = self.client("psql", "postgres")?;
        cmd.args(["-X", "-q", "-t", "-A", "-c"]).arg(format!(
            "SELECT 1 FROM pg_database WHERE datname = '{name}'"
        ));
        if run(&mut cmd, "psql")?.trim() == "1" {
            return Ok(());
        }
        let mut cmd = self.command("createdb")?;
        cmd.args(["-h", "127.0.0.1", "-p"])
            .arg(self.port.to_string())
            .args(["-U", PG_USER, name]);
        run(&mut cmd, "createdb").map(|_| ())
    }

    /// Connection URL for database `db` (loopback, trust auth).
    pub fn database_url(&self, db: &str) -> String {
        format!("postgres://{PG_USER}@127.0.0.1:{}/{db}", self.port)
    }

    /// Dumps [`APP_DATABASE`] in custom format (`pg_dump -Fc`) to `out`.
    pub fn dump(&self, out: &Path) -> Result<(), PgError> {
        let mut cmd = self.client("pg_dump", APP_DATABASE)?;
        cmd.arg("-Fc").arg("-f").arg(out);
        run(&mut cmd, "pg_dump").map(|_| ())
    }

    /// Restores a [`dump`](Self::dump) into [`APP_DATABASE`] (created if needed), replacing the
    /// objects it contains (`pg_restore --clean --if-exists`).
    pub fn restore(&self, file: &Path) -> Result<(), PgError> {
        self.ensure_database(APP_DATABASE)?;
        let mut cmd = self.client("pg_restore", APP_DATABASE)?;
        cmd.args(["--clean", "--if-exists", "--no-owner", "--exit-on-error"])
            .arg(file);
        run(&mut cmd, "pg_restore").map(|_| ())
    }

    fn pid_file(&self) -> PathBuf {
        self.data_dir.join("postmaster.pid")
    }

    /// `<data root>/logs`, the data root being the parent of the cluster directory
    /// (see `DataDir::pg` and `DataDir::logs`).
    fn logs_dir(&self) -> PathBuf {
        self.data_dir
            .parent()
            .unwrap_or(&self.data_dir)
            .join("logs")
    }

    /// `Ok(true)` when a cluster of the right major version exists, `Ok(false)` when there is
    /// none yet.
    fn check_version(&self) -> Result<bool, PgError> {
        let path = self.data_dir.join("PG_VERSION");
        match std::fs::read_to_string(&path) {
            Ok(v) if v.trim() == PG_MAJOR => Ok(true),
            Ok(v) => Err(PgError::WrongMajorVersion {
                data_dir: self.data_dir.clone(),
                found: v.trim().to_string(),
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_err(&path)(e)),
        }
    }

    /// `pg_ctl status`: exit 0 = running, 3 = not running, 4 = no usable data directory.
    fn is_running(&self) -> Result<bool, PgError> {
        if !self.pid_file().exists() {
            return Ok(false);
        }
        let mut cmd = self.command("pg_ctl")?;
        cmd.arg("status").arg("-D").arg(&self.data_dir);
        Ok(output(&mut cmd)?.status.success())
    }

    /// A command for `tool` from `bin_dir`, with inherited `PG*` variables removed and, on
    /// Windows, no console window.
    fn command(&self, tool: &str) -> Result<Command, PgError> {
        let exe = self
            .bin_dir
            .join(format!("{tool}{}", std::env::consts::EXE_SUFFIX));
        if !exe.is_file() {
            return Err(PgError::MissingBinary(exe));
        }
        let mut cmd = Command::new(exe);
        for var in PG_ENV_VARS {
            cmd.env_remove(var);
        }
        hide_console(&mut cmd);
        Ok(cmd)
    }

    /// A client tool connected to database `db` over loopback.
    fn client(&self, tool: &str, db: &str) -> Result<Command, PgError> {
        let mut cmd = self.command(tool)?;
        cmd.args(["-h", "127.0.0.1", "-p"])
            .arg(self.port.to_string())
            .args(["-U", PG_USER, "-d", db]);
        Ok(cmd)
    }
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

fn output(cmd: &mut Command) -> Result<Output, PgError> {
    let program = PathBuf::from(cmd.get_program());
    cmd.stdin(Stdio::null())
        .output()
        .map_err(|e| io_err(&program)(e))
}

/// Runs `cmd` to completion and returns its stdout, or `CommandFailed` with its stderr.
fn run(cmd: &mut Command, label: &str) -> Result<String, PgError> {
    let out = output(cmd)?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(PgError::CommandFailed {
        command: label.to_string(),
        status: out.status.to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// `pg_ctl` reports a `-t` expiry in its message ("did not start in time" / "does not shut down").
fn pg_ctl_error(command: &str, status: String, detail: String) -> PgError {
    let detail = detail.trim().to_string();
    if detail.contains("in time") || detail.contains("does not shut down") {
        PgError::Timeout {
            command: command.to_string(),
            detail,
        }
    } else {
        PgError::CommandFailed {
            command: command.to_string(),
            status,
            stderr: detail,
        }
    }
}

fn validate_db_name(name: &str) -> Result<(), PgError> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit();
    if ok {
        Ok(())
    } else {
        Err(PgError::InvalidDatabaseName(name.to_string()))
    }
}

/// True when `pid` is a running process named `postgres` (`postgres.exe` on Windows).
fn is_postgres_process(pid: u32) -> bool {
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).is_some_and(|p| {
        let name = p.name().to_string_lossy().to_ascii_lowercase();
        name == "postgres" || name == "postgres.exe"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_names_are_validated() {
        for ok in ["storm_codex", "a", "db_2"] {
            assert!(validate_db_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "Storm", "1db", "a-b", "a'b", "a b", &"x".repeat(64)] {
            assert!(validate_db_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn current_process_is_not_postgres() {
        assert!(!is_postgres_process(std::process::id()));
    }

    #[test]
    fn pg_ctl_timeout_is_classified() {
        assert!(matches!(
            pg_ctl_error(
                "pg_ctl start",
                "exit code: 1".into(),
                "pg_ctl: server did not start in time".into()
            ),
            PgError::Timeout { .. }
        ));
        assert!(matches!(
            pg_ctl_error(
                "pg_ctl start",
                "exit code: 1".into(),
                "pg_ctl: could not start server".into()
            ),
            PgError::CommandFailed { .. }
        ));
    }
}
