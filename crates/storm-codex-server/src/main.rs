//! `storm-codex-server` — single server (axum + Postgres): upload, parse, projection,
//! WebSocket, REST. Milestone 3. Config through env (see `.env.example`). Subcommand
//! `migrate [--to <version>]`: migrations only (`migrate_cmd`).

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
mod migrate_cmd;
mod players;
pub mod project;
mod rank;
mod raw;
mod read;
mod replay2d;
mod scouting;
mod standalone;
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

/// Projector version — bump when the projection changes; drives the idempotent re-process.
pub const PARSER_VERSION: i32 = 4;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: PgPool,
    /// Limits the concurrent CPU parses (= number of cores).
    pub parse_sem: Arc<Semaphore>,
    /// Real-time broadcast (WS) — `match.parsed`, backfill progress, `draft.updated`.
    pub events: broadcast::Sender<serde_json::Value>,
    /// Authoritative state of the draft simulator (singleton, persisted in `draft_live`).
    pub draft: Arc<RwLock<draft::DraftState>>,
    /// Series of the cast control room (singleton, persisted in `cast_live`).
    pub cast: Arc<RwLock<cast::Series>>,
    /// Version of the in-memory cast state (assigned under the write-lock of each mutation) and
    /// last version written to the database: an older snapshot never overwrites a more recent one.
    pub cast_version: Arc<std::sync::atomic::AtomicU64>,
    pub cast_saved: Arc<tokio::sync::Mutex<u64>>,
    /// Current live lobby (singleton, persisted in `lobby_live`). `None` = no lobby.
    pub lobby: Arc<RwLock<Option<lobby::LobbyState>>>,
    /// Game in progress (live feed, in memory, disposable). `None` = none.
    pub live: Arc<RwLock<Option<live::LiveSession>>>,
    /// Auto playback timer of the draft from a replay (only one at a time).
    pub replay_timer: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Standalone (installer) mode: import status/commands shared with the import task.
    /// `Some` only when `cfg.standalone`; the `/api/standalone/*` routes exist only then.
    pub standalone: Option<Arc<standalone::StandaloneState>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "storm_codex_server=info,tower_http=warn".into()),
        )
        .init();

    // `migrate [--to <version>]`: migrations only, then exit (see `migrate_cmd`).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("migrate") {
        match migrate_cmd::run(&args[1..]).await {
            Ok(()) => std::process::exit(0),
            Err((code, msg)) => {
                eprintln!("{msg}");
                std::process::exit(code);
            }
        }
    }

    if let Err(e) = run().await {
        tracing::error!("startup failed: {e}");
        std::process::exit(1);
    }
}

/// Copies the baked minimaps (`bundle`, e.g. `assets/minimaps`) into `images_dir/minimaps`
/// (idempotent: skips the files already present). Then served on `/images/minimaps` by the
/// existing ServeDir. Best-effort: an error does not block startup (front fallback on the painted art).
fn bundle_minimaps(bundle: &std::path::Path, images_dir: &std::path::Path) {
    let dst = images_dir.join("minimaps");
    if std::fs::create_dir_all(&dst).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(bundle) else {
        return; // bundle missing (dev without assets) → no minimaps, front fallback
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
        tracing::info!("baked minimaps: {copied} copied into {}", dst.display());
    }
}

async fn run() -> Result<(), String> {
    let cfg = Config::from_env()?;
    cfg.validate()?;
    std::fs::create_dir_all(&cfg.archive_dir).map_err(|e| format!("archive_dir: {e}"))?;
    std::fs::create_dir_all(&cfg.raw_cache_dir).map_err(|e| format!("raw_cache_dir: {e}"))?;
    bundle_minimaps(&cfg.minimaps_bundle_dir, &cfg.images_dir);

    let db = PgPoolOptions::new()
        .max_connections(16)
        .connect(&cfg.database_url)
        .await
        .map_err(|e| format!("Postgres connection: {e}"))?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .map_err(|e| format!("migrations: {e}"))?;

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let (events, _) = broadcast::channel(1024);
    // Draft state: restored from disk if present, otherwise fresh (Standard, blue first-pick, Sky Temple).
    let mut draft = draft::store::load(&db).await.unwrap_or_else(|| {
        draft::DraftState::new(
            draft::Format::Standard,
            draft::Side::Blue,
            "Sky Temple".into(),
        )
    });
    // No timer after a restart: auto playback restarts paused (cursor kept).
    draft.after_restart();
    // Cast series: restored from disk if readable, otherwise a fresh inactive state (never a startup failure).
    let cast_initial = cast::store::load(&db)
        .await
        .unwrap_or_else(cast::Series::new);
    let lobby_initial = lobby::store::load(&db).await;
    let standalone = cfg
        .standalone
        .then(|| Arc::new(standalone::StandaloneState::new(&cfg)));
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
        standalone,
    };

    // Hero/talent/patch reference data + images (best-effort, 24 h refresh; each new patch →
    // in-app WS notification + optional webhook). Two mutually exclusive sources:
    //  - REFERENTIAL_URL  : published snapshot (`referential.tar.gz`) → standalone bundle, without live HPN.
    //  - HOTSPATCHNOTES_URL : live HotsPatchNotes API (maintainer setup).
    // The snapshot takes precedence when both are set.
    // Standalone mode: no network and no 24 h refresh loop; the import task (started once the
    // listener is bound) seeds an empty `dim_heroes` from the bundled snapshot first.
    if !state.cfg.standalone
        && (state.cfg.referential_url.is_some() || state.cfg.hotspatchnotes_url.is_some())
    {
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let new_patches = if let Some(url) = st.cfg.referential_url.clone() {
                    dim::ingest_snapshot(&st.db, &st.cfg.images_dir, &url).await
                } else if let Some(url) = st.cfg.hotspatchnotes_url.clone() {
                    // one-time sync of heroes/talents/images on the 1st round (idempotent afterwards).
                    dim::sync_heroes(&st.db, &url).await;
                    dim::sync_talents(&st.db, &url).await;
                    dim::vendor_images(&st.cfg.images_dir, &url).await;
                    let np = dim::sync_patches(&st.db, &url).await;
                    dim::backfill_hero_sections(&st.db, &url).await; // projects the hero sections (live)
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
                // Reference data still empty (source unreachable at startup, e.g. box off):
                // new attempt in 10 min rather than 24 h.
                let vide =
                    sqlx::query_scalar::<_, bool>("SELECT NOT EXISTS (SELECT 1 FROM dim_heroes)")
                        .fetch_one(&st.db)
                        .await
                        .unwrap_or(true);
                let pause = if vide { 600 } else { 24 * 3600 };
                tokio::time::sleep(std::time::Duration::from_secs(pause)).await;
            }
        });
    }
    match upload::resume_pending(&state).await {
        Ok(0) => {}
        Ok(n) => tracing::info!("{n} pending upload(s) resumed at startup"),
        Err(e) => tracing::error!("resuming the pending uploads: {e}"),
    }
    let bind = state.cfg.bind_addr.clone();
    let standalone_state = state.clone();
    let app = api_router(&state);
    // the front-end: bundle files under `/v2/<bundle>/`, every page at `/` (API routes first)
    let app = match state.cfg.web_dir.as_deref() {
        Some(dir) => serve_v2_at_root(serve_v2(app, Some(dir)), dir),
        None => app,
    }
    .with_state(state)
    // Request body limit: the axum default (2 MB) rejected with 413 — BEFORE the handler, so
    // without a trace or an uploads row — the replays of long games (a 5v5 Braxis Holdout exceeds
    // 2 MB), making the uploader loop. 64 MB covers it widely (replays < ~10 MB).
    .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
    // gzip/brotli according to `Accept-Encoding` (API JSON, JS/CSS bundle); the default predicate
    // skips small responses, images and streams (WS: 101 response without a body).
    .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("bind {bind}: {e}"))?;
    serve(listener, app, &standalone_state, shutdown_signal()).await
}

