//! Lifecycle of the bundled Postgres 17 cluster: init, stale-lock cleanup, start/stop,
//! database creation, dump/restore. Every tool is run with `std::process::Command` (no shell)
//! from `bin_dir`; on Windows no console window is shown.
//!
//! Authentication: `initdb` sets `scram-sha-256` for every connection, with a random password
//! generated once and stored in `pg-password` next to (not inside) the cluster directory. The
//! password is a secret: it is passed to child tools through `PGPASSWORD`, never as an
//! argument, and never appears in errors or logs.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Postgres major version the bundled binaries and the data directory must share.
pub const PG_MAJOR: &str = "17";
/// Database used by the Storm Codex server; `dump`/`restore` operate on it.
pub const APP_DATABASE: &str = "storm_codex";
/// Superuser created by `initdb` (password auth, loopback only).
const PG_USER: &str = "storm";
/// Non-superuser role an imported (untrusted) dump is restored as: `NOSUPERUSER NOCREATEDB
/// NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT`, member of no role, owner of the staging
/// database only. It cannot run programs, read or write server files, load C code or change the
/// server configuration.
pub const IMPORT_ROLE: &str = "storm_import";
/// Database an import is restored into before it replaces [`APP_DATABASE`].
pub const STAGING_DATABASE: &str = "storm_codex_import";
/// Name the replaced app database has between the swap and its removal.
pub const PREVIOUS_DATABASE: &str = "storm_codex_previous";

/// Archive entry kinds (`pg_restore -l`) an imported dump may hold: the app schema has tables,
/// sequences, indexes, constraints, defaults and data only. Anything else (functions,
/// procedures, triggers, rules, extensions, languages, casts, operators, foreign data, event
/// triggers, publications, ACLs, ...) is refused before anything is restored.
const ALLOWED_TOC_KINDS: &[&str] = &[
    "ENCODING",
    "STDSTRINGS",
    "SEARCHPATH",
    "TABLE",
    "TABLE DATA",
    "SEQUENCE",
    "SEQUENCE OWNED BY",
    "SEQUENCE SET",
    "DEFAULT",
    "INDEX",
    "CONSTRAINT",
    "FK CONSTRAINT",
    "COMMENT",
];

/// First OID of objects created after `initdb` (`FirstNormalObjectId`).
const FIRST_USER_OID: u32 = 16384;

/// Built-in functions the app schema's expressions (column defaults, check constraints,
/// index expressions, views) may use. Any other function referenced by a restored object is
/// refused: the server connects as the superuser, and a default such as
/// `pg_read_file(...)` would run with its rights on the next insert.
const ALLOWED_EXPRESSION_FUNCTIONS: &[&str] = &[
    "now()",
    "nextval(regclass)",
    "lower(text)",
    "upper(text)",
    "btrim(text)",
];
/// Name of the password file, a sibling of the cluster directory.
const PASSWORD_FILE: &str = "pg-password";
/// Random bytes in the generated password (hex-encoded: 64 characters).
const PASSWORD_BYTES: usize = 32;
/// Seconds `pg_ctl` waits for start/stop.
const PG_CTL_TIMEOUT_SECS: u64 = 60;
/// Ceiling for `pg_ctl` itself (its own `-t` plus margin).
const PG_CTL_LIMIT: Duration = Duration::from_secs(PG_CTL_TIMEOUT_SECS + 30);
/// Ceiling for quick client commands (psql, createdb).
const CLIENT_LIMIT: Duration = Duration::from_secs(60);
/// Ceiling for `pg_dump` / `pg_restore`.
const DUMP_LIMIT: Duration = Duration::from_secs(30 * 60);
/// libpq connection timeout given to every client tool (seconds).
const CONNECT_TIMEOUT_SECS: &str = "10";
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
    "PGCONNECT_TIMEOUT",
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
    /// The password file exists but does not hold a hex password (its content is not shown).
    #[error("invalid Postgres password file {0}")]
    InvalidPasswordFile(PathBuf),
    #[error("could not generate a random password: {0}")]
    Random(String),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// An imported dump holds objects the app never creates (refused, nothing changed).
    #[error("the backup's database holds objects Storm Codex never creates: {0}")]
    UnsafeDump(String),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> PgError + '_ {
    move |source| PgError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// What `postmaster.pid` says about the cluster.
