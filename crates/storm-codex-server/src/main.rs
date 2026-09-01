//! `storm-codex-server` — serveur unique (axum + Postgres) : upload, parse, projection,
//! WebSocket, REST. Jalon 3. Config par env (cf. `.env.example`).

mod admin;
mod auth;
mod azure;
mod builds;
mod config;
mod dim;
mod draft;
mod jarvis;
mod lobby;
mod manage;
pub mod project;
mod raw;
mod read;
mod replay2d;
mod upload;
mod ws;

use axum::{
    extract::State, http::StatusCode, routing::any, routing::get, routing::post, Json, Router,
};
use config::Config;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Semaphore};

/// Version du projecteur — bumper quand la projection change ; pilote le re-process idempotent.
pub const PARSER_VERSION: i32 = 1;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: PgPool,
    /// Limite les parses CPU concurrents (= nb de cœurs).
    pub parse_sem: Arc<Semaphore>,
    /// Diffusion temps réel (WS) — `match.parsed`, progression backfill, `draft.updated`.
    pub events: broadcast::Sender<serde_json::Value>,
    /// État autoritatif du simulateur de draft (singleton, persisté dans `draft_live`).
    pub draft: Arc<RwLock<draft::DraftState>>,
    /// Lobby live courant (singleton, persisté dans `lobby_live`). `None` = aucun lobby.
    pub lobby: Arc<RwLock<Option<lobby::LobbyState>>>,
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

/// Copie les minimaps bakées (`bundle`, ex. `assets/minimaps`) dans `images_dir/minimaps`
/// (idempotent : saute les fichiers déjà présents). Servies ensuite sur `/images/minimaps` par le
/// ServeDir existant. Best-effort : une erreur ne bloque pas le démarrage (fallback front sur l'art peint).
fn bundle_minimaps(bundle: &std::path::Path, images_dir: &std::path::Path) {
    let dst = images_dir.join("minimaps");
    if std::fs::create_dir_all(&dst).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(bundle) else {
        return; // bundle absent (dev sans assets) → pas de minimaps, fallback front
    };
    let mut copied = 0u32;
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name() {
            let target = dst.join(name);
            if !target.exists() && std::fs::copy(&path, &target).is_ok() {
                copied += 1;
            }
        }
    }
    if copied > 0 {
        tracing::info!("minimaps bakées : {copied} copiées dans {}", dst.display());
    }
}

