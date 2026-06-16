//! `storm-codex-server` — serveur unique (axum + Postgres) : upload, parse, projection,
//! WebSocket, REST. Jalon 3. Config par env (cf. `.env.example`).

mod admin;
mod azure;
mod config;
mod dim;
mod jarvis;
mod manage;
pub mod project;
mod raw;
mod read;
mod upload;
mod ws;

use axum::{
    extract::State, http::StatusCode, routing::any, routing::get, routing::post, Json, Router,
};
use config::Config;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{broadcast, Semaphore};

/// Version du projecteur — bumper quand la projection change ; pilote le re-process idempotent.
pub const PARSER_VERSION: i32 = 1;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: PgPool,
    /// Limite les parses CPU concurrents (= nb de cœurs).
    pub parse_sem: Arc<Semaphore>,
    /// Diffusion temps réel (WS) — `match.parsed`, progression backfill.
    pub events: broadcast::Sender<serde_json::Value>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "storm_codex_server=info,tower_http=warn".into()),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!("démarrage impossible : {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let cfg = Config::from_env()?;
    std::fs::create_dir_all(&cfg.archive_dir).map_err(|e| format!("archive_dir : {e}"))?;
    std::fs::create_dir_all(&cfg.raw_cache_dir).map_err(|e| format!("raw_cache_dir : {e}"))?;

    let db = PgPoolOptions::new()
        .max_connections(16)
        .connect(&cfg.database_url)
        .await
        .map_err(|e| format!("connexion Postgres : {e}"))?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .map_err(|e| format!("migrations : {e}"))?;

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let (events, _) = broadcast::channel(1024);
    let state = AppState {
        cfg: Arc::new(cfg),
        db,
        parse_sem: Arc::new(Semaphore::new(cores)),
        events,
    };

    // Référentiel héros/talents/patches + images (best-effort, refresh 24 h ; chaque nouveau patch →
    // notif WS in-app + webhook optionnel). Deux sources mutuellement exclusives :
    //  - REFERENTIAL_URL  : snapshot publié (`referential.tar.gz`) → bundle autonome, sans HPN live.
    //  - HOTSPATCHNOTES_URL : API HotsPatchNotes live (setup mainteneur).
    // Le snapshot est prioritaire quand les deux sont définis.
    if state.cfg.referential_url.is_some() || state.cfg.hotspatchnotes_url.is_some() {
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let new_patches = if let Some(url) = st.cfg.referential_url.clone() {
                    dim::ingest_snapshot(&st.db, &st.cfg.images_dir, &url).await
                } else if let Some(url) = st.cfg.hotspatchnotes_url.clone() {
                    // sync une-fois des héros/talents/images au 1er tour (idempotent ensuite).
                    dim::sync_heroes(&st.db, &url).await;
                    dim::sync_talents(&st.db, &url).await;
                    dim::vendor_images(&st.cfg.images_dir, &url).await;
                    dim::sync_patches(&st.db, &url).await
                } else {
                    Vec::new()
                };
                for (iid, name) in new_patches {
                    let _ = st.events.send(serde_json::json!({
                        "type": "patch.new", "internalId": iid, "name": name,
                    }));
                    if let Some(hook) = st.cfg.patch_webhook_url.clone() {
                        let body = serde_json::json!({
                            "content": format!("🆕 New HotS patch: {name}"),
                            "patchName": name, "internalId": iid,
                        });
                        let _ = tokio::task::spawn_blocking(move || {
                            let _ = ureq::post(&hook).send_json(body);
                        })
                        .await;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
            }
        });
    }
    let bind = state.cfg.bind_addr.clone();

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/upload", post(upload::upload))
        // alias compat client-rs (Hots-Overlay) : il poste sur /api/upload-raw (octets bruts,
        // header X-Filename, Bearer) — même handler, mêmes garanties.
        .route("/api/upload-raw", post(upload::upload))
        .route("/api/matches", get(read::list_matches))
        .route("/api/matches/{id}", get(read::get_match))
        .route("/api/matches/{id}/raw", get(raw::get_raw))
        .route("/api/players/{toon}", get(read::get_player))
        .route("/api/heroes", get(read::list_heroes))
        .route("/api/hero/{hero}", get(read::hero_detail))
        .route("/api/synergies", get(read::synergies))
        .route("/api/patches", get(read::patches_list))
        .route("/api/patches/{id}", get(read::patch_detail))
        .route("/api/maps", get(read::list_maps))
        .route("/api/dim/heroes", get(read::dim_heroes))
        .route("/api/dim/talents", get(read::dim_talents))
        .route("/api/matches.csv", get(read::matches_csv))
        .route("/api/trends", get(manage::trends))
        .route("/api/now-playing", get(read::now_playing))
        .route("/api/settings", get(manage::get_settings))
        .route("/api/admin/settings", axum::routing::put(manage::put_settings))
        .route("/api/teams", get(manage::list_teams).post(manage::create_team))
        .route("/api/teams/{id}", axum::routing::delete(manage::delete_team).put(manage::update_team))
        .route("/api/collections", get(manage::list_collections).post(manage::create_collection))
        .route("/api/collections/{id}", axum::routing::delete(manage::delete_collection))
        .route("/api/admin/tokens", post(admin::create_token))
        .route(
            "/api/admin/tokens/{id}",
            axum::routing::delete(admin::revoke_token),
        )
        .route("/api/admin/uploads", get(admin::uploads_health))
        .route("/api/admin/reprocess", post(admin::reprocess))
        .route("/ws", any(ws::ws_handler))
        // portraits héros + images de cartes vendorisés (servis depuis images_dir)
        .nest_service(
            "/images",
            tower_http::services::ServeDir::new(&state.cfg.images_dir),
        );

    // Front buildé (SPA) : ServeDir sert les assets ; toute route inconnue renvoie index.html
    // (statut 200) pour que le routing client React fonctionne sur les liens profonds.
    let app = match &state.cfg.web_dir {
        Some(dir) => {
            let index = std::fs::read_to_string(dir.join("index.html")).unwrap_or_default();
            // index.html en `no-cache` : non fingerprinté, il doit toujours être revalidé sinon
            // un redeploy (nouveau hash de bundle) laisse le navigateur sur un bundle 404 → page
            // blanche. Les assets (fingerprintés) restent cachables par ServeDir.
            let spa = axum::routing::get(move || {
                let index = index.clone();
                async move {
                    (
                        [(axum::http::header::CACHE_CONTROL, "no-cache")],
                        axum::response::Html(index),
                    )
                }
            });
            // `append_index_html_on_directories(false)` → "/" tombe sur le handler SPA (no-cache)
            // au lieu d'être servi par ServeDir sans en-tête de cache.
            app.fallback_service(
                tower_http::services::ServeDir::new(dir)
                    .append_index_html_on_directories(false)
                    .fallback(spa),
            )
        }
        None => app,
    }
    .with_state(state)
    // Limite de corps de requête : le défaut axum (2 Mo) rejetait en 413 — AVANT le handler, donc
    // sans trace ni ligne uploads — les replays de longues parties (un Braxis Holdout 5v5 dépasse
    // 2 Mo), faisant boucler l'uploader. 64 Mo couvre largement (replays < ~10 Mo).
    .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024));

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("bind {bind} : {e}"))?;
    tracing::info!("storm-codex-server à l'écoute sur {bind}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve : {e}"))
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let db_up = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let code = if db_up {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(serde_json::json!({
            "status": if db_up { "ok" } else { "degraded" },
            "parser_version": PARSER_VERSION,
            "db": if db_up { "up" } else { "down" },
        })),
    )
}