/// How long the shutdown waits for the standalone import task to flush and stop.
const IMPORT_STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Serves `app` until `signal` (or, in standalone mode, `POST /api/standalone/shutdown`), then
/// shuts down gracefully. Standalone: the import task is started first and uploads to this very
/// server at the address actually bound (`BIND_ADDR` may ask for port 0; `/api/health` answers
/// during the whole import). On shutdown it is told to stop and awaited (bounded) while the
/// server still answers its in-flight uploads, so the ledger is flushed before this returns.
async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    state: &AppState,
    signal: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    let local = listener.local_addr().map_err(|e| format!("bind: {e}"))?;
    tracing::info!("storm-codex-server listening on {local}");
    let standalone = state.standalone.clone();
    let import = standalone.as_ref().map(|st| {
        st.set_port(local.port());
        standalone::spawn(state.clone(), format!("http://{local}"))
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let Some(st) = standalone else {
                return signal.await;
            };
            tokio::select! {
                () = signal => {},
                () = st.shutdown_requested() => {},
            }
            tracing::info!("shutdown requested: stopping the import task");
            st.stop_import();
            if let Some(mut import) = import {
                if tokio::time::timeout(IMPORT_STOP_TIMEOUT, &mut import)
                    .await
                    .is_err()
                {
                    tracing::warn!("import task did not stop in time, aborted");
                    import.abort();
                }
            }
        })
        .await
        .map_err(|e| format!("serve: {e}"))
}

/// Ctrl-C or SIGTERM (`docker stop`): the server stops accepting connections and lets
/// the ongoing requests finish. Unfinished parses stay `pending` and are resumed at the
/// next startup (`upload::resume_pending`).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Ctrl-C signal: {e}");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!("SIGTERM signal: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
    tracing::info!("shutdown requested: finishing the ongoing requests");
}

/// All the API routes + images (without the SPA fallback) — extracted for the integration tests.
/// The `/api/standalone/*` routes are added only in standalone mode (404 otherwise).
fn api_router(state: &AppState) -> Router<AppState> {
    let router = base_router(state);
    if state.standalone.is_some() {
        router.merge(standalone::api::router(state))
    } else {
        router
    }
}

