//! Configuration through environment variables (V2-ready: stateless server, everything through env).

use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub archive_dir: PathBuf,
    pub raw_cache_dir: PathBuf,
    pub raw_cache_max_bytes: u64,
    /// Admin token (Bearer) protecting the writes. `None` (ADMIN_TOKEN missing/empty) = **open
    /// mode**: no admin auth required — meant for local self-hosting (LAN/Tailscale,
    /// a single operator). Setting it re-enables auth (recommended if exposed through a reverse proxy).
    pub admin_token: Option<String>,
    /// Folder of the built front-end (`web-rs/dist`, Leptos): `overlay/` (cast and draft overlays), `sources/` (session
    /// and music sources) and `app/`, one Trunk output each. Every page answers at `/`; empty = serves only the API.
    pub web_dir: Option<PathBuf>,
    /// Jarvis Redis (optional); missing = no event emission.
    pub redis_url: Option<String>,
    pub jarvis_channel: String,
    /// Post-game push to the Twitch Azure EBS (optional).
    pub azure_push_url: Option<String>,
    pub azure_push_token: Option<String>,
    /// HotsPatchNotes API (hero/talent reference data) — replicated into `dim_*` at startup.
    pub hotspatchnotes_url: Option<String>,
    /// Published reference-data snapshot (`referential.tar.gz`) — standalone bundle mode, without live
    /// HotsPatchNotes. Takes precedence over `hotspatchnotes_url` when set. See `dim::ingest_snapshot`.
    pub referential_url: Option<String>,
    /// Standalone (installer) mode, `STANDALONE=1`: loopback-only bind, no network refresh of the
    /// reference data; it is seeded once from `referential_file`. See `Config::validate`.
    pub standalone: bool,
    /// Bundled `referential.tar.gz` on disk (`REFERENTIAL_FILE`), used to seed an empty
    /// `dim_heroes` in standalone mode. See `dim::ingest_snapshot_file`.
    pub referential_file: Option<PathBuf>,
    /// Hero portraits + map images vendored here at startup (self-sufficient,
    /// deterministic — no runtime dependency on HotsPatchNotes), served on `/images`.
    pub images_dir: PathBuf,
    /// Baked/committed in-game minimaps (`assets/minimaps`), copied into `images_dir/minimaps`
    /// at startup (idempotent) → served on `/images/minimaps`. No runtime fetch.
    pub minimaps_bundle_dir: PathBuf,
    /// Orpheus API (music) — proxied by `/api/now-playing` for the OBS music widget.
    pub orpheus_url: Option<String>,
    /// Outgoing webhook (Discord/ntfy/generic) notified at each new patch. Missing = no notification.
    pub patch_webhook_url: Option<String>,
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
            admin_token: std::env::var("ADMIN_TOKEN").ok().filter(|s| !s.is_empty()),
            web_dir: std::env::var("WEB_DIR")
                .ok()
                .map(PathBuf::from)
                .filter(|p| p.is_dir()),
            redis_url: std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty()),
            jarvis_channel: std::env::var("JARVIS_CHANNEL")
                .unwrap_or_else(|_| "jarvis:events".into()),
            azure_push_url: std::env::var("AZURE_PUSH_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            azure_push_token: std::env::var("AZURE_PUSH_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            hotspatchnotes_url: std::env::var("HOTSPATCHNOTES_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            referential_url: std::env::var("REFERENTIAL_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            standalone: std::env::var("STANDALONE").is_ok_and(|v| v == "1"),
            referential_file: std::env::var("REFERENTIAL_FILE")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            images_dir: PathBuf::from(
                std::env::var("IMAGES_DIR").unwrap_or_else(|_| "./.images".into()),
            ),
            minimaps_bundle_dir: PathBuf::from(
                std::env::var("MINIMAPS_BUNDLE_DIR").unwrap_or_else(|_| "assets/minimaps".into()),
            ),
            orpheus_url: std::env::var("ORPHEUS_URL").ok().filter(|s| !s.is_empty()),
            patch_webhook_url: std::env::var("PATCH_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }

    /// Cross-field checks. Standalone mode must never listen beyond the loopback interface.
    pub fn validate(&self) -> Result<(), String> {
        if self.standalone {
            let loopback = self
                .bind_addr
                .parse::<std::net::SocketAddr>()
                .is_ok_and(|a| a.ip().is_loopback());
            if !loopback {
                return Err("standalone mode requires a loopback BIND_ADDR".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(bind: &str, standalone: bool) -> Config {
        Config {
            database_url: String::new(),
            bind_addr: bind.into(),
            archive_dir: PathBuf::new(),
            raw_cache_dir: PathBuf::new(),
            raw_cache_max_bytes: 0,
            admin_token: None,
            web_dir: None,
            redis_url: None,
            jarvis_channel: String::new(),
            azure_push_url: None,
            azure_push_token: None,
            hotspatchnotes_url: None,
            referential_url: None,
            standalone,
            referential_file: None,
            images_dir: PathBuf::new(),
            minimaps_bundle_dir: PathBuf::new(),
            orpheus_url: None,
            patch_webhook_url: None,
        }
    }

    #[test]
    fn standalone_rejects_non_loopback_bind() {
        assert_eq!(
            cfg("0.0.0.0:8088", true).validate(),
            Err("standalone mode requires a loopback BIND_ADDR".to_string())
        );
        assert!(cfg("127.0.0.1:0", true).validate().is_ok());
        assert!(cfg("[::1]:8088", true).validate().is_ok());
        assert!(cfg("not-an-address", true).validate().is_err());
    }

    #[test]
    fn non_standalone_allows_any_bind() {
        assert!(cfg("0.0.0.0:8088", false).validate().is_ok());
    }
}
