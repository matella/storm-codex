//! Configuration par variables d'environnement (V2-ready : serveur stateless, tout par env).

use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub archive_dir: PathBuf,
    pub raw_cache_dir: PathBuf,
    pub raw_cache_max_bytes: u64,
    /// Token admin (Bearer) protégeant les écritures. `None` (ADMIN_TOKEN absent/vide) = **mode
    /// ouvert** : aucune auth admin requise — destiné à l'auto-hébergement local (LAN/Tailscale,
    /// un seul opérateur). Le définir réactive l'auth (recommandé si exposé via reverse proxy).
    pub admin_token: Option<String>,
    /// Dossier du front buildé (web/dist) servi sur `/` ; vide = ne sert que l'API.
    pub web_dir: Option<PathBuf>,
    /// Redis Jarvis (option) ; absent = pas d'émission d'événements.
    pub redis_url: Option<String>,
    pub jarvis_channel: String,
    /// Push post-game vers l'EBS Twitch Azure (option).
    pub azure_push_url: Option<String>,
    pub azure_push_token: Option<String>,
    /// API HotsPatchNotes (référentiel héros/talents) — répliquée dans `dim_*` au démarrage.
    pub hotspatchnotes_url: Option<String>,
    /// Portraits héros + images de cartes vendorisés ici au démarrage (auto-suffisant,
    /// déterministe — pas de dépendance runtime à HotsPatchNotes), servis sur `/images`.
    pub images_dir: PathBuf,
    /// API Orpheus (musique) — proxifiée par `/api/now-playing` pour le widget musique OBS.
    pub orpheus_url: Option<String>,
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
            azure_push_url: std::env::var("AZURE_PUSH_URL").ok().filter(|s| !s.is_empty()),
            azure_push_token: std::env::var("AZURE_PUSH_TOKEN").ok().filter(|s| !s.is_empty()),
            hotspatchnotes_url: std::env::var("HOTSPATCHNOTES_URL").ok().filter(|s| !s.is_empty()),
            images_dir: PathBuf::from(
                std::env::var("IMAGES_DIR").unwrap_or_else(|_| "./.images".into()),
            ),
            orpheus_url: std::env::var("ORPHEUS_URL").ok().filter(|s| !s.is_empty()),
        })
    }
}