fn base_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/upload", post(upload::upload))
        // client-rs (Hots-Overlay) compat alias: it posts to /api/upload-raw (raw bytes,
        // X-Filename header, Bearer) — same handler, same guarantees.
        .route("/api/upload-raw", post(upload::upload))
        .route("/api/matches", get(read::list_matches))
        .route("/api/matches/{id}", get(read::get_match))
        .route("/api/matches/{id}/raw", get(raw::get_raw))
        .route("/api/matches/{id}/replay2d", get(replay2d::get_replay2d))
        .route("/api/players/search", get(players::api::search))
        .route("/api/players/{toon}", get(read::get_player))
        .route("/api/players/{toon}/identity", get(players::api::identity))
        .route("/api/players/{toon}/chat", get(players::api::chat))
        .route(
            "/api/players/{toon}/links/{other}",
            axum::routing::put(players::api::put_link),
        )
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
        .route(
            "/api/admin/settings",
            axum::routing::put(manage::put_settings),
        )
        .route(
            "/api/admin/minimap-anchors",
            axum::routing::put(manage::put_minimap_anchors),
        )
        .route(
            "/api/teams",
            get(manage::list_teams).post(manage::create_team),
        )
        .route(
            "/api/teams/{id}",
            axum::routing::delete(manage::delete_team).put(manage::update_team),
        )
        .route(
            "/api/collections",
            get(manage::list_collections).post(manage::create_collection),
        )
        .route(
            "/api/collections/{id}",
            axum::routing::delete(manage::delete_collection),
        )
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
        // Cast control room (server authoritative state + WS broadcast cast.updated), open mode
        .route("/api/cast", get(cast::api::get_cast))
        .route(
            "/api/cast/series",
            post(cast::api::start_series).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/teams",
            axum::routing::put(cast::api::put_teams).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/pool/map",
            post(cast::api::mark_map).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/games/start",
            post(cast::api::start_game).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/games/{n}/winner",
            post(cast::api::set_winner).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/games/{n}",
            axum::routing::delete(cast::api::remove_game),
        )
        .route(
            "/api/cast/games/{n}/replay",
            post(cast::replay::load_game_replay),
        )
        .route(
            "/api/rank/result",
            post(rank::post_result).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route("/api/cast/replays/inspect", post(cast::replay::inspect))
        .route(
            "/api/cast/replays/import",
            post(cast::replay::import_replay),
        )
        .route(
            "/api/cast/score",
            post(cast::api::set_score).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/scene",
            post(cast::api::set_scene).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/lineup-mode",
            post(cast::api::set_lineup_mode).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route(
            "/api/cast/slide",
            post(cast::api::set_slide).layer(axum::extract::DefaultBodyLimit::max(
                cast::api::JSON_BODY_LIMIT,
            )),
        )
        .route("/api/cast/lineup", get(cast::api::get_lineup))
        .route(
            "/api/cast/logo/{side}",
            post(cast::api::upload_logo)
                .layer(axum::extract::DefaultBodyLimit::max(
                    cast::api::LOGO_BODY_LIMIT,
                ))
                .get(cast::api::get_logo),
        )
        // Draft simulator (server authoritative state + WS broadcast draft.updated)
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
        .route(
            "/api/draft/replay/fearless",
            post(draft::replay_api::fearless),
        )
        .route("/api/draft/replay/play", post(draft::replay_api::play))
        .route(
            "/api/draft/replay/queue/move",
            post(draft::replay_api::move_game),
        )
        .route(
            "/api/draft/replay/queue/{i}",
            axum::routing::delete(draft::replay_api::remove),
        )
        // Live lobby (companion): detection during the loading screen + WS broadcast lobby.detected
        .route(
            "/api/lobby",
            get(lobby::api::get)
                .post(lobby::api::ingest)
                .delete(lobby::api::clear),
        )
        .route("/api/lobby/hero", post(lobby::api::set_hero))
        .route("/api/lobby/map", post(lobby::api::set_map))
        .route("/api/lobby/teams", post(lobby::api::set_teams))
        // Live feed of a game in progress (TempWriteReplayP1 blocks) + WS broadcast live.updated
        .route("/api/live", get(live::api::get).delete(live::api::clear))
        .route("/api/live/chunk", post(live::api::chunk))
        .route("/api/live/end", post(live::api::end))
        // Scouting reports (dedicated tables, never projected into matches)
        .route(
            "/api/scouting",
            get(scouting::api::list).post(scouting::api::create),
        )
        .route(
            "/api/scouting/{id}",
            get(scouting::api::get)
                .patch(scouting::api::patch)
                .delete(scouting::api::delete),
        )
        .route(
            "/api/scouting/{id}/replays",
            post(scouting::api::add_replay),
        )
        .route(
            "/api/scouting/{id}/replays/{gid}",
            axum::routing::patch(scouting::api::set_side).delete(scouting::api::delete_replay),
        )
        .route("/api/scouting/{id}/pack.md", get(scouting::api::pack_md))
        .route(
            "/api/scouting/{id}/pack.xlsx",
            get(scouting::api::pack_xlsx),
        )
        .route(
            "/api/scouting/{id}/analysis",
            axum::routing::put(scouting::api::put_analysis).delete(scouting::api::delete_analysis),
        )
        .route("/ws", any(ws::ws_handler))
        // vendored hero portraits + map images (served from images_dir)
        .nest_service(
            "/images",
            tower_http::services::ServeDir::new(&state.cfg.images_dir),
        )
        // cast/draft writes: admin Bearer required when ADMIN_TOKEN is set
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::guard_writes,
        ))
}

/// Page of the Rust front-end for a path under `/v2` (without the prefix). The OBS sources are served by two small
/// bundles, each kept under the 300 kB size limit: `overlay/` (cast and draft overlays) and `sources/` (session
/// and music sources); everything else by the app (`app/`).
fn v2_page(path: &str) -> &'static str {
    match path.trim_end_matches('/') {
        "/cast/overlay" | "/draft/overlay" => "overlay/index.html",
        "/ticker" | "/now-playing" | "/widget" | "/queue" => "sources/index.html",
        _ => "app/index.html",
    }
}

