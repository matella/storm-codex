//! `storm-codex-server migrate [--to <version>]`: runs this build's migrations on
//! `DATABASE_URL` and exits, without starting the server. Used by the desktop shell to give an
//! imported backup's staging database the app's own schema (the backup holds data only): first
//! up to the backup's version, then (after the data is restored) to the newest one.
//!
//! Exit codes: 0 done; 2 usage (bad arguments, no `DATABASE_URL`); 3 the target version is newer
//! than this build (the backup comes from a newer app); 1 any other failure. Messages go to
//! stderr; `DATABASE_URL` (it holds a password) is never printed.

/// Exit code when `--to` names a version newer than this build's newest migration.
pub const EXIT_NEWER: i32 = 3;
const EXIT_USAGE: i32 = 2;
const EXIT_FAILED: i32 = 1;

/// The `--to` version, if any, from the arguments after `migrate`.
pub fn parse_args(args: &[String]) -> Result<Option<i64>, String> {
    match args {
        [] => Ok(None),
        [flag, v] if flag == "--to" => v
            .parse::<i64>()
            .map(Some)
            .map_err(|_| format!("--to expects a migration version, got {v:?}")),
        _ => Err("usage: storm-codex-server migrate [--to <version>]".into()),
    }
}

/// Runs the command; `Err((exit code, message))` on failure.
pub async fn run(args: &[String]) -> Result<(), (i32, String)> {
    let target = parse_args(args).map_err(|e| (EXIT_USAGE, e))?;
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| (EXIT_USAGE, "DATABASE_URL is not set".to_string()))?;
    let migrator = sqlx::migrate!("./migrations");
    let newest = migrator.iter().map(|m| m.version).max().unwrap_or_default();
    if let Some(t) = target {
        if t > newest {
            return Err((
                EXIT_NEWER,
                format!("database version {t} is newer than this build's ({newest})"),
            ));
        }
        if !migrator.version_exists(t) {
            return Err((EXIT_FAILED, format!("unknown database version {t}")));
        }
    }
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| (EXIT_FAILED, format!("Postgres connection: {e}")))?;
    let result = match target {
        Some(t) => migrator.run_to(t, &db).await,
        None => migrator.run(&db).await,
    };
    db.close().await;
    result.map_err(|e| (EXIT_FAILED, format!("migrations: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn migrate_arguments() {
        assert_eq!(parse_args(&args(&[])), Ok(None));
        assert_eq!(parse_args(&args(&["--to", "14"])), Ok(Some(14)));
        assert!(parse_args(&args(&["--to"])).is_err());
        assert!(parse_args(&args(&["--to", "x"])).is_err());
        assert!(parse_args(&args(&["--to", "1", "extra"])).is_err());
        assert!(parse_args(&args(&["--from", "1"])).is_err());
    }

    #[test]
    fn the_newest_migration_is_known() {
        let migrator = sqlx::migrate!("./migrations");
        assert!(migrator.iter().any(|m| m.version >= 16));
    }
}