async fn run() -> Result<(), String> {
    let cfg = Config::from_env()?;
    std::fs::create_dir_all(&cfg.archive_dir).map_err(|e| format!("archive_dir : {e}"))?;
    std::fs::create_dir_all(&cfg.raw_cache_dir).map_err(|e| format!("raw_cache_dir : {e}"))?;
    bundle_minimaps(&cfg.minimaps_bundle_dir, &cfg.images_dir);

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
    // État de draft : repris du disque si présent, sinon neuf (Standard, blue first-pick, Sky Temple).
    let draft = draft::store::load(&db).await.unwrap_or_else(|| {
        draft::DraftState::new(draft::Format::Standard, draft::Side::Blue, "Sky Temple".into())
    });
    let lobby_initial = lobby::store::load(&db).await;
    let state = AppState {
        cfg: Arc::new(cfg),
        db,
        parse_sem: Arc::new(Semaphore::new(cores)),
        events,
        draft: Arc::new(RwLock::new(draft)),
        lobby: Arc::new(RwLock::new(lobby_initial)),
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
                    let np = dim::sync_patches(&st.db, &url).await;
                    dim::backfill_hero_sections(&st.db, &url).await; // projette les sections héros (live)
                    np
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
    let app = api_router(&state);
    let app = serve_spa(app, &state).with_state(state)
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

/// Toutes les routes API + images (sans le fallback SPA) — extraite pour les tests d'intégration.
fn api_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/upload", post(upload::upload))
        // alias compat client-rs (Hots-Overlay) : il poste sur /api/upload-raw (octets bruts,
        // header X-Filename, Bearer) — même handler, mêmes garanties.
        .route("/api/upload-raw", post(upload::upload))
        .route("/api/matches", get(read::list_matches))
        .route("/api/matches/{id}", get(read::get_match))
        .route("/api/matches/{id}/raw", get(raw::get_raw))
        .route("/api/matches/{id}/replay2d", get(replay2d::get_replay2d))
        .route("/api/players/{toon}", get(read::get_player))
        .route("/api/heroes", get(read::list_heroes))
        .route("/api/hero/{hero}", get(read::hero_detail))
        .route("/api/hero/{hero}/patches", get(read::hero_patches))
        .route("/api/hero-changes", get(read::hero_changes))
        .route("/api/hero-changes/heroes", get(read::hero_changes_heroes))
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
        .route("/api/admin/minimap-anchors", axum::routing::put(manage::put_minimap_anchors))
        .route("/api/teams", get(manage::list_teams).post(manage::create_team))
        .route("/api/teams/{id}", axum::routing::delete(manage::delete_team).put(manage::update_team))
        .route("/api/collections", get(manage::list_collections).post(manage::create_collection))
        .route("/api/collections/{id}", axum::routing::delete(manage::delete_collection))
        .route("/api/builds", get(builds::list).post(builds::create))
        .route(
            "/api/builds/{id}",
            axum::routing::put(builds::update).delete(builds::delete),
        )
        .route("/api/builds/from-match", post(builds::from_match))
        .route("/api/admin/tokens", post(admin::create_token))
        .route(
            "/api/admin/tokens/{id}",
            axum::routing::delete(admin::revoke_token),
        )
        .route("/api/admin/uploads", get(admin::uploads_health))
        .route("/api/admin/reprocess", post(admin::reprocess))
        // Simulateur de draft (état autoritatif serveur + broadcast WS draft.updated)
        .route("/api/draft", get(draft::api::get_draft))
        .route("/api/draft/config", post(draft::api::config))
        .route("/api/draft/action", post(draft::api::action))
        .route("/api/draft/undo", post(draft::api::undo))
        .route("/api/draft/reset", post(draft::api::reset))
        .route("/api/draft/unavailable", post(draft::api::unavailable))
        .route("/api/draft/score", post(draft::api::score))
        .route("/api/draft/teams", post(draft::api::teams))
        .route("/api/draft/series/next", post(draft::api::series_next))
        .route("/api/draft/series/new", post(draft::api::series_new))
        // Lobby live (companion) : détection pendant l'écran de chargement + broadcast WS lobby.detected
        .route(
            "/api/lobby",
            get(lobby::api::get)
                .post(lobby::api::ingest)
                .delete(lobby::api::clear),
        )
        .route("/api/lobby/hero", post(lobby::api::set_hero))
        .route("/api/lobby/map", post(lobby::api::set_map))
        .route("/api/lobby/teams", post(lobby::api::set_teams))
        .route("/ws", any(ws::ws_handler))
        // portraits héros + images de cartes vendorisés (servis depuis images_dir)
        .nest_service(
            "/images",
            tower_http::services::ServeDir::new(&state.cfg.images_dir),
        )
}

/// Front buildé (SPA) : ServeDir sert les assets ; toute route inconnue renvoie index.html
/// (statut 200) pour que le routing client React fonctionne sur les liens profonds.
fn serve_spa(app: Router<AppState>, state: &AppState) -> Router<AppState> {
    match &state.cfg.web_dir {
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

/// Tests d'intégration niveau routeur (tower::oneshot, sans écoute réseau). Ignorés sans
/// `DATABASE_URL` (pattern du test de projection) — lancer avec le Postgres Docker du dev,
/// exécutés en CI contre un service Postgres.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod api_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    async fn test_state(admin_token: Option<&str>) -> Option<AppState> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL absent → test d'intégration API ignoré");
            return None;
        };
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .expect("connexion");
        sqlx::migrate!("./migrations").run(&db).await.expect("migrations");
        let dir = std::env::temp_dir().join(format!("storm-codex-test-{}", uuid::Uuid::new_v4()));
        let cfg = config::Config {
            database_url: url,
            bind_addr: "127.0.0.1:0".into(),
            archive_dir: dir.join("archive"),
            raw_cache_dir: dir.join("raw-cache"),
            raw_cache_max_bytes: 64 * 1024 * 1024,
            admin_token: admin_token.map(str::to_owned),
            web_dir: None,
            redis_url: None,
            jarvis_channel: "test".into(),
            azure_push_url: None,
            azure_push_token: None,
            hotspatchnotes_url: None,
            referential_url: None,
            images_dir: dir.join("images"),
            minimaps_bundle_dir: dir.join("minimaps"),
            orpheus_url: None,
            patch_webhook_url: None,
        };
        std::fs::create_dir_all(&cfg.archive_dir).unwrap();
        std::fs::create_dir_all(&cfg.raw_cache_dir).unwrap();
        let (events, _) = tokio::sync::broadcast::channel(64);
        Some(AppState {
            cfg: Arc::new(cfg),
            db,
            parse_sem: Arc::new(Semaphore::new(2)),
            events,
            lobby: Arc::new(RwLock::new(None)),
            draft: Arc::new(RwLock::new(draft::DraftState::new(
                draft::Format::Standard,
                draft::Side::Blue,
                "Sky Temple".into(),
            ))),
        })
    }

    fn app(state: &AppState) -> Router {
        api_router(state).with_state(state.clone())
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_repond_ok_avec_db() {
        let Some(state) = test_state(None).await else { return };
        let resp = app(&state)
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["status"], "ok");
        assert_eq!(v["parser_version"], PARSER_VERSION);
    }

    #[tokio::test]
    async fn admin_ferme_exige_bearer_et_ouvre_avec() {
        let Some(state) = test_state(Some("s3cret")).await else { return };
        // sans token → 401
        let resp = app(&state)
            .oneshot(
                Request::post("/api/admin/tokens")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"t"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        // avec le bon Bearer → créé
        let resp = app(&state)
            .oneshot(
                Request::post("/api/admin/tokens")
                    .header("authorization", "Bearer s3cret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"test-integration"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let v = json_body(resp).await;
        assert!(v["token"].as_str().is_some_and(|t| !t.is_empty()));
    }

    /// Bout-en-bout réel : création de token → upload d'un replay committé → parse → projection
    /// → lecture du détail (avec `match.messages`) → dédup 409 au re-upload.
    #[tokio::test]
    async fn upload_parse_lecture_et_dedup() {
        let Some(state) = test_state(None).await else { return };
        // token d'upload (mode admin ouvert)
        let resp = app(&state)
            .oneshot(
                Request::post("/api/admin/tokens")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"e2e"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let token = json_body(resp).await["token"].as_str().unwrap().to_owned();

        // upload du replay de référence (alias compat client-rs)
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("replay committé");
        // purge d'un éventuel run précédent (idempotence du test)
        let hash = crate::upload::sha256_hex(&bytes);
        sqlx::query("DELETE FROM uploads WHERE fingerprint = $1")
            .bind(&hash)
            .execute(&state.db)
            .await
            .unwrap();
        let upload_req = |b: Vec<u8>, tok: &str| {
            Request::post("/api/upload-raw")
                .header("authorization", format!("Bearer {tok}"))
                .header("x-filename", "2026-06-09 20.35.02 Industrial%20District.StormReplay")
                .body(Body::from(b))
                .unwrap()
        };
        let resp = app(&state).oneshot(upload_req(bytes.clone(), &token)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["status"], "parsed", "réponse : {v}");
        let match_id = v["match_id"].as_i64().expect("match_id");

        // lecture du détail : forme {match, players}, messages présents (chat + pings)
        let resp = app(&state)
            .oneshot(Request::get(format!("/api/matches/{match_id}")).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let detail = json_body(resp).await;
        assert_eq!(detail["match"]["map"], "Industrial District");
        assert_eq!(detail["players"].as_object().unwrap().len(), 10);
        assert!(
            detail["match"]["messages"].as_array().is_some_and(|m| !m.is_empty()),
            "messages absents du détail"
        );

        // re-upload identique → 409 duplicate
        let resp = app(&state).oneshot(upload_req(bytes, &token)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // sans token → 401
        let resp = app(&state)
            .oneshot(
                Request::post("/api/upload-raw")
                    .header("x-filename", "x.StormReplay")
                    .body(Body::from(vec![0u8; 8]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