#[derive(Debug, PartialEq, Eq)]
enum PidFile {
    Absent,
    /// Names a live postmaster running our binary on our data directory.
    Ours,
    /// Dead PID, unreadable content, or a PID reused by another process (including another
    /// Postgres): never signalled, safe to delete.
    Stale,
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
    /// another major version (checked before any binary is needed). On first init a random
    /// password is generated (or an existing password file reused) and set for `storm`.
    pub fn init_if_needed(&self) -> Result<(), PgError> {
        if self.check_version()? {
            return Ok(());
        }
        let initdb = self.command("initdb")?;
        let pw_path = self.ensure_password_file()?;
        let mut cmd = initdb;
        cmd.arg("-D")
            .arg(&self.data_dir)
            .args([
                "-U",
                PG_USER,
                "-A",
                "scram-sha-256",
                "-E",
                "UTF8",
                "--locale=C",
            ])
            .arg(format!("--pwfile={}", pw_path.display()));
        run(&mut cmd, "initdb", CLIENT_LIMIT)?;
        // No Unix-domain socket: the server is reached over TCP loopback only.
        let conf = self.data_dir.join("postgresql.conf");
        let mut text = std::fs::read_to_string(&conf).map_err(io_err(&conf))?;
        text.push_str(
            "\n# Storm Codex standalone\nlisten_addresses = '127.0.0.1'\nunix_socket_directories = ''\n",
        );
        std::fs::write(&conf, text).map_err(io_err(&conf))
    }

    /// Removes `postmaster.pid` unless it names a live postmaster of this cluster (our
    /// `postgres` binary, our data directory). Dead PIDs and PIDs reused by any other process,
    /// including another Postgres, are stale.
    pub fn clean_stale(&self) -> Result<(), PgError> {
        if self.pid_file_state()? == PidFile::Stale {
            self.remove_pid_file()?;
        }
        Ok(())
    }

    /// Starts the server (`pg_ctl start -w`), logging to `<data root>/logs/postgres.log`.
    /// A stale pid file is removed first; our own server, if already running, is restarted so
    /// it uses `port`.
    pub fn start(&self) -> Result<(), PgError> {
        self.check_version()?;
        match self.pid_file_state()? {
            PidFile::Absent => {}
            PidFile::Stale => self.remove_pid_file()?,
            PidFile::Ours => self.stop()?,
        }
        let mut cmd = self.command("pg_ctl")?;
        let logs = self.logs_dir();
        std::fs::create_dir_all(&logs).map_err(io_err(&logs))?;
        // pg_ctl's own output goes to a file, not a pipe: the detached postmaster may inherit
        // the handles, and a pipe would then never reach EOF.
        let out_path = logs.join("pg_ctl.log");
        let out = File::create(&out_path).map_err(io_err(&out_path))?;
        let err = out.try_clone().map_err(io_err(&out_path))?;

        cmd.args(["start", "-w", "-t", &PG_CTL_TIMEOUT_SECS.to_string(), "-D"])
            .arg(&self.data_dir)
            .arg("-l")
            .arg(logs.join("postgres.log"))
            .arg("-o")
            .arg(format!("-p {} -c listen_addresses=127.0.0.1", self.port))
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        let started = Instant::now();
        let child = cmd.spawn().map_err(io_err(&self.bin_dir))?;
        let status = wait_limited(child, "pg_ctl start", PG_CTL_LIMIT)?;
        if status.success() {
            return Ok(());
        }
        let detail = std::fs::read_to_string(&out_path).unwrap_or_default();
        Err(pg_ctl_error(
            "pg_ctl start",
            status.to_string(),
            detail,
            started.elapsed(),
        ))
    }

