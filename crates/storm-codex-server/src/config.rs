//! Configuration par variables d'environnement (V2-ready : serveur stateless, tout par env).

use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub archive_dir: PathBuf,
    pub raw_cache_dir: PathBuf,
    pub raw_cache_max_bytes: u64,
    pub admin_token: String,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let var = |k: &str| std::env::var(k).map_err(|_| format!("variable {k} manquante"));
        Ok(Config {
            database_url: var("DATABASE_URL")?,
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8088".into()),
            archive_dir: PathBuf::from(
                std::env::var("ARCHIVE_DIR").unwrap_or_else(|_| "./.archive".into()),
            ),
            raw_cache_dir: PathBuf::from(
                std::env::var("RAW_CACHE_DIR").unwrap_or_else(|_| "./.raw-cache".into()),
            ),
            raw_cache_max_bytes: std::env::var("RAW_CACHE_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5 * 1024 * 1024 * 1024),
            admin_token: std::env::var("ADMIN_TOKEN").unwrap_or_else(|_| "dev-admin-token".into()),
        })
    }
}
