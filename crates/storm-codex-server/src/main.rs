//! `storm-codex-server` — serveur unique (axum + Postgres) : upload, parse, projection,
//! WebSocket, REST. Jalon 3. Config par env (cf. `.env.example`).

mod admin;
mod auth;
mod azure;
mod builds;
mod cast;
mod config;
mod dim;
mod draft;
mod jarvis;
mod live;
mod lobby;
mod manage;
mod players;
pub mod project;
mod rank;
mod raw;
mod read;
mod replay2d;
mod scouting;
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
pub const PARSER_VERSION: i32 = 2;

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
    /// Série de la régie de cast (singleton, persisté dans `cast_live`).
    pub cast: Arc<RwLock<cast::Series>>,
    /// Version de l'état de cast en mémoire (attribuée sous le write-lock de chaque mutation) et
    /// dernière version écrite en base : un instantané plus ancien n'écrase jamais un plus récent.
    pub cast_version: Arc<std::sync::atomic::AtomicU64>,
    pub cast_saved: Arc<tokio::sync::Mutex<u64>>,
    /// Lobby live courant (singleton, persisté dans `lobby_live`). `None` = aucun lobby.
    pub lobby: Arc<RwLock<Option<lobby::LobbyState>>>,
    /// Partie en cours (feed live, en mémoire, jetable). `None` = aucune.
    pub live: Arc<RwLock<Option<live::LiveSession>>>,
    /// Minuterie de lecture auto du draft depuis un replay (une seule à la fois).
    pub replay_timer: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
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
    let mut draft = draft::store::load(&db).await.unwrap_or_else(|| {
        draft::DraftState::new(draft::Format::Standard, draft::Side::Blue, "Sky Temple".into())
    });
    // Aucune minuterie après un redémarrage : la lecture auto repart en pause (curseur conservé).
    draft.after_restart();
    // Série de cast : reprise du disque si lisible, sinon état neuf inactif (jamais d'échec au démarrage).
    let cast_initial = cast::store::load(&db).await.unwrap_or_else(cast::Series::new);
    let lobby_initial = lobby::store::load(&db).await;
    let state = AppState {
        cfg: Arc::new(cfg),
        db,
        parse_sem: Arc::new(Semaphore::new(cores)),
        events,
        draft: Arc::new(RwLock::new(draft)),
        cast: Arc::new(RwLock::new(cast_initial)),
        cast_version: Arc::default(),
        cast_saved: Arc::default(),
        lobby: Arc::new(RwLock::new(lobby_initial)),
        live: Arc::new(RwLock::new(None)),
        replay_timer: Arc::default(),
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
        .route("/api/players/search", get(players::api::search))
        .route("/api/players/{toon}", get(read::get_player))
        .route("/api/players/{toon}/identity", get(players::api::identity))
        .route("/api/players/{toon}/links/{other}", axum::routing::put(players::api::put_link))
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
        .route("/api/dim/hero-attributes", get(read::dim_hero_attributes))
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
        // Régie de cast (état autoritatif serveur + broadcast WS cast.updated), mode ouvert
        .route("/api/cast", get(cast::api::get_cast))
        .route(
            "/api/cast/series",
            post(cast::api::start_series).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/teams",
            axum::routing::put(cast::api::put_teams).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/pool/map",
            post(cast::api::mark_map).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/games/start",
            post(cast::api::start_game).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/games/{n}/winner",
            post(cast::api::set_winner).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route("/api/cast/games/{n}", axum::routing::delete(cast::api::remove_game))
        .route("/api/cast/games/{n}/replay", post(cast::replay::load_game_replay))
        .route(
            "/api/rank/result",
            post(rank::post_result).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route("/api/cast/replays/inspect", post(cast::replay::inspect))
        .route("/api/cast/replays/import", post(cast::replay::import_replay))
        .route(
            "/api/cast/score",
            post(cast::api::set_score).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/scene",
            post(cast::api::set_scene).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/lineup-mode",
            post(cast::api::set_lineup_mode).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route(
            "/api/cast/slide",
            post(cast::api::set_slide).layer(axum::extract::DefaultBodyLimit::max(cast::api::JSON_BODY_LIMIT)),
        )
        .route("/api/cast/lineup", get(cast::api::get_lineup))
        .route(
            "/api/cast/logo/{side}",
            post(cast::api::upload_logo)
                .layer(axum::extract::DefaultBodyLimit::max(cast::api::LOGO_BODY_LIMIT))
                .get(cast::api::get_logo),
        )
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
        .route("/api/draft/replay", get(draft::replay_api::get_replay))
        .route("/api/draft/replay/load", post(draft::replay_api::load))
        .route("/api/draft/replay/next", post(draft::replay_api::next))
        .route("/api/draft/replay/back", post(draft::replay_api::back))
        .route("/api/draft/replay/reset", post(draft::replay_api::reset))
        .route("/api/draft/replay/select", post(draft::replay_api::select))
        .route("/api/draft/replay/fearless", post(draft::replay_api::fearless))
        .route("/api/draft/replay/play", post(draft::replay_api::play))
        .route("/api/draft/replay/queue/move", post(draft::replay_api::move_game))
        .route("/api/draft/replay/queue/{i}", axum::routing::delete(draft::replay_api::remove))
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
        // Feed live d'une partie en cours (blocs TempWriteReplayP1) + broadcast WS live.updated
        .route("/api/live", get(live::api::get).delete(live::api::clear))
        .route("/api/live/chunk", post(live::api::chunk))
        .route("/api/live/end", post(live::api::end))
        // Rapports de scouting (tables dédiées, jamais projetés dans matches)
        .route("/api/scouting", get(scouting::api::list).post(scouting::api::create))
        .route(
            "/api/scouting/{id}",
            get(scouting::api::get)
                .patch(scouting::api::patch)
                .delete(scouting::api::delete),
        )
        .route("/api/scouting/{id}/replays", post(scouting::api::add_replay))
        .route(
            "/api/scouting/{id}/replays/{gid}",
            axum::routing::patch(scouting::api::set_side).delete(scouting::api::delete_replay),
        )
        .route("/api/scouting/{id}/pack.md", get(scouting::api::pack_md))
        .route("/api/scouting/{id}/pack.xlsx", get(scouting::api::pack_xlsx))
        .route(
            "/api/scouting/{id}/analysis",
            axum::routing::put(scouting::api::put_analysis).delete(scouting::api::delete_analysis),
        )
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
pub(crate) mod api_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    pub(crate) async fn test_state(admin_token: Option<&str>) -> Option<AppState> {
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
            cast: Arc::new(RwLock::new(cast::Series::new())),
            cast_version: Arc::default(),
            cast_saved: Arc::default(),
            lobby: Arc::new(RwLock::new(None)),
            live: Arc::new(RwLock::new(None)),
            replay_timer: Arc::default(),
            draft: Arc::new(RwLock::new(draft::DraftState::new(
                draft::Format::Standard,
                draft::Side::Blue,
                "Sky Temple".into(),
            ))),
        })
    }

    pub(crate) fn app(state: &AppState) -> Router {
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

    /// `name` = pseudo actuel de l'historique daté (pas `players.last_name`, écrasé dans l'ordre de
    /// traitement) ; `names` récent d'abord ; les champs historiques du résumé restent présents.
    #[tokio::test]
    async fn get_player_pseudo_actuel_vient_de_lhistorique_date() {
        use crate::players::testutil::{insert_match, row, uid};
        use chrono::TimeZone;
        let Some(state) = test_state(None).await else { return };
        let (toon, u) = (format!("t-{}", uid()), uid());
        let (old, new) = (format!("Old{u}"), format!("New{u}"));
        let t = |m| chrono::Utc.with_ymd_and_hms(2025, m, 1, 20, 0, 0).unwrap();
        insert_match(&state.db, Some(t(9)), &[row(&toon, &new, 0, "Muradin")]).await;
        insert_match(&state.db, Some(t(1)), &[row(&toon, &old, 0, "Muradin")]).await; // traité en dernier
        let resp = app(&state)
            .oneshot(Request::get(format!("/api/players/{toon}")).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["name"], new.as_str());
        assert_eq!(v["names"], serde_json::json!([new, old]));
        for k in ["toon", "matches", "wins", "avg_kills", "heroes", "recent"] {
            assert!(v.get(k).is_some(), "champ {k} manquant");
        }
    }

    /// `/api/players/search` (statique) ne doit pas tomber dans `/api/players/{toon}` ; requête
    /// courte → liste vide ; `_` littéral ; `limit` plafonné.
    #[tokio::test]
    async fn players_search_route_dediee_et_litterale() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else { return };
        let (toon, u) = (format!("t-{}", uid()), uid());
        insert_match(&state.db, None, &[row(&toon, &format!("Kerr{u}_Main"), 0, "Muradin")]).await;
        let get = |uri: String| {
            let st = state.clone();
            async move { app(&st).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap() }
        };
        let resp = get(format!("/api/players/search?q=Kerr{u}_M")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert!(v["hits"].as_array().unwrap().iter().any(|h| h["toon"] == toon.as_str()));
        assert!(v.get("toon").is_none(), "tombé dans get_player");
        let v = json_body(get("/api/players/search?q=a".into()).await).await;
        assert_eq!(v["hits"], serde_json::json!([]));
        assert_eq!(get("/api/players/search?q=ab&limit=9999".into()).await.status(), StatusCode::OK);
    }

    async fn put_link(st: &AppState, a: &str, b: &str, status: &str, bearer: Option<&str>) -> axum::response::Response {
        let mut req = Request::put(format!("/api/players/{a}/links/{b}"))
            .header("content-type", "application/json");
        if let Some(t) = bearer {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        app(st)
            .oneshot(req.body(Body::from(format!(r#"{{"status":"{status}"}}"#))).unwrap())
            .await
            .unwrap()
    }

    async fn get_identity(st: &AppState, toon: &str) -> axum::response::Response {
        app(st)
            .oneshot(Request::get(format!("/api/players/{toon}/identity")).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn identity_inconnu_404_et_put_soi_meme_400() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else { return };
        let resp = get_identity(&state, &format!("t-inconnu-{}", uid())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(json_body(resp).await["error"].is_string(), "404 applicatif, pas route absente");
        let toon = format!("t-{}", uid());
        insert_match(&state.db, None, &[row(&toon, "Solo", 0, "Muradin")]).await;
        assert_eq!(put_link(&state, &toon, &toon, "confirmed", None).await.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn put_link_garde_admin_statut_valide_et_inconnu() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(Some("sekret")).await else { return };
        let (a, b) = (format!("t-a{}", uid()), format!("t-b{}", uid()));
        insert_match(&state.db, None, &[row(&a, "A", 0, "Muradin"), row(&b, "B", 1, "Muradin")]).await;
        assert_eq!(put_link(&state, &a, &b, "confirmed", None).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(put_link(&state, &a, &b, "confirmed", Some("faux")).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(put_link(&state, &a, &b, "confirmed", Some("sekret")).await.status(), StatusCode::OK);
        let bad = put_link(&state, &a, &b, "maybe", Some("sekret")).await.status();
        assert!(bad.is_client_error(), "statut invalide refusé, reçu {bad}");
        let ghost = format!("t-ghost-{}", uid());
        assert_eq!(put_link(&state, &a, &ghost, "confirmed", Some("sekret")).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_link_est_symetrique_et_idempotent() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else { return };
        let (a, b) = (format!("t-a{}", uid()), format!("t-b{}", uid()));
        insert_match(&state.db, None, &[row(&a, "NameA", 0, "Muradin"), row(&b, "NameB", 1, "Muradin")]).await;
        assert_eq!(put_link(&state, &a, &b, "confirmed", None).await.status(), StatusCode::OK);
        assert_eq!(put_link(&state, &b, &a, "confirmed", None).await.status(), StatusCode::OK);
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM player_links WHERE toon_a IN ($1,$2) AND toon_b IN ($1,$2)")
            .bind(&a)
            .bind(&b)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 1, "A→B puis B→A = une seule ligne");
        for (me, other, other_name) in [(&a, &b, "NameB"), (&b, &a, "NameA")] {
            let v = json_body(get_identity(&state, me).await).await;
            let conf = v["confirmed"].as_array().unwrap();
            assert!(conf.iter().any(|c| c["toon"] == other.as_str() && c["name"] == other_name));
        }
    }

    #[tokio::test]
    async fn identity_historique_suggestions_et_decision() {
        use crate::players::testutil::smurf_world;
        let Some(state) = test_state(None).await else { return };
        let w = smurf_world(&state.db).await;
        let v = json_body(get_identity(&state, &w.target).await).await;
        assert_eq!(v["toon"], w.target.as_str());
        assert!(v["names"].as_array().unwrap().iter().any(|n| n["name"] == "N" && n["games"] == 18));
        let s = v["suggestions"].as_array().unwrap().iter().find(|s| s["toon"] == w.smurf.as_str()).expect("smurf suggéré");
        assert_eq!(s["level"], "high");
        assert!(s["score"].as_u64().unwrap() >= 50);
        assert!(!s["reasons"].as_array().unwrap().is_empty());
        // écarter → plus proposé, depuis la cible comme depuis le smurf
        assert_eq!(put_link(&state, &w.target, &w.smurf, "dismissed", None).await.status(), StatusCode::OK);
        for me in [&w.target, &w.smurf] {
            let v = json_body(get_identity(&state, me).await).await;
            let other = if me == &w.target { &w.smurf } else { &w.target };
            assert!(v["suggestions"].as_array().unwrap().iter().all(|s| s["toon"] != other.as_str()), "écarté mais reproposé");
            assert!(v["confirmed"].as_array().unwrap().is_empty());
        }
    }

    /// Codes attribut des bans → noms parser (attr.json), y compris les codes en capitales.
    #[tokio::test]
    async fn dim_hero_attributes_resout_les_codes_de_ban() {
        let Some(state) = test_state(None).await else { return };
        let resp = app(&state)
            .oneshot(Request::get("/api/dim/hero-attributes").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["Crus"], "Johanna");
        assert_eq!(v["DEAT"], "Deathwing");
        assert_eq!(v["L90E"], "E.T.C.");
        assert!(v.as_object().is_some_and(|o| o.len() >= 90));
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

    /// Scouting bout-en-bout : création → dépôt d'un replay → lot ambigu (1 partie : les 10
    /// joueurs à 100 %) → ancre → côté trouvé → faits → pack .md/.xlsx → import d'analyse →
    /// écrasement. Et l'isolation : rien n'est écrit dans uploads/matches pour ce replay.
    #[tokio::test]
    async fn scouting_cycle_complet_et_isolation() {
        let Some(state) = test_state(None).await else { return };
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("replay committé");
        let sha = crate::upload::sha256_hex(&bytes);
        let out = storm_stats::process_replay(&replay, "x");
        let fp = crate::upload::game_fingerprint(&out).unwrap();
        let counts = |db: sqlx::PgPool, sha: String, fp: String| async move {
            let u: i64 = sqlx::query_scalar("SELECT count(*) FROM uploads WHERE fingerprint = $1")
                .bind(&sha).fetch_one(&db).await.unwrap();
            let m: i64 = sqlx::query_scalar("SELECT count(*) FROM matches WHERE fingerprint = $1")
                .bind(&fp).fetch_one(&db).await.unwrap();
            (u, m)
        };
        let before = counts(state.db.clone(), sha.clone(), fp.clone()).await;

        let req = |method: &str, uri: String, body: Body| {
            Request::builder().method(method).uri(uri).header("content-type", "application/json").body(body).unwrap()
        };
        let resp = app(&state)
            .oneshot(req("POST", "/api/scouting".into(), Body::from(r#"{"title":"Test scout","target_name":"Team X"}"#)))
            .await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = json_body(resp).await["id"].as_i64().unwrap();

        let add = |b: Vec<u8>| {
            Request::post(format!("/api/scouting/{id}/replays"))
                .header("x-filename", "2024-10-07 22.29.55 Industrial%20District.StormReplay")
                .body(Body::from(b)).unwrap()
        };
        let v = json_body(app(&state).oneshot(add(bytes.clone())).await.unwrap()).await;
        assert_eq!(v["status"], "added", "{v}");
        assert!(v["target_team"].is_null(), "lot d'une partie = ambigu, pas de côté : {v}");
        let v = json_body(app(&state).oneshot(add(bytes.clone())).await.unwrap()).await;
        assert_eq!(v["status"], "duplicate");

        let detail = json_body(app(&state).oneshot(req("GET", format!("/api/scouting/{id}"), Body::empty())).await.unwrap()).await;
        assert_eq!(detail["snapshot"]["detection"]["ambiguous"], true);
        assert_eq!(detail["snapshot"]["facts"]["overview"]["excluded"], 1);
        assert_eq!(detail["status"], "ready");
        let anchor = detail["games"][0]["teams"][1][0]["toon"].as_str().unwrap().to_owned();

        // ancre côté 1 → côté trouvé, roster = les 5 coéquipiers, 1 partie analysée
        let body = serde_json::json!({ "anchors": [anchor] }).to_string();
        let detail = json_body(app(&state).oneshot(req("PATCH", format!("/api/scouting/{id}"), Body::from(body))).await.unwrap()).await;
        assert_eq!(detail["games"][0]["target_team"], 1, "{}", detail["games"]);
        assert_eq!(detail["games"][0]["target_source"], "roster");
        assert_eq!(detail["snapshot"]["roster"].as_array().unwrap().len(), 5);
        assert_eq!(detail["snapshot"]["facts"]["overview"]["games"], 1);
        let fv = detail["facts_version"].as_i64().unwrap();

        // côté manuel puis retour à l'automatique
        let body = r#"{"target_team":0}"#.to_string();
        let gid = detail["games"][0]["gid"].as_i64().unwrap();
        let d2 = json_body(app(&state).oneshot(req("PATCH", format!("/api/scouting/{id}/replays/{gid}"), Body::from(body))).await.unwrap()).await;
        assert_eq!(d2["games"][0]["target_source"], "manual");
        let d2 = json_body(app(&state).oneshot(req("PATCH", format!("/api/scouting/{id}/replays/{gid}"), Body::from(r#"{"target_team":null}"#))).await.unwrap()).await;
        assert_eq!(d2["games"][0]["target_team"], 1);
        let fv = d2["facts_version"].as_i64().unwrap().max(fv);

        // packs
        let resp = app(&state).oneshot(req("GET", format!("/api/scouting/{id}/pack.md"), Body::empty())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let md = String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
        assert!(md.contains(&format!("report_id: **{id}**")));
        assert!(md.contains("`map.industrial_district.record`"));
        let resp = app(&state).oneshot(req("GET", format!("/api/scouting/{id}/pack.xlsx"), Body::empty())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let x = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&x[..2], b"PK");

        // import : JSON cassé → 422 et rien d'écrit ; valide → importé ; ré-import → écrasé
        let resp = app(&state).oneshot(req("PUT", format!("/api/scouting/{id}/analysis"), Body::from("```json\n{bad}\n```"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // plan de draft v2 : 1 carte citant un fait réel + un id inconnu, 1 considération sans preuve
        let analysis = |summary: &str| format!(
            "Here you go:\n```json\n{{\"format_version\":2,\"report_id\":{id},\"facts_version\":{fv},\"summary\":\"{summary}\",\"maps\":[{{\"map\":\"Industrial District\",\"confidence\":\"low\",\"overview\":\"o\",\"evidence\":[\"map.industrial_district.record\",\"nope\"],\"considerations\":[{{\"point\":\"y\",\"evidence\":[]}}]}}]}}\n```");
        let resp = app(&state).oneshot(req("PUT", format!("/api/scouting/{id}/analysis"), Body::from(analysis("first")))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["tally"]["claims"], 2, "{v}");
        assert_eq!(v["tally"]["unsupported"], 1);
        app(&state).oneshot(req("PUT", format!("/api/scouting/{id}/analysis"), Body::from(analysis("second")))).await.unwrap();
        let detail = json_body(app(&state).oneshot(req("GET", format!("/api/scouting/{id}"), Body::empty())).await.unwrap()).await;
        assert_eq!(detail["analysis"]["summary"], "second");
        assert_eq!(detail["status"], "analyzed");
        assert_eq!(detail["analysis"]["maps"][0]["evidence"][0]["known"], true);
        assert_eq!(detail["analysis"]["maps"][0]["evidence"][1]["known"], false);

        // isolation : aucune écriture dans uploads/matches pour ce replay
        assert_eq!(counts(state.db.clone(), sha, fp).await, before);

        let resp = app(&state).oneshot(req("DELETE", format!("/api/scouting/{id}"), Body::empty())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
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

    // ── draft depuis un replay ────────────────────────────────────────────────

    async fn with_game(state: &AppState) {
        let fx: serde_json::Value = serde_json::from_str(include_str!("draft/fixtures/custom_draft.json")).unwrap();
        let out = storm_stats::Output {
            status: 1,
            match_: fx["match"].as_object().cloned(),
            players: fx["players"].as_object().cloned(),
        };
        let g = draft::replay_extract::loaded_game_from_output(&out, "fx.StormReplay", storm_stats::constants::hero_attribute())
            .unwrap();
        state.draft.write().await.add_game(g);
    }

    /// Attend (au plus 5 s) que l'état de draft vérifie `ok`, et renvoie le curseur observé.
    async fn wait_until(state: &AppState, ok: impl Fn(&draft::DraftState) -> bool) -> usize {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let d = state.draft.read().await;
            if ok(&d) || std::time::Instant::now() >= deadline {
                return d.cursor;
            }
            drop(d);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    pub(crate) async fn call(state: &AppState, method: &str, path: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(path);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let resp = app(state).oneshot(req.body(body).unwrap()).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    /// Requête à corps brut (octets) ; renvoie statut + octets de la réponse.
    pub(crate) async fn call_raw(state: &AppState, method: &str, path: &str, content_type: Option<&str>, body: Vec<u8>) -> (StatusCode, Vec<u8>) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        let resp = app(state).oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
        let status = resp.status();
        (status, resp.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    #[tokio::test]
    async fn replay_load_rejects_empty_body() {
        let Some(state) = test_state(None).await else { return };
        let resp = app(&state)
            .oneshot(Request::post("/api/draft/replay/load").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn replay_load_rejects_non_custom_and_garbage_and_queues_nothing() {
        let Some(state) = test_state(None).await else { return };
        let aram = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../storm-stats/tests/data/silver-city-aram.StormReplay")).unwrap();
        for bytes in [aram, vec![7u8; 64]] {
            let resp = app(&state)
                .oneshot(Request::post("/api/draft/replay/load").body(Body::from(bytes)).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }
        assert!(state.draft.read().await.replay.queue.is_empty());
    }

    #[tokio::test]
    async fn replay_next_back_errors_are_409_and_change_nothing() {
        let Some(state) = test_state(None).await else { return };
        assert_eq!(call(&state, "POST", "/api/draft/replay/next", None).await.0, StatusCode::CONFLICT); // pas de partie
        with_game(&state).await;
        assert_eq!(call(&state, "POST", "/api/draft/replay/back", None).await.0, StatusCode::CONFLICT); // au début
        for _ in 0..12 {
            assert_eq!(call(&state, "POST", "/api/draft/replay/next", None).await.0, StatusCode::OK);
        }
        assert_eq!(call(&state, "POST", "/api/draft/replay/next", None).await.0, StatusCode::CONFLICT); // fini
        assert_eq!(state.draft.read().await.cursor, 16);
    }

    #[tokio::test]
    async fn public_draft_hides_future_but_replay_route_has_script() {
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        call(&state, "POST", "/api/draft/replay/next", None).await;
        let (_, public) = call(&state, "GET", "/api/draft", None).await;
        assert!(public["replay"]["queue"][0]["script"].as_array().unwrap().is_empty());
        assert_eq!(public["assignments"].as_array().unwrap().iter().filter(|a| !a.is_null()).count(), 1);
        let (_, full) = call(&state, "GET", "/api/draft/replay", None).await;
        assert_eq!(full["queue"][0]["script"].as_array().unwrap().len(), 16);
    }

    #[tokio::test]
    async fn replay_autoplay_ticks_then_pause_stops() {
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        let (st, _) = call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": true, "interval_ms": 20}))).await;
        assert_eq!(st, StatusCode::OK);
        // attente bornée au lieu d'un sommeil fixe : un runner CI chargé peut retarder les ticks
        let c = wait_until(&state, |d| d.cursor >= 2).await;
        assert!(c >= 2, "la lecture auto n'avance pas (curseur {c})");
        call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": false}))).await;
        let paused = state.draft.read().await.cursor;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(state.draft.read().await.cursor, paused);
        // intervalle absurde → 400
        let (st, _) = call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": true, "interval_ms": 1}))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn replay_autoplay_stops_itself_at_the_end() {
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": true, "interval_ms": 10}))).await;
        wait_until(&state, |d| d.is_complete() && !d.replay.playing).await;
        let d = state.draft.read().await;
        assert!(d.is_complete() && !d.replay.playing);
    }

    #[tokio::test]
    async fn replay_manual_next_while_playing_never_double_reveals() {
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": true, "interval_ms": 60_000}))).await;
        call(&state, "POST", "/api/draft/replay/next", None).await;
        // 1 phase = 1 étape de ban → curseur 1 exactement, le timer 60 s n'a pas tiré
        assert_eq!(state.draft.read().await.cursor, 1);
        call(&state, "POST", "/api/draft/replay/play", Some(serde_json::json!({"playing": false}))).await;
    }
}