    /// Fast shutdown (`pg_ctl stop -m fast`). No-op when no server of this cluster runs; a
    /// stale pid file (including one naming a foreign process) is removed, never signalled.
    pub fn stop(&self) -> Result<(), PgError> {
        match self.pid_file_state()? {
            PidFile::Absent => return Ok(()),
            PidFile::Stale => return self.remove_pid_file(),
            PidFile::Ours => {}
        }
        let mut cmd = self.command("pg_ctl")?;
        cmd.args([
            "stop",
            "-m",
            "fast",
            "-w",
            "-t",
            &PG_CTL_TIMEOUT_SECS.to_string(),
            "-D",
        ])
        .arg(&self.data_dir);
        let started = Instant::now();
        let out = output_limited(&mut cmd, "pg_ctl stop", PG_CTL_LIMIT)?;
        if out.status.success() {
            return Ok(());
        }
        // It died on its own meanwhile: nothing runs, which is what stop wants.
        if self.pid_file_state()? != PidFile::Ours {
            return self.remove_pid_file();
        }
        Err(pg_ctl_error(
            "pg_ctl stop",
            out.status.to_string(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            started.elapsed(),
        ))
    }

    /// Creates database `name` if it does not exist.
    pub fn ensure_database(&self, name: &str) -> Result<(), PgError> {
        validate_db_name(name)?;
        let mut cmd = self.client("psql")?;
        cmd.args(["-d", "postgres", "-X", "-q", "-t", "-A", "-c"])
            .arg(format!(
                "SELECT 1 FROM pg_database WHERE datname = '{name}'"
            ));
        if run(&mut cmd, "psql", CLIENT_LIMIT)?.trim() == "1" {
            return Ok(());
        }
        let mut cmd = self.client("createdb")?;
        cmd.arg(name);
        run(&mut cmd, "createdb", CLIENT_LIMIT).map(|_| ())
    }

    /// Connection URL for database `db` on loopback, with the generated password. The URL is a
    /// secret: do not log it. Without a readable password file (cluster not initialised yet)
    /// the URL has no password and connecting fails with an authentication error.
    pub fn database_url(&self, db: &str) -> String {
        match self.password() {
            Ok(pw) => format!("postgres://{PG_USER}:{pw}@127.0.0.1:{}/{db}", self.port),
            Err(_) => format!("postgres://{PG_USER}@127.0.0.1:{}/{db}", self.port),
        }
    }

    /// Dumps [`APP_DATABASE`] in custom format (`pg_dump -Fc`) to `out`.
    pub fn dump(&self, out: &Path) -> Result<(), PgError> {
        let mut cmd = self.client("pg_dump")?;
        cmd.args(["-d", APP_DATABASE, "-Fc", "-f"]).arg(out);
        run(&mut cmd, "pg_dump", DUMP_LIMIT).map(|_| ())
    }

    /// Restores a [`dump`](Self::dump) into [`APP_DATABASE`] (created if needed), replacing the
    /// objects it contains (`pg_restore --clean --if-exists`). Lock waits are capped at 30 s.
    /// One transaction (`--single-transaction`): a failed restore leaves the database as it was.
    pub fn restore(&self, file: &Path) -> Result<(), PgError> {
        self.ensure_database(APP_DATABASE)?;
        let mut cmd = self.client("pg_restore")?;
        cmd.env("PGOPTIONS", "-c lock_timeout=30s")
            .args([
                "-d",
                APP_DATABASE,
                "--clean",
                "--if-exists",
                "--no-owner",
                "--exit-on-error",
                "--single-transaction",
            ])
            .arg(file);
        run(&mut cmd, "pg_restore", DUMP_LIMIT).map(|_| ())
    }

    /// Restores an **untrusted** dump into [`STAGING_DATABASE`] without touching
    /// [`APP_DATABASE`] (the server may keep running):
    /// 1. `pg_restore -l`: refused unless every entry kind is in the allow-list;
    /// 2. a fresh staging database owned by [`IMPORT_ROLE`] (non-superuser);
    /// 3. `pg_restore` connected **as that role** (`--no-owner --no-acl`, one transaction,
    ///    `-L` with the checked entries only);
    /// 4. catalog check of the result (no function, trigger, rule, extension, policy,
    ///    operator, cast, language or foreign table of its own; expressions may only call the
    ///    allow-listed built-ins).
    ///
    /// On any failure the staging database is dropped. [`promote_import`](Self::promote_import)
    /// then swaps it in.
    pub fn stage_import(&self, file: &Path) -> Result<(), PgError> {
        let mut list_cmd = self.command("pg_restore")?;
        list_cmd.arg("-l").arg(file);
        let list = run(&mut list_cmd, "pg_restore -l", CLIENT_LIMIT)?;
        let refused = toc_refusals(&list);
        if !refused.is_empty() {
            return Err(PgError::UnsafeDump(refused.join(", ")));
        }
        let list_file = file.with_extension("toc");
        std::fs::write(&list_file, &list).map_err(io_err(&list_file))?;
        self.discard_import()?;
        let result = self.stage_checked(file, &list_file);
        let _ = std::fs::remove_file(&list_file);
        if result.is_err() {
            let _ = self.discard_import();
        }
        result
    }

    fn stage_checked(&self, file: &Path, list_file: &Path) -> Result<(), PgError> {
        self.ensure_import_role()?;
        self.psql_postgres(&format!(
            "CREATE DATABASE {STAGING_DATABASE} OWNER {IMPORT_ROLE}"
        ))?;
        self.psql_postgres(&format!(
            "REVOKE ALL ON DATABASE {STAGING_DATABASE} FROM PUBLIC"
        ))?;
        let mut cmd = self.client_as("pg_restore", IMPORT_ROLE)?;
        cmd.args([
            "-d",
            STAGING_DATABASE,
            "--no-owner",
            "--no-acl",
            "--exit-on-error",
            "--single-transaction",
            "-L",
        ])
        .arg(list_file)
        .arg(file);
        run(&mut cmd, "pg_restore", DUMP_LIMIT)?;
        self.check_staged()
    }

    /// Catalog check of the staging database (see [`stage_import`](Self::stage_import)).
    pub fn check_staged(&self) -> Result<(), PgError> {
        let unsafe_objects = self.unsafe_objects(STAGING_DATABASE)?;
        if unsafe_objects.is_empty() {
            Ok(())
        } else {
            Err(PgError::UnsafeDump(unsafe_objects.join(", ")))
        }
    }

    /// True when a staged import exists (restored, not swapped in yet).
    pub fn staging_exists(&self) -> Result<bool, PgError> {
        let mut cmd = self.client("psql")?;
        cmd.args(["-d", "postgres", "-X", "-q", "-t", "-A", "-c"])
            .arg(format!(
                "SELECT 1 FROM pg_database WHERE datname = '{STAGING_DATABASE}'"
            ));
        Ok(run(&mut cmd, "psql", CLIENT_LIMIT)?.trim() == "1")
    }

    /// Drops the staging database, if any.
    pub fn discard_import(&self) -> Result<(), PgError> {
        self.psql_postgres(&format!(
            "DROP DATABASE IF EXISTS {STAGING_DATABASE} WITH (FORCE)"
        ))
    }

    /// Swaps the staged database in, atomically (both renames in one transaction): afterwards
    /// either the staging database still exists (nothing changed) or it is the app database.
    /// Nothing may be connected to [`APP_DATABASE`] (stop the server first). Then
    /// [`finish_import`](Self::finish_import).
    pub fn promote_import(&self) -> Result<(), PgError> {
        // Settings the dump may have given the database or the import role never reach the
        // app: reset, then the catalog check runs again right before the swap.
        self.psql_postgres(&format!("ALTER DATABASE {STAGING_DATABASE} RESET ALL"))?;
        self.psql_postgres(&format!("ALTER ROLE {IMPORT_ROLE} RESET ALL"))?;
        self.check_staged()?;
        self.psql_postgres(&format!(
            "DROP DATABASE IF EXISTS {PREVIOUS_DATABASE} WITH (FORCE)"
        ))?;
        // One `-c` string with several statements runs as one transaction.
        self.psql_postgres(&format!(
            "ALTER DATABASE {APP_DATABASE} RENAME TO {PREVIOUS_DATABASE}; \
             ALTER DATABASE {STAGING_DATABASE} RENAME TO {APP_DATABASE}"
        ))?;
        self.finish_import()
    }

    /// After a promote (idempotent, also run by crash recovery): every object of the new app
    /// database goes back to the superuser that owns a fresh install, and the replaced
    /// database is dropped.
    pub fn finish_import(&self) -> Result<(), PgError> {
        let mut cmd = self.client("psql")?;
        cmd.args([
            "-d",
            APP_DATABASE,
            "-X",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
        ])
        .arg(format!(
            "DO $$ BEGIN IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{IMPORT_ROLE}') \
                 THEN EXECUTE 'REASSIGN OWNED BY {IMPORT_ROLE} TO {PG_USER}'; END IF; END $$"
        ));
        run(&mut cmd, "psql", CLIENT_LIMIT)?;
        self.psql_postgres(&format!(
            "DROP DATABASE IF EXISTS {PREVIOUS_DATABASE} WITH (FORCE)"
        ))
    }

    /// Connection URL as [`IMPORT_ROLE`] (same generated password). Secret: do not log it.
    pub fn import_role_url(&self, db: &str) -> String {
        match self.password() {
            Ok(pw) => format!("postgres://{IMPORT_ROLE}:{pw}@127.0.0.1:{}/{db}", self.port),
            Err(_) => format!("postgres://{IMPORT_ROLE}@127.0.0.1:{}/{db}", self.port),
        }
    }

    /// Creates or resets [`IMPORT_ROLE`]. The SQL (it holds the password) goes through a
    /// private file next to the cluster, never the command line.
    fn ensure_import_role(&self) -> Result<(), PgError> {
        let pw = self.password()?;
        let attrs = "LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT";
        let sql = format!(
            "SELECT format('CREATE ROLE {IMPORT_ROLE}') \
             WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{IMPORT_ROLE}') \\gexec\n\
             ALTER ROLE {IMPORT_ROLE} WITH {attrs} PASSWORD '{pw}';\n\
             ALTER ROLE {IMPORT_ROLE} RESET ALL;\n"
        );
        let path = self.password_file().with_file_name("import-role.sql");
        let _ = std::fs::remove_file(&path);
        write_private(&path, &sql).map_err(io_err(&path))?;
        let mut cmd = self.client("psql")?;
        cmd.args(["-d", "postgres", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-f"])
            .arg(&path);
        let result = run(&mut cmd, "psql", CLIENT_LIMIT).map(|_| ());
        let _ = std::fs::remove_file(&path);
        result
    }

    /// Objects of `db` the app never creates, described for the error message (read from the
    /// catalogs only: no user code runs).
    fn unsafe_objects(&self, db: &str) -> Result<Vec<String>, PgError> {
        let allowed = ALLOWED_EXPRESSION_FUNCTIONS
            .iter()
            .map(|f| format!("'{f}'"))
            .collect::<Vec<_>>()
            .join(", ");
        // Objects created after initdb only (`oid >= FirstNormalObjectId`): the catalogs also
        // list the system's own. Functions called by expressions are read from the stored
        // expression trees (`:funcid <oid>`): built-in functions are pinned, so `pg_depend`
        // records no dependency on them. Operators are not listed: user operators are refused
        // above and the built-in ones only call built-in comparison/arithmetic functions.
        // Every relation, function and type is schema-qualified, and the session runs with
        // `search_path=pg_catalog,pg_temp` forced by the client (see `client_as`): settings the
        // dump gave the database (`ALTER DATABASE ... SET search_path`) cannot shadow the
        // catalogs. Those settings, and any schema besides `public`, are refused too.
        let sql = format!(
            "SELECT 'function ' || p.oid::pg_catalog.regprocedure::pg_catalog.text \
               FROM pg_catalog.pg_proc p WHERE p.oid >= {FIRST_USER_OID} \
             UNION ALL SELECT 'schema ' || n.nspname::pg_catalog.text FROM pg_catalog.pg_namespace n \
               WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'public', 'pg_toast') \
               AND n.nspname::pg_catalog.text !~ '^pg_(toast_)?temp_' \
             UNION ALL SELECT 'setting ' || pg_catalog.array_to_string(s.setconfig, ',') \
               FROM pg_catalog.pg_db_role_setting s \
               WHERE s.setdatabase = (SELECT d.oid FROM pg_catalog.pg_database d \
                                      WHERE d.datname = pg_catalog.current_database()) \
               OR s.setrole = (SELECT r.oid FROM pg_catalog.pg_roles r \
                               WHERE r.rolname = '{IMPORT_ROLE}') \
             UNION ALL SELECT 'trigger ' || t.tgname::pg_catalog.text FROM pg_catalog.pg_trigger t \
               WHERE NOT t.tgisinternal \
             UNION ALL SELECT 'rule ' || r.rulename::pg_catalog.text FROM pg_catalog.pg_rewrite r \
               WHERE r.oid >= {FIRST_USER_OID} AND r.rulename <> '_RETURN' \
             UNION ALL SELECT 'extension ' || x.extname::pg_catalog.text \
               FROM pg_catalog.pg_extension x WHERE x.oid >= {FIRST_USER_OID} \
             UNION ALL SELECT 'policy ' || y.polname::pg_catalog.text FROM pg_catalog.pg_policy y \
             UNION ALL SELECT 'operator ' || o.oid::pg_catalog.regoperator::pg_catalog.text \
               FROM pg_catalog.pg_operator o WHERE o.oid >= {FIRST_USER_OID} \
             UNION ALL SELECT 'cast ' || c.oid::pg_catalog.text FROM pg_catalog.pg_cast c \
               WHERE c.oid >= {FIRST_USER_OID} \
             UNION ALL SELECT 'language ' || l.lanname::pg_catalog.text \
               FROM pg_catalog.pg_language l WHERE l.oid >= {FIRST_USER_OID} \
             UNION ALL SELECT 'foreign table ' || ft.ftrelid::pg_catalog.regclass::pg_catalog.text \
               FROM pg_catalog.pg_foreign_table ft \
             UNION ALL SELECT 'event trigger ' || et.evtname::pg_catalog.text \
               FROM pg_catalog.pg_event_trigger et \
             UNION ALL SELECT 'publication ' || pb.pubname::pg_catalog.text \
               FROM pg_catalog.pg_publication pb \
             UNION ALL SELECT DISTINCT 'use of function ' || \
               used.f::pg_catalog.regprocedure::pg_catalog.text FROM ( \
               SELECT (pg_catalog.regexp_matches(e, ':(?:funcid|aggfnoid|winfnoid) (\\d+)', 'g'))[1]::pg_catalog.oid AS f \
               FROM ( \
                 SELECT ad.adbin::pg_catalog.text FROM pg_catalog.pg_attrdef ad \
                   WHERE ad.oid >= {FIRST_USER_OID} \
                 UNION ALL SELECT co.conbin::pg_catalog.text FROM pg_catalog.pg_constraint co \
                   WHERE co.oid >= {FIRST_USER_OID} AND co.conbin IS NOT NULL \
                 UNION ALL SELECT i.indexprs::pg_catalog.text FROM pg_catalog.pg_index i \
                   WHERE i.indexrelid >= {FIRST_USER_OID} AND i.indexprs IS NOT NULL \
                 UNION ALL SELECT i.indpred::pg_catalog.text FROM pg_catalog.pg_index i \
                   WHERE i.indexrelid >= {FIRST_USER_OID} AND i.indpred IS NOT NULL \
                 UNION ALL SELECT rw.ev_action::pg_catalog.text FROM pg_catalog.pg_rewrite rw \
                   WHERE rw.oid >= {FIRST_USER_OID} \
                 UNION ALL SELECT se.stxexprs::pg_catalog.text FROM pg_catalog.pg_statistic_ext se \
                   WHERE se.oid >= {FIRST_USER_OID} AND se.stxexprs IS NOT NULL \
                 UNION ALL SELECT cl.relpartbound::pg_catalog.text FROM pg_catalog.pg_class cl \
                   WHERE cl.oid >= {FIRST_USER_OID} AND cl.relpartbound IS NOT NULL \
               ) AS exprs(e) \
             ) AS used \
             WHERE used.f::pg_catalog.regprocedure::pg_catalog.text NOT IN ({allowed})"
        );
        let mut cmd = self.client("psql")?;
        cmd.args([
            "-d",
            db,
            "-X",
            "-q",
            "-t",
            "-A",
            "-v",
            "ON_ERROR_STOP=1",
            "-c",
        ])
        .arg(sql);
        let out = run(&mut cmd, "psql", CLIENT_LIMIT)?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// One statement on the `postgres` maintenance database, as the superuser.
    fn psql_postgres(&self, sql: &str) -> Result<(), PgError> {
        let mut cmd = self.client("psql")?;
        cmd.args(["-d", "postgres", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-c"])
            .arg(sql);
        run(&mut cmd, "psql", CLIENT_LIMIT).map(|_| ())
    }

    fn pid_file(&self) -> PathBuf {
        self.data_dir.join("postmaster.pid")
    }

    fn password_file(&self) -> PathBuf {
        self.data_dir.with_file_name(PASSWORD_FILE)
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

    /// Reads the password file (hex, trimmed).
    fn password(&self) -> Result<String, PgError> {
        let path = self.password_file();
        let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        let pw = text.trim();
        if pw.is_empty() || !pw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PgError::InvalidPasswordFile(path));
        }
        Ok(pw.to_string())
    }

    /// Returns the password file, creating it with a fresh random password when absent.
    fn ensure_password_file(&self) -> Result<PathBuf, PgError> {
        let path = self.password_file();
        if path.exists() {
            self.password()?;
            return Ok(path);
        }
        let mut bytes = [0u8; PASSWORD_BYTES];
        getrandom::fill(&mut bytes).map_err(|e| PgError::Random(e.to_string()))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        write_private(&path, &hex).map_err(io_err(&path))?;
        Ok(path)
    }

    fn pid_file_state(&self) -> Result<PidFile, PgError> {
        let pid_file = self.pid_file();
        // Bytes, not a String: line 2 holds the data directory in the ANSI code page on
        // Windows, which is not UTF-8 for non-ASCII paths. Only the first line (ASCII PID) is used.
        let bytes = match std::fs::read(&pid_file) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PidFile::Absent),
            Err(e) => return Err(io_err(&pid_file)(e)),
        };
        let ours = parse_pid(&bytes).is_some_and(|pid| self.is_our_postmaster(pid));
        Ok(if ours { PidFile::Ours } else { PidFile::Stale })
    }

    fn remove_pid_file(&self) -> Result<(), PgError> {
        let pid_file = self.pid_file();
        match std::fs::remove_file(&pid_file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(&pid_file)(e)),
            _ => Ok(()),
        }
    }

    /// True when `pid` is alive, runs `<bin_dir>/postgres[.exe]`, and was started with
    /// `-D <our data_dir>` (how `pg_ctl` launches the postmaster; backends have no `-D`).
    fn is_our_postmaster(&self, pid: u32) -> bool {
        let pid = Pid::from_u32(pid);
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
        let Some(process) = sys.process(pid) else {
            return false;
        };
        let expected_exe = self
            .bin_dir
            .join(format!("postgres{}", std::env::consts::EXE_SUFFIX));
        let exe_ok = process
            .exe()
            .is_some_and(|exe| same_path(exe, &expected_exe));
        let dir_ok = process
            .cmd()
            .windows(2)
            .any(|w| w[0] == "-D" && same_path(Path::new(&w[1]), &self.data_dir));
        exe_ok && dir_ok
    }

    /// A command for `tool` from `bin_dir`: inherited `PG*` variables removed, English
    /// messages, and on Windows no console window.
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
        cmd.env("LC_MESSAGES", "C");
        hide_console(&mut cmd);
        Ok(cmd)
    }

    /// A client tool for the server on loopback: never prompts (`-w`), password through
    /// `PGPASSWORD`, bounded connection time.
    fn client(&self, tool: &str) -> Result<Command, PgError> {
        self.client_as(tool, PG_USER)
    }

    /// [`client`](Self::client) connected as `role` (same generated password). The session's
    /// `search_path` is forced to `pg_catalog,pg_temp` (`PGOPTIONS`: a client startup option
    /// overrides `ALTER DATABASE/ROLE ... SET`), so an imported database's settings cannot
    /// make a superuser session resolve names to its objects.
    fn client_as(&self, tool: &str, role: &str) -> Result<Command, PgError> {
        let mut cmd = self.command(tool)?;
        cmd.env("PGOPTIONS", "-c search_path=pg_catalog,pg_temp")
            .env("PGPASSWORD", self.password()?)
            .env("PGCONNECT_TIMEOUT", CONNECT_TIMEOUT_SECS)
            .args(["-w", "-h", "127.0.0.1", "-p"])
            .arg(self.port.to_string())
            .args(["-U", role]);
        Ok(cmd)
    }
}

/// Writes `contents` to a new file readable by the current user only (Unix mode 0600; on
/// Windows the per-user data folder's ACL applies).
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()
}

