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
    /// Folder of the built front (web/dist) served on `/`; empty = serves only the API.
    pub web_dir: Option<PathBuf>,
    /// Folder of the built Rust front-end (`web-rs`, Leptos) served under `/v2/` while it is ported;
    /// empty = no `/v2`. Holds `overlay/` (OBS sources) and `app/`, one Trunk output each.
    pub web_v2_dir: Option<PathBuf>,
    /// `WEB_ROOT=rust`: the Rust front-end (`web_v2_dir`) answers every page at `/` instead of React (`web_dir`), which
    /// stays in the image for rollback (`WEB_ROOT=react`, the default). `/v2/...` keeps working either way.
    pub web_root_rust: bool,
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
            web_v2_dir: std::env::var("WEB_V2_DIR")
                .ok()
                .map(PathBuf::from)
                .filter(|p| p.is_dir()),
            web_root_rust: std::env::var("WEB_ROOT").is_ok_and(|v| v.eq_ignore_ascii_case("rust")),
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
}