/// Handler answering the page of `v2_page` for a path (status 200, `no-cache`, read at each request so a rebuild is
/// picked up without restarting the server); a path that looks like a file answers 404.
fn v2_page_handler(
    dir: &std::path::Path,
) -> axum::routing::MethodRouter<(), std::convert::Infallible> {
    let root = dir.to_path_buf();
    axum::routing::get(move |uri: axum::http::Uri| {
        let root = root.clone();
        async move {
            use axum::response::IntoResponse;
            let path = uri.path().strip_prefix("/v2").unwrap_or(uri.path());
            if path.rsplit('/').next().is_some_and(|s| s.contains('.')) {
                return StatusCode::NOT_FOUND.into_response();
            }
            match tokio::fs::read_to_string(root.join(v2_page(path))).await {
                Ok(html) => (
                    [(axum::http::header::CACHE_CONTROL, "no-cache")],
                    axum::response::Html(html),
                )
                    .into_response(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    })
}

/// The switch: the Rust front-end answers every page at `/` (the API routes come first; the bundles' files stay under
/// `/v2/`, where `serve_v2` serves them).
fn serve_v2_at_root<S: Clone + Send + Sync + 'static>(
    app: Router<S>,
    dir: &std::path::Path,
) -> Router<S> {
    app.fallback_service(v2_page_handler(dir).with_state(()))
}

/// Rust front-end under `/v2/` (one Trunk output per bundle: `overlay/` at `/v2/overlay/`, `sources/` at
/// `/v2/sources/`, `app/` at `/v2/app/`): files are fingerprinted (cacheable for a
/// year); a path that looks like a file and does not exist answers 404 (never an HTML page instead of a
/// `.wasm`); any other path returns the page of `v2_page` (status 200, `no-cache`, read at each request so a
/// rebuild is picked up without restarting the server). React keeps `/`.
fn serve_v2<S: Clone + Send + Sync + 'static>(
    app: Router<S>,
    dir: Option<&std::path::Path>,
) -> Router<S> {
    let Some(dir) = dir else { return app };
    let page = v2_page_handler(dir);
    let files = tower_http::set_header::SetResponseHeader::if_not_present(
        tower_http::services::ServeDir::new(dir)
            .append_index_html_on_directories(false)
            .fallback(page),
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    app.nest_service("/v2", files)
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

/// App state for tests around a given config and pool (no draft/cast/lobby restored).
#[cfg(test)]
pub(crate) fn test_app_state(cfg: Config, db: PgPool) -> AppState {
    let (events, _) = tokio::sync::broadcast::channel(64);
    AppState {
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
        standalone: None,
    }
}

/// Router-level integration tests (tower::oneshot, without network listening). Skipped without
/// `DATABASE_URL` (pattern of the projection test) — run with the dev Docker Postgres,
/// run in CI against a Postgres service.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod front_end_tests {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::util::ServiceExt;

    /// `/v2`: fingerprinted files cacheable, cast/draft overlay routes on `overlay/index.html`, session/music sources on
    /// `sources/index.html`, other routes on `app/index.html`,
    /// a missing file 404, never an HTML page instead of a `.wasm`.
    #[tokio::test]
    async fn v2_serves_app_and_overlay_pages() {
        let dir = std::env::temp_dir().join(format!("storm-codex-v2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::create_dir_all(dir.join("overlay")).unwrap();
        std::fs::create_dir_all(dir.join("sources")).unwrap();
        std::fs::write(dir.join("app/index.html"), "<html>app</html>").unwrap();
        std::fs::write(dir.join("overlay/index.html"), "<html>overlay</html>").unwrap();
        std::fs::write(dir.join("sources/index.html"), "<html>sources</html>").unwrap();
        std::fs::write(dir.join("overlay/a-123.wasm"), "wasm").unwrap();
        let app = super::serve_v2(axum::Router::<()>::new(), Some(&dir));
        let get = |path: &'static str| {
            let app = app.clone();
            async move {
                let r = app
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = r.status();
                let cache = r
                    .headers()
                    .get(header::CACHE_CONTROL)
                    .map(|v| v.to_str().unwrap().to_string());
                let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
                (status, cache, String::from_utf8_lossy(&body).to_string())
            }
        };
        let (st, cache, _) = get("/v2/overlay/a-123.wasm").await;
        assert_eq!(
            (st, cache.as_deref()),
            (StatusCode::OK, Some("public, max-age=31536000, immutable"))
        );
        assert_eq!(
            get("/v2/overlay/absent.wasm").await.0,
            StatusCode::NOT_FOUND
        );
        for (path, page) in [
            ("/v2", "app"),
            ("/v2/", "app"),
            ("/v2/matches/12", "app"),
            ("/v2/cast/overlay", "overlay"),
            ("/v2/draft/overlay", "overlay"),
            ("/v2/ticker/", "sources"),
            ("/v2/now-playing", "sources"),
            ("/v2/widget", "sources"),
            ("/v2/queue", "sources"),
        ] {
            let (st, cache, body) = get(path).await;
            assert_eq!(st, StatusCode::OK, "{path}");
            assert_eq!(cache.as_deref(), Some("no-cache"), "{path}");
            assert!(body.contains(page), "{path}: {body}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `WEB_ROOT=rust`: every page answers at `/` with the same page choice as under `/v2`, the API routes stay first,
    /// the bundles' files stay under `/v2/`, and a file-looking path that does not exist is a 404.
    #[tokio::test]
    async fn the_rust_front_end_can_answer_at_the_root() {
        let dir = std::env::temp_dir().join(format!("storm-codex-root-{}", uuid::Uuid::new_v4()));
        for d in ["app", "overlay", "sources"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
            std::fs::write(dir.join(d).join("index.html"), format!("<html>{d}</html>")).unwrap();
        }
        std::fs::write(dir.join("app/a-1.wasm"), "wasm").unwrap();
        let api =
            axum::Router::<()>::new().route("/api/health", axum::routing::get(|| async { "ok" }));
        let app = super::serve_v2_at_root(super::serve_v2(api, Some(&dir)), &dir);
        let get = |path: &'static str| {
            let app = app.clone();
            async move {
                let r = app
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = r.status();
                let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
                (status, String::from_utf8_lossy(&body).to_string())
            }
        };
        for (path, page) in [
            ("/", "app"),
            ("/matches/12", "app"),
            ("/cast", "app"),
            ("/cast/overlay", "overlay"),
            ("/draft/overlay", "overlay"),
            ("/ticker", "sources"),
            ("/widget", "sources"),
            ("/v2/matches", "app"),
        ] {
            let (st, body) = get(path).await;
            assert_eq!(st, StatusCode::OK, "{path}");
            assert!(body.contains(page), "{path}: {body}");
        }
        assert_eq!(get("/api/health").await, (StatusCode::OK, "ok".to_string()));
        assert_eq!(get("/v2/app/a-1.wasm").await.0, StatusCode::OK);
        assert_eq!(get("/nothing.js").await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

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
            eprintln!("DATABASE_URL missing → API integration test skipped");
            return None;
        };
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .expect("connexion");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("migrations");
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
            standalone: false,
            referential_file: None,
            ledger_file: None,
            images_dir: dir.join("images"),
            minimaps_bundle_dir: dir.join("minimaps"),
            orpheus_url: None,
            patch_webhook_url: None,
        };
        std::fs::create_dir_all(&cfg.archive_dir).unwrap();
        std::fs::create_dir_all(&cfg.raw_cache_dir).unwrap();
        Some(super::test_app_state(cfg, db))
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
        let Some(state) = test_state(None).await else {
            return;
        };
        let resp = app(&state)
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["status"], "ok");
        assert_eq!(v["parser_version"], PARSER_VERSION);
    }

    /// `name` = current nickname from the dated history (not `players.last_name`, overwritten in
    /// processing order); `names` most recent first; the historical fields of the summary stay present.
    #[tokio::test]
    async fn get_player_pseudo_actuel_vient_de_lhistorique_date() {
        use crate::players::testutil::{insert_match, row, uid};
        use chrono::TimeZone;
        let Some(state) = test_state(None).await else {
            return;
        };
        let (toon, u) = (format!("t-{}", uid()), uid());
        let (old, new) = (format!("Old{u}"), format!("New{u}"));
        let t = |m| chrono::Utc.with_ymd_and_hms(2025, m, 1, 20, 0, 0).unwrap();
        insert_match(&state.db, Some(t(9)), &[row(&toon, &new, 0, "Muradin")]).await;
        insert_match(&state.db, Some(t(1)), &[row(&toon, &old, 0, "Muradin")]).await; // processed last
        let resp = app(&state)
            .oneshot(
                Request::get(format!("/api/players/{toon}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["name"], new.as_str());
        assert_eq!(v["names"], serde_json::json!([new, old]));
        for k in ["toon", "matches", "wins", "avg_kills", "heroes", "recent"] {
            assert!(v.get(k).is_some(), "field {k} missing");
        }
    }

    /// `/api/players/search` (static) must not fall into `/api/players/{toon}`; short
    /// query → empty list; literal `_`; `limit` capped.
    #[tokio::test]
    async fn players_search_route_dediee_et_litterale() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else {
            return;
        };
        let (toon, u) = (format!("t-{}", uid()), uid());
        insert_match(
            &state.db,
            None,
            &[row(&toon, &format!("Kerr{u}_Main"), 0, "Muradin")],
        )
        .await;
        let get = |uri: String| {
            let st = state.clone();
            async move {
                app(&st)
                    .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
            }
        };
        let resp = get(format!("/api/players/search?q=Kerr{u}_M")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert!(v["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["toon"] == toon.as_str()));
        assert!(v.get("toon").is_none(), "fell into get_player");
        let v = json_body(get("/api/players/search?q=a".into()).await).await;
        assert_eq!(v["hits"], serde_json::json!([]));
        assert_eq!(
            get("/api/players/search?q=ab&limit=9999".into())
                .await
                .status(),
            StatusCode::OK
        );
    }

    async fn put_link(
        st: &AppState,
        a: &str,
        b: &str,
        status: &str,
        bearer: Option<&str>,
    ) -> axum::response::Response {
        let mut req = Request::put(format!("/api/players/{a}/links/{b}"))
            .header("content-type", "application/json");
        if let Some(t) = bearer {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        app(st)
            .oneshot(
                req.body(Body::from(format!(r#"{{"status":"{status}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn get_identity(st: &AppState, toon: &str) -> axum::response::Response {
        app(st)
            .oneshot(
                Request::get(format!("/api/players/{toon}/identity"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn identity_inconnu_404_et_put_soi_meme_400() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else {
            return;
        };
        let resp = get_identity(&state, &format!("t-inconnu-{}", uid())).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(
            json_body(resp).await["error"].is_string(),
            "application 404, not a missing route"
        );
        let toon = format!("t-{}", uid());
        insert_match(&state.db, None, &[row(&toon, "Solo", 0, "Muradin")]).await;
        assert_eq!(
            put_link(&state, &toon, &toon, "confirmed", None)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn put_link_garde_admin_statut_valide_et_inconnu() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(Some("sekret")).await else {
            return;
        };
        let (a, b) = (format!("t-a{}", uid()), format!("t-b{}", uid()));
        insert_match(
            &state.db,
            None,
            &[row(&a, "A", 0, "Muradin"), row(&b, "B", 1, "Muradin")],
        )
        .await;
        assert_eq!(
            put_link(&state, &a, &b, "confirmed", None).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            put_link(&state, &a, &b, "confirmed", Some("faux"))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            put_link(&state, &a, &b, "confirmed", Some("sekret"))
                .await
                .status(),
            StatusCode::OK
        );
        let bad = put_link(&state, &a, &b, "maybe", Some("sekret"))
            .await
            .status();
        assert!(bad.is_client_error(), "invalid status refused, got {bad}");
        let ghost = format!("t-ghost-{}", uid());
        assert_eq!(
            put_link(&state, &a, &ghost, "confirmed", Some("sekret"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn put_link_est_symetrique_et_idempotent() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else {
            return;
        };
        let (a, b) = (format!("t-a{}", uid()), format!("t-b{}", uid()));
        insert_match(
            &state.db,
            None,
            &[
                row(&a, "NameA", 0, "Muradin"),
                row(&b, "NameB", 1, "Muradin"),
            ],
        )
        .await;
        assert_eq!(
            put_link(&state, &a, &b, "confirmed", None).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            put_link(&state, &b, &a, "confirmed", None).await.status(),
            StatusCode::OK
        );
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM player_links WHERE toon_a IN ($1,$2) AND toon_b IN ($1,$2)",
        )
        .bind(&a)
        .bind(&b)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(n, 1, "A→B then B→A = a single row");
        for (me, other, other_name) in [(&a, &b, "NameB"), (&b, &a, "NameA")] {
            let v = json_body(get_identity(&state, me).await).await;
            let conf = v["confirmed"].as_array().unwrap();
            assert!(conf
                .iter()
                .any(|c| c["toon"] == other.as_str() && c["name"] == other_name));
        }
    }

    #[tokio::test]
    async fn identity_historique_suggestions_et_decision() {
        use crate::players::testutil::smurf_world;
        let Some(state) = test_state(None).await else {
            return;
        };
        let w = smurf_world(&state.db).await;
        let v = json_body(get_identity(&state, &w.target).await).await;
        assert_eq!(v["toon"], w.target.as_str());
        assert!(v["names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["name"] == "N" && n["games"] == 18));
        let s = v["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["toon"] == w.smurf.as_str())
            .expect("smurf suggested");
        assert_eq!(s["level"], "high");
        assert!(s["score"].as_u64().unwrap() >= 50);
        assert!(!s["reasons"].as_array().unwrap().is_empty());
        // dismiss → no longer offered, from the target as from the smurf
        assert_eq!(
            put_link(&state, &w.target, &w.smurf, "dismissed", None)
                .await
                .status(),
            StatusCode::OK
        );
        for me in [&w.target, &w.smurf] {
            let v = json_body(get_identity(&state, me).await).await;
            let other = if me == &w.target { &w.smurf } else { &w.target };
            assert!(
                v["suggestions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|s| s["toon"] != other.as_str()),
                "dismissed but offered again"
            );
            assert!(v["confirmed"].as_array().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn apm_par_partie_et_moyenne_par_joueur() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else {
            return;
        };
        let u = uid();
        let (a, b) = (format!("t-pa{u}"), format!("t-pb{u}"));
        let m1 = insert_match(
            &state.db,
            None,
            &[row(&a, "A", 0, "Muradin"), row(&b, "B", 1, "Diablo")],
        )
        .await;
        let m2 = insert_match(
            &state.db,
            None,
            &[row(&a, "A", 0, "Muradin"), row(&b, "B", 1, "Diablo")],
        )
        .await;
        let m3 = insert_match(&state.db, None, &[row(&a, "A", 0, "Muradin")]).await;
        // A: 100 then 140 APM, plus a game from before the re-process (NULL, outside the average); B: 60
        for (m, toon, apm) in [
            (m1, &a, Some(100.0)),
            (m2, &a, Some(140.0)),
            (m3, &a, None),
            (m1, &b, Some(60.0)),
        ] {
            sqlx::query(
                "UPDATE match_players SET apm = $3 WHERE match_id = $1 AND toon_handle = $2",
            )
            .bind(m)
            .bind(toon)
            .bind(apm)
            .execute(&state.db)
            .await
            .unwrap();
        }
        let get = |uri: String| {
            let st = state.clone();
            async move {
                json_body(
                    app(&st)
                        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                        .await
                        .unwrap(),
                )
                .await
            }
        };
        let va = get(format!("/api/players/{a}")).await;
        assert_eq!(va["avg_apm"], 120, "average of the measured games only");
        assert_eq!(va["apm_games"], 2);
        let recent = va["recent"].as_array().unwrap();
        assert!(recent
            .iter()
            .any(|g| g["match_id"] == m1 && g["apm"] == 100));
        assert!(recent
            .iter()
            .any(|g| g["match_id"] == m3 && g["apm"].is_null()));
        let vb = get(format!("/api/players/{b}")).await;
        assert_eq!(vb["avg_apm"], 60);
        let md = get(format!("/api/matches/{m1}")).await;
        assert_eq!(md["players"][&a]["apm"], 100.0);
        assert_eq!(md["players"][&b]["apm"], 60.0);
    }

    #[tokio::test]
    async fn chat_habitudes_comptees_pour_le_bon_joueur() {
        use crate::players::testutil::{insert_match, row, uid};
        let Some(state) = test_state(None).await else {
            return;
        };
        let u = uid();
        let (a, b) = (format!("t-ca{u}"), format!("t-cb{u}"));
        let id = insert_match(
            &state.db,
            None,
            &[row(&a, "A", 0, "Muradin"), row(&b, "B", 1, "Diablo")],
        )
        .await;
        let msg = |player: &str, ty: i64, text: &str| serde_json::json!({"type": ty, "player": player, "recipient": 1, "time": 60.0, "text": text});
        let messages = serde_json::json!([
            msg(&a, 0, "noob"),
            msg(&a, 0, "noob"),
            msg(&a, 0, "noob"),
            msg(&b, 0, "salut"),
            msg(&a, 4, "annonce")
        ]);
        sqlx::query("UPDATE matches SET data = jsonb_build_object('messages', $2::jsonb), length = 600 WHERE id = $1")
            .bind(id)
            .bind(messages)
            .execute(&state.db)
            .await
            .unwrap();
        let get = |toon: String| {
            let st = state.clone();
            async move {
                app(&st)
                    .oneshot(
                        Request::get(format!("/api/players/{toon}/chat"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            }
        };
        let va = json_body(get(a.clone()).await).await;
        assert_eq!(va["messages"], 3);
        assert_eq!(
            va["top_words"],
            serde_json::json!([{"word": "noob", "count": 3}])
        );
        assert!(!va.to_string().contains("annonce"), "no text exposed");
        assert!(va.get("text").is_none() && !va.to_string().contains("salut"));
        // another player's messages, or a too rare word: never in A's response
        assert!(va["top_words"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w["word"] != "salut"));
        let vb = json_body(get(b.clone()).await).await;
        assert_eq!(vb["messages"], 1);
        assert_eq!(
            get(format!("inconnu-{u}")).await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn chat_fenetre_bornee_aux_parties_recentes() {
        use crate::players::testutil::{insert_match, row, uid};
        use chrono::{Duration, TimeZone, Utc};
        let Some(state) = test_state(None).await else {
            return;
        };
        let (u, a) = (uid(), format!("t-cw{}", uid()));
        let t0 = Utc.with_ymd_and_hms(2025, 5, 1, 20, 0, 0).unwrap();
        for d in 0..3 {
            let id = insert_match(
                &state.db,
                Some(t0 + Duration::days(d)),
                &[row(&a, "A", 0, "Muradin")],
            )
            .await;
            let m = serde_json::json!([{"type": 0, "player": a, "recipient": 1, "time": 60.0, "text": format!("m{u}{d}")}]);
            sqlx::query("UPDATE matches SET data = jsonb_build_object('messages', $2::jsonb), length = 600 WHERE id = $1")
                .bind(id)
                .bind(m)
                .execute(&state.db)
                .await
                .unwrap();
        }
        let (games, rows) = crate::players::chat::load_rows_with(&state.db, &a, 2)
            .await
            .unwrap();
        assert_eq!(games, 2, "games = window size");
        assert_eq!(rows.len(), 2, "messages of the 2 most recent games only");
        let texts: Vec<_> = rows.iter().filter_map(|r| r.text.as_deref()).collect();
        assert!(
            !texts.iter().any(|t| t.ends_with('0')),
            "the oldest is excluded"
        );
    }

    /// Ban attribute codes → parser names (attr.json), including the upper-case codes.
    #[tokio::test]
    async fn dim_hero_attributes_resout_les_codes_de_ban() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let resp = app(&state)
            .oneshot(
                Request::get("/api/dim/hero-attributes")
                    .body(Body::empty())
                    .unwrap(),
            )
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
        let Some(state) = test_state(Some("s3cret")).await else {
            return;
        };
        // without a token → 401
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
        // with the right Bearer → created
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

    /// ADMIN_TOKEN set: the cast/draft writes require the Bearer, the reads stay free
    /// (OBS overlays without a token). Invalid body with the Bearer → 4xx from the handler (the guard is
    /// passed) without mutating the shared state.
    #[tokio::test]
    async fn ecritures_cast_draft_gardees_quand_admin_token() {
        let Some(state) = test_state(Some("s3cret")).await else {
            return;
        };
        for path in ["/api/cast/scene", "/api/draft/undo"] {
            let resp = app(&state)
                .oneshot(
                    Request::post(path)
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "{path} without a token"
            );
        }
        let resp = app(&state)
            .oneshot(
                Request::post("/api/cast/scene")
                    .header("authorization", "Bearer s3cret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"scene":"pas-une-scene"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            resp.status().is_client_error() && resp.status() != StatusCode::UNAUTHORIZED,
            "statut {}",
            resp.status()
        );
        let resp = app(&state)
            .oneshot(Request::get("/api/cast").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// End-to-end scouting: creation → replay drop → ambiguous batch (1 game: the 10
    /// players at 100%) → anchor → side found → facts → .md/.xlsx pack → analysis import →
    /// overwrite. And the isolation: nothing is written into uploads/matches for this replay.
    #[tokio::test]
    async fn scouting_cycle_complet_et_isolation() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("committed replay");
        let sha = crate::upload::sha256_hex(&bytes);
        let out = storm_stats::process_replay(&replay, "x");
        let fp = crate::upload::game_fingerprint(&out).unwrap();
        let counts = |db: sqlx::PgPool, sha: String, fp: String| async move {
            let u: i64 = sqlx::query_scalar("SELECT count(*) FROM uploads WHERE fingerprint = $1")
                .bind(&sha)
                .fetch_one(&db)
                .await
                .unwrap();
            let m: i64 = sqlx::query_scalar("SELECT count(*) FROM matches WHERE fingerprint = $1")
                .bind(&fp)
                .fetch_one(&db)
                .await
                .unwrap();
            (u, m)
        };
        let before = counts(state.db.clone(), sha.clone(), fp.clone()).await;

        let req = |method: &str, uri: String, body: Body| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(body)
                .unwrap()
        };
        let resp = app(&state)
            .oneshot(req(
                "POST",
                "/api/scouting".into(),
                Body::from(r#"{"title":"Test scout","target_name":"Team X"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = json_body(resp).await["id"].as_i64().unwrap();

        let add = |b: Vec<u8>| {
            Request::post(format!("/api/scouting/{id}/replays"))
                .header(
                    "x-filename",
                    "2024-10-07 22.29.55 Industrial%20District.StormReplay",
                )
                .body(Body::from(b))
                .unwrap()
        };
        let v = json_body(app(&state).oneshot(add(bytes.clone())).await.unwrap()).await;
        assert_eq!(v["status"], "added", "{v}");
        assert!(
            v["target_team"].is_null(),
            "a one-game batch = ambiguous, no side: {v}"
        );
        let v = json_body(app(&state).oneshot(add(bytes.clone())).await.unwrap()).await;
        assert_eq!(v["status"], "duplicate");

        let detail = json_body(
            app(&state)
                .oneshot(req("GET", format!("/api/scouting/{id}"), Body::empty()))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(detail["snapshot"]["detection"]["ambiguous"], true);
        assert_eq!(detail["snapshot"]["facts"]["overview"]["excluded"], 1);
        assert_eq!(detail["status"], "ready");
        let anchor = detail["games"][0]["teams"][1][0]["toon"]
            .as_str()
            .unwrap()
            .to_owned();

        // anchor on side 1 → side found, roster = the 5 teammates, 1 game analysed
        let body = serde_json::json!({ "anchors": [anchor] }).to_string();
        let detail = json_body(
            app(&state)
                .oneshot(req(
                    "PATCH",
                    format!("/api/scouting/{id}"),
                    Body::from(body),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(detail["games"][0]["target_team"], 1, "{}", detail["games"]);
        assert_eq!(detail["games"][0]["target_source"], "roster");
        assert_eq!(detail["snapshot"]["roster"].as_array().unwrap().len(), 5);
        assert_eq!(detail["snapshot"]["facts"]["overview"]["games"], 1);
        let fv = detail["facts_version"].as_i64().unwrap();

        // manual side then back to automatic
        let body = r#"{"target_team":0}"#.to_string();
        let gid = detail["games"][0]["gid"].as_i64().unwrap();
        let d2 = json_body(
            app(&state)
                .oneshot(req(
                    "PATCH",
                    format!("/api/scouting/{id}/replays/{gid}"),
                    Body::from(body),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(d2["games"][0]["target_source"], "manual");
        let d2 = json_body(
            app(&state)
                .oneshot(req(
                    "PATCH",
                    format!("/api/scouting/{id}/replays/{gid}"),
                    Body::from(r#"{"target_team":null}"#),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(d2["games"][0]["target_team"], 1);
        let fv = d2["facts_version"].as_i64().unwrap().max(fv);

        // packs
        let resp = app(&state)
            .oneshot(req(
                "GET",
                format!("/api/scouting/{id}/pack.md"),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let md = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(md.contains(&format!("report_id: **{id}**")));
        assert!(md.contains("`map.industrial_district.record`"));
        let resp = app(&state)
            .oneshot(req(
                "GET",
                format!("/api/scouting/{id}/pack.xlsx"),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let x = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&x[..2], b"PK");

        // import: broken JSON → 422 and nothing written; valid → imported; re-import → overwritten
        let resp = app(&state)
            .oneshot(req(
                "PUT",
                format!("/api/scouting/{id}/analysis"),
                Body::from("```json\n{bad}\n```"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // v2 draft plan: 1 card citing a real fact + an unknown id, 1 consideration without evidence
        let analysis = |summary: &str| {
            format!(
            "Here you go:\n```json\n{{\"format_version\":2,\"report_id\":{id},\"facts_version\":{fv},\"summary\":\"{summary}\",\"maps\":[{{\"map\":\"Industrial District\",\"confidence\":\"low\",\"overview\":\"o\",\"evidence\":[\"map.industrial_district.record\",\"nope\"],\"considerations\":[{{\"point\":\"y\",\"evidence\":[]}}]}}]}}\n```")
        };
        let resp = app(&state)
            .oneshot(req(
                "PUT",
                format!("/api/scouting/{id}/analysis"),
                Body::from(analysis("first")),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["tally"]["claims"], 2, "{v}");
        assert_eq!(v["tally"]["unsupported"], 1);
        app(&state)
            .oneshot(req(
                "PUT",
                format!("/api/scouting/{id}/analysis"),
                Body::from(analysis("second")),
            ))
            .await
            .unwrap();
        let detail = json_body(
            app(&state)
                .oneshot(req("GET", format!("/api/scouting/{id}"), Body::empty()))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(detail["analysis"]["summary"], "second");
        assert_eq!(detail["status"], "analyzed");
        assert_eq!(detail["analysis"]["maps"][0]["evidence"][0]["known"], true);
        assert_eq!(detail["analysis"]["maps"][0]["evidence"][1]["known"], false);

        // isolation: no write into uploads/matches for this replay
        assert_eq!(counts(state.db.clone(), sha, fp).await, before);

        let resp = app(&state)
            .oneshot(req("DELETE", format!("/api/scouting/{id}"), Body::empty()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// Real end to end: token creation → upload of a committed replay → parse → projection
    /// → reading the detail (with `match.messages`) → 409 dedup on re-upload.
    #[tokio::test]
    async fn upload_parse_lecture_et_dedup() {
        let Some(state) = test_state(None).await else {
            return;
        };
        // upload token (open admin mode)
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

        // upload of the reference replay (client-rs compat alias)
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("committed replay");
        // purge of a possible previous run (test idempotence)
        let hash = crate::upload::sha256_hex(&bytes);
        sqlx::query("DELETE FROM uploads WHERE fingerprint = $1")
            .bind(&hash)
            .execute(&state.db)
            .await
            .unwrap();
        let upload_req = |b: Vec<u8>, tok: &str| {
            Request::post("/api/upload-raw")
                .header("authorization", format!("Bearer {tok}"))
                .header(
                    "x-filename",
                    "2026-06-09 20.35.02 Industrial%20District.StormReplay",
                )
                .body(Body::from(b))
                .unwrap()
        };
        let resp = app(&state)
            .oneshot(upload_req(bytes.clone(), &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert_eq!(v["status"], "parsed", "response: {v}");
        let match_id = v["match_id"].as_i64().expect("match_id");

        // reading the detail: {match, players} shape, messages present (chat + pings)
        let resp = app(&state)
            .oneshot(
                Request::get(format!("/api/matches/{match_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let detail = json_body(resp).await;
        assert_eq!(detail["match"]["map"], "Industrial District");
        assert_eq!(detail["players"].as_object().unwrap().len(), 10);
        assert!(
            detail["match"]["messages"]
                .as_array()
                .is_some_and(|m| !m.is_empty()),
            "messages missing from the detail"
        );

        // re-upload identique → 409 duplicate
        let resp = app(&state)
            .oneshot(upload_req(bytes, &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // without a token → 401
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

    // ── draft from a replay ──────────────────────────────────────────────────

    async fn with_game(state: &AppState) {
        let fx: serde_json::Value =
            serde_json::from_str(include_str!("draft/fixtures/custom_draft.json")).unwrap();
        let out = storm_stats::Output {
            hotkey_profiles: Default::default(),
            actions: Default::default(),
            status: 1,
            match_: fx["match"].as_object().cloned(),
            players: fx["players"].as_object().cloned(),
        };
        let g = draft::replay_extract::loaded_game_from_output(
            &out,
            "fx.StormReplay",
            storm_stats::constants::hero_attribute(),
        )
        .unwrap();
        state.draft.write().await.add_game(g);
    }

    /// Waits (at most 5 s) for the draft state to satisfy `ok`, and returns the observed cursor.
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

    pub(crate) async fn call(
        state: &AppState,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Raw-body (bytes) request; returns status + response bytes.
    pub(crate) async fn call_raw(
        state: &AppState,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, Vec<u8>) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        let resp = app(state)
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        (
            status,
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    }

    #[tokio::test]
    async fn replay_load_rejects_empty_body() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let resp = app(&state)
            .oneshot(
                Request::post("/api/draft/replay/load")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn replay_load_rejects_non_custom_and_garbage_and_queues_nothing() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let aram = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../storm-stats/tests/data/silver-city-aram.StormReplay"
        ))
        .unwrap();
        for bytes in [aram, vec![7u8; 64]] {
            let resp = app(&state)
                .oneshot(
                    Request::post("/api/draft/replay/load")
                        .body(Body::from(bytes))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }
        assert!(state.draft.read().await.replay.queue.is_empty());
    }

    #[tokio::test]
    async fn replay_next_back_errors_are_409_and_change_nothing() {
        let Some(state) = test_state(None).await else {
            return;
        };
        assert_eq!(
            call(&state, "POST", "/api/draft/replay/next", None).await.0,
            StatusCode::CONFLICT
        ); // no game
        with_game(&state).await;
        assert_eq!(
            call(&state, "POST", "/api/draft/replay/back", None).await.0,
            StatusCode::CONFLICT
        ); // at the start
        for _ in 0..12 {
            assert_eq!(
                call(&state, "POST", "/api/draft/replay/next", None).await.0,
                StatusCode::OK
            );
        }
        assert_eq!(
            call(&state, "POST", "/api/draft/replay/next", None).await.0,
            StatusCode::CONFLICT
        ); // finished
        assert_eq!(state.draft.read().await.cursor, 16);
    }

    #[tokio::test]
    async fn public_draft_hides_future_but_replay_route_has_script() {
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        call(&state, "POST", "/api/draft/replay/next", None).await;
        let (_, public) = call(&state, "GET", "/api/draft", None).await;
        assert!(public["replay"]["queue"][0]["script"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            public["assignments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| !a.is_null())
                .count(),
            1
        );
        let (_, full) = call(&state, "GET", "/api/draft/replay", None).await;
        assert_eq!(full["queue"][0]["script"].as_array().unwrap().len(), 16);
    }

    #[tokio::test]
    async fn replay_autoplay_ticks_then_pause_stops() {
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": true, "interval_ms": 20})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        // bounded wait instead of a fixed sleep: a loaded CI runner can delay the ticks
        let c = wait_until(&state, |d| d.cursor >= 2).await;
        assert!(c >= 2, "auto playback does not advance (cursor {c})");
        call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": false})),
        )
        .await;
        let paused = state.draft.read().await.cursor;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(state.draft.read().await.cursor, paused);
        // intervalle absurde → 400
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": true, "interval_ms": 1})),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn replay_autoplay_stops_itself_at_the_end() {
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": true, "interval_ms": 10})),
        )
        .await;
        wait_until(&state, |d| d.is_complete() && !d.replay.playing).await;
        let d = state.draft.read().await;
        assert!(d.is_complete() && !d.replay.playing);
    }

    #[tokio::test]
    async fn replay_manual_next_while_playing_never_double_reveals() {
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": true, "interval_ms": 60_000})),
        )
        .await;
        call(&state, "POST", "/api/draft/replay/next", None).await;
        // 1 phase = 1 ban step → cursor exactly 1, the 60 s timer has not fired
        assert_eq!(state.draft.read().await.cursor, 1);
        call(
            &state,
            "POST",
            "/api/draft/replay/play",
            Some(serde_json::json!({"playing": false})),
        )
        .await;
    }
}