/// Compares two paths for identity: canonical forms when both resolve, otherwise a
/// normalised string comparison (`\` → `/`, no trailing slash, case-insensitive on Windows).
fn same_path(a: &Path, b: &Path) -> bool {
    if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return normalise(&ca) == normalise(&cb);
    }
    normalise(&std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf()))
        == normalise(&std::path::absolute(b).unwrap_or_else(|_| b.to_path_buf()))
}

fn normalise(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_string()
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

/// Waits for `child`, killing it and returning `Timeout` after `limit`.
fn wait_limited(mut child: Child, label: &str, limit: Duration) -> Result<ExitStatus, PgError> {
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(e) => return Err(io_err(Path::new(label))(e)),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PgError::Timeout {
                command: label.to_string(),
                detail: format!("no result after {} s, process killed", limit.as_secs()),
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Runs `cmd` with captured output and a wall-clock `limit`.
fn output_limited(cmd: &mut Command, label: &str, limit: Duration) -> Result<Output, PgError> {
    let program = PathBuf::from(cmd.get_program());
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_err(&program))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let status = wait_limited(child, label, limit)?;
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Runs `cmd` to completion and returns its stdout, or `CommandFailed` with its stderr.
fn run(cmd: &mut Command, label: &str, limit: Duration) -> Result<String, PgError> {
    let out = output_limited(cmd, label, limit)?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(PgError::CommandFailed {
        command: label.to_string(),
        status: out.status.to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// A failed `pg_ctl` is a `Timeout` when it used up its `-t` wait or says so ("did not start
/// in time" / "does not shut down"; messages are forced to English via `LC_MESSAGES=C`).
fn pg_ctl_error(command: &str, status: String, detail: String, elapsed: Duration) -> PgError {
    let detail = detail.trim().to_string();
    let timed_out = elapsed >= Duration::from_secs(PG_CTL_TIMEOUT_SECS)
        || detail.contains("in time")
        || detail.contains("does not shut down");
    if timed_out {
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

/// Entry kinds of a `pg_restore -l` listing that are not in [`ALLOWED_TOC_KINDS`] (each as
/// `KIND name`). Comment lines (`;`) are ignored; an entry line is
/// `<id>; <catalog oid> <object oid> <KIND> <schema> <name> <owner>`.
fn toc_refusals(list: &str) -> Vec<String> {
    let mut kinds: Vec<&str> = ALLOWED_TOC_KINDS.to_vec();
    // Longest first: "TABLE DATA" before "TABLE".
    kinds.sort_by_key(|k| std::cmp::Reverse(k.len()));
    let mut refused = Vec::new();
    for line in list.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let rest = line
            .split_once(';')
            .map(|(_, r)| r.trim_start())
            .and_then(|r| r.split_once(' ').map(|(_, r)| r))
            .and_then(|r| r.split_once(' ').map(|(_, r)| r));
        let ok = rest.is_some_and(|rest| {
            kinds.iter().any(|k| {
                rest.strip_prefix(k)
                    .is_some_and(|after| after.starts_with(' '))
            })
        });
        if !ok {
            refused.push(rest.unwrap_or(line).to_string());
        }
    }
    refused
}

/// PID on the first line of `postmaster.pid`.
fn parse_pid(bytes: &[u8]) -> Option<u32> {
    let first = bytes.split(|&b| b == b'\n').next()?;
    std::str::from_utf8(first).ok()?.trim().parse().ok()
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn toc_listing_allows_only_the_app_schema_kinds() {
        let ok = "; Archive created at 2026-11-14\n\
                  ;     dbname: storm_codex\n\
                  3; 0 0 ENCODING - ENCODING\n\
                  216; 1259 16390 TABLE public matches storm\n\
                  215; 1259 16389 SEQUENCE public matches_id_seq storm\n\
                  4790; 0 0 SEQUENCE OWNED BY public matches_id_seq storm\n\
                  4627; 2604 16393 DEFAULT public matches id storm\n\
                  4950; 0 16390 TABLE DATA public matches storm\n\
                  4951; 0 0 SEQUENCE SET public matches_id_seq storm\n\
                  4637; 2606 16400 CONSTRAINT public matches matches_pkey storm\n\
                  4640; 1259 16410 INDEX public matches_played_at storm\n\
                  4660; 2606 16420 FK CONSTRAINT public match_players fk storm\n";
        assert!(toc_refusals(ok).is_empty(), "{:?}", toc_refusals(ok));
        let bad = "217; 1255 16500 FUNCTION public evil() storm\n\
                   218; 2620 16501 TRIGGER public matches t storm\n\
                   219; 3079 16502 EXTENSION - dblink \n\
                   220; 0 0 ACL - SCHEMA public storm\n\
                   221; 1259 16503 TABLEX public x storm\n\
                   garbage line\n";
        let refused = toc_refusals(bad);
        assert_eq!(refused.len(), 6, "{refused:?}");
        assert!(refused[0].starts_with("FUNCTION public evil()"));
        assert!(refused.iter().any(|r| r.starts_with("EXTENSION")));
    }

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
    fn current_process_is_not_our_postmaster() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pg = Postgres::new(tmp.path().join("bin"), tmp.path().join("pg"), 5432);
        assert!(!pg.is_our_postmaster(std::process::id()));
    }

    #[test]
    fn pid_is_parsed_from_a_non_utf8_pid_file() {
        // Line 2 is `C:\Zoë` in Windows-1252 (0xEB), as Postgres writes it on Windows.
        let bytes = b"4242\r\nC:\\Zo\xeb\r\n1700000000\r\n";
        assert_eq!(parse_pid(bytes), Some(4242));
        assert_eq!(parse_pid(b"garbage\n"), None);
        assert_eq!(parse_pid(b""), None);
    }

    #[test]
    fn same_path_ignores_separators_and_trailing_slash() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = tmp.path().join("pg");
        std::fs::create_dir_all(&a).expect("mkdir");
        let forward = PathBuf::from(format!("{}/", a.to_string_lossy().replace('\\', "/")));
        assert!(same_path(&a, &forward));
        assert!(!same_path(&a, &tmp.path().join("other")));
    }

    #[test]
    fn password_file_is_created_once_next_to_the_cluster() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pg = Postgres::new(tmp.path().join("bin"), tmp.path().join("pg"), 5432);
        let path = pg.ensure_password_file().expect("create");
        assert_eq!(path, tmp.path().join(PASSWORD_FILE));
        let first = pg.password().expect("read");
        assert_eq!(first.len(), PASSWORD_BYTES * 2);
        pg.ensure_password_file().expect("reuse");
        assert_eq!(pg.password().expect("reread"), first);
        assert!(pg.database_url("db").contains(&format!(":{first}@")));
    }

    #[test]
    fn pg_ctl_timeout_is_classified() {
        let quick = Duration::from_secs(1);
        assert!(matches!(
            pg_ctl_error(
                "pg_ctl start",
                "exit code: 1".into(),
                "pg_ctl: server did not start in time".into(),
                quick
            ),
            PgError::Timeout { .. }
        ));
        assert!(matches!(
            pg_ctl_error(
                "pg_ctl start",
                "exit code: 1".into(),
                "pg_ctl: could not start server".into(),
                quick
            ),
            PgError::CommandFailed { .. }
        ));
        assert!(matches!(
            pg_ctl_error(
                "pg_ctl start",
                "exit code: 1".into(),
                "message localise".into(),
                Duration::from_secs(PG_CTL_TIMEOUT_SECS)
            ),
            PgError::Timeout { .. }
        ));
    }
}
