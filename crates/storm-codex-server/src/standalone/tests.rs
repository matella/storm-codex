//! Standalone mode: router tests (`tower::ServiceExt`, no listener) and DB-backed tests
//! (skipped without `DATABASE_URL`, like `api_tests`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::{config::Config, AppState};
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use storm_codex_watcher::backfill::{self, Progress};
use storm_codex_watcher::ledger::Ledger;
use storm_codex_watcher::upload::{UploadResult, Uploader};
use tower::util::ServiceExt;

fn test_config(dir: &Path) -> Config {
    Config {
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".into(),
        archive_dir: dir.join("archive"),
        raw_cache_dir: dir.join("raw-cache"),
        raw_cache_max_bytes: 64 * 1024 * 1024,
        admin_token: None,
        web_dir: None,
        redis_url: None,
        jarvis_channel: "test".into(),
        azure_push_url: None,
        azure_push_token: None,
        hotspatchnotes_url: None,
        referential_url: None,
        standalone: true,
        referential_file: None,
        ledger_file: None,
        images_dir: dir.join("images"),
        minimaps_bundle_dir: dir.join("minimaps"),
        orpheus_url: None,
        patch_webhook_url: None,
    }
}

/// App state whose pool never reaches a database (unroutable port, 1 s acquire timeout):
/// enough for the routes and orchestrator steps that do not need Postgres.
fn lazy_state(dir: &Path, standalone: Option<StandaloneState>) -> AppState {
    let cfg = test_config(dir);
    std::fs::create_dir_all(&cfg.archive_dir).unwrap();
    let db = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(1))
        .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none")
        .unwrap();
    let mut state = crate::test_app_state(cfg, db);
    state.standalone = standalone.map(Arc::new);
    state
}

fn app(state: &AppState) -> Router {
    crate::app_router(state.clone())
}

/// Sends `req` (with `Host: 127.0.0.1` unless the test set one).
async fn send(app: &Router, mut req: Request<Body>) -> (StatusCode, serde_json::Value) {
    if !req.headers().contains_key(header::HOST) {
        req.headers_mut()
            .insert(header::HOST, "127.0.0.1".parse().unwrap());
    }
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn get_status(app: &Router) -> serde_json::Value {
    let (code, v) = send(
        app,
        Request::get("/api/standalone/status")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    v
}

/// Polls the status until `ok` holds (panics after `secs`).
async fn wait_status(
    app: &Router,
    secs: u64,
    ok: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let v = get_status(app).await;
        if ok(&v) {
            return v;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "status never matched: {v}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn put_folders(folders: &[&str]) -> Request<Body> {
    Request::put("/api/standalone/folders")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "folders": folders }).to_string(),
        ))
        .unwrap()
}

/// A replay file whose mtime is old enough for the backfill (> 2 s).
fn old_replay(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    let f = std::fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(60))
        .unwrap();
    path
}

#[tokio::test]
async fn status_is_404_when_not_standalone() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&lazy_state(tmp.path(), None));
    for req in [
        Request::get("/api/standalone/status")
            .body(Body::empty())
            .unwrap(),
        put_folders(&[]),
        Request::post("/api/standalone/rescan")
            .body(Body::empty())
            .unwrap(),
        Request::post("/api/standalone/shutdown")
            .body(Body::empty())
            .unwrap(),
    ] {
        let path = req.uri().path().to_string();
        assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn status_reports_no_folders_phase_when_none_found() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path());
    let st = StandaloneState::new(&cfg).with_detector(Vec::new);
    let state = lazy_state(tmp.path(), Some(st));
    let app = app(&state);
    let _task = spawn(state.clone(), "http://127.0.0.1:1".into());
    let v = wait_status(&app, 10, |v| v["phase"] == "no-folders").await;
    assert_eq!(v["folders"], serde_json::json!([]));
    // no saved extra folder: an empty list, never missing
    assert_eq!(v["extraFolders"], serde_json::json!([]));
    assert_eq!(v["total"], 0);
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["dataDir"], tmp.path().to_string_lossy().as_ref());
    for key in [
        "done",
        "duplicates",
        "rejected",
        "failed",
        "freeBytes",
        "neededBytes",
    ] {
        assert!(v.get(key).is_some(), "missing {key}: {v}");
    }
}

#[test]
fn status_carries_the_saved_extra_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let st = StandaloneState::new(&test_config(tmp.path()));
    assert_eq!(st.status().extra_folders, Vec::<String>::new());
    st.set_folders(&[tmp.path().join("found")]);
    st.set_extra_folders(&["D:/Backup".to_string()]);
    let v = serde_json::to_value(st.status()).unwrap();
    assert_eq!(v["extraFolders"], serde_json::json!(["D:/Backup"]));
    // the effective list is separate: the saved extras are not mixed into it
    assert_eq!(
        v["folders"],
        serde_json::json!([tmp.path().join("found").to_string_lossy()])
    );
}

/// Stub `/api/upload`: `a*` uploaded, `b*` duplicate, `c*` rejected (400 "corrupt").
async fn stub_upload_server() -> String {
    use axum::http::HeaderMap;
    let route = axum::routing::post(|headers: HeaderMap| async move {
        let name = headers
            .get("x-filename")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        match name.chars().next() {
            Some('a') => (StatusCode::OK, "{}".to_string()),
            Some('b') => (StatusCode::CONFLICT, "{}".to_string()),
            _ => (StatusCode::BAD_REQUEST, "corrupt".to_string()),
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/api/upload", route)).await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn status_counts_follow_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    std::fs::create_dir_all(&replays).unwrap();
    for name in [
        "a1.StormReplay",
        "a2.StormReplay",
        "b1.StormReplay",
        "c1.StormReplay",
    ] {
        old_replay(&replays, name, name.as_bytes());
    }
    let base = stub_upload_server().await;
    let uploader = Uploader::new(base).with_backoff_unit(Duration::from_millis(1));
    let mut ledger = Ledger::open(&tmp.path().join("uploaded.json")).unwrap();
    let progress = Arc::new(Progress::default());

    let cfg = test_config(tmp.path());
    let st = StandaloneState::new(&cfg);
    st.start_run(progress.clone());
    let state = lazy_state(tmp.path(), Some(st));
    let app = app(&state);

    backfill::run(&[replays], &uploader, &mut ledger, &progress).await;
    let v = get_status(&app).await;
    assert_eq!(v["total"], 4);
    assert_eq!(v["done"], 4);
    assert_eq!(v["duplicates"], 1);
    assert_eq!(
        v["rejected"],
        serde_json::json!([{ "file": "c1.StormReplay", "reason": "corrupt" }])
    );
    assert_eq!(v["failed"], serde_json::json!([]));

    // live watcher outcomes add up with the backfill run
    let st = state.standalone.as_ref().unwrap();
    st.record_live("d1.StormReplay", &UploadResult::Duplicate);
    st.record_live("e1.StormReplay", &UploadResult::Retryable("down".into()));
    let v = get_status(&app).await;
    assert_eq!(v["total"], 6);
    assert_eq!(v["done"], 6);
    assert_eq!(v["duplicates"], 2);
    assert_eq!(v["failed"], serde_json::json!(["e1.StormReplay"]));
}

#[tokio::test]
async fn put_folders_rejects_what_is_not_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();
    let cfg = test_config(tmp.path());
    let state = lazy_state(tmp.path(), Some(StandaloneState::new(&cfg)));
    let app = app(&state);
    let missing = tmp.path().join("missing");
    for bad in [file.to_string_lossy(), missing.to_string_lossy(), "".into()] {
        let (code, v) = send(&app, put_folders(&[bad.as_ref()])).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
        assert!(
            v["error"]
                .as_str()
                .unwrap()
                .contains("not an existing folder"),
            "{v}"
        );
    }
}

#[tokio::test]
async fn rescan_is_accepted_and_queued() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path());
    let state = lazy_state(tmp.path(), Some(StandaloneState::new(&cfg)));
    let mut commands = state.standalone.as_ref().unwrap().take_commands().unwrap();
    let app = app(&state);
    let (code, _) = send(
        &app,
        Request::post("/api/standalone/rescan")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED);
    assert!(matches!(commands.try_recv(), Ok(Command::Rescan)));
}

#[tokio::test]
async fn shutdown_triggers_the_graceful_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path());
    let state = lazy_state(tmp.path(), Some(StandaloneState::new(&cfg)));
    let st = state.standalone.clone().unwrap();
    let app = app(&state);
    let (code, _) = send(
        &app,
        Request::post("/api/standalone/shutdown")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED);
    tokio::time::timeout(Duration::from_secs(1), st.shutdown_requested())
        .await
        .expect("shutdown notified");
}

#[tokio::test]
async fn foreign_host_headers_are_refused_on_every_route() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path());
    let state = lazy_state(tmp.path(), Some(StandaloneState::new(&cfg)));
    let st = state.standalone.clone().unwrap();
    st.set_port(8088);
    let app = app(&state);
    let routes = || {
        [
            (Method::GET, "/api/standalone/status"),
            (Method::HEAD, "/api/standalone/status"),
            (Method::PUT, "/api/standalone/folders"),
            (Method::POST, "/api/standalone/rescan"),
            (Method::POST, "/api/standalone/shutdown"),
        ]
    };
    for host in [
        "evil.com",
        "127.0.0.1.evil.com",
        "localhost.evil.com:8088",
        "LOCALHOST:8088",
        "127.0.0.1:9999",
    ] {
        for (method, path) in routes() {
            let req = Request::builder()
                .method(method.clone())
                .uri(path)
                .header(header::HOST, host)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"folders":[]}"#))
                .unwrap();
            assert_eq!(
                send(&app, req).await.0,
                StatusCode::FORBIDDEN,
                "{method} {path} Host {host}"
            );
        }
    }
    // no request reached a handler
    assert!(
        tokio::time::timeout(Duration::from_millis(50), st.shutdown_requested())
            .await
            .is_err()
    );
    for host in [
        "127.0.0.1:8088",
        "localhost:8088",
        "[::1]:8088",
        "127.0.0.1",
    ] {
        let req = Request::get("/api/standalone/status")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::OK, "Host {host}");
    }
}

#[tokio::test]
async fn cross_origin_writes_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_config(tmp.path());
    let state = lazy_state(tmp.path(), Some(StandaloneState::new(&cfg)));
    let st = state.standalone.clone().unwrap();
    let app = app(&state);
    let (code, _) = send(
        &app,
        Request::post("/api/standalone/shutdown")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), st.shutdown_requested())
            .await
            .is_err(),
        "a cross-origin request must not stop the server"
    );
    // the app's own pages (loopback origin) may write
    let (code, _) = send(
        &app,
        Request::post("/api/standalone/rescan")
            .header(header::ORIGIN, "http://127.0.0.1:8088")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED);
}

/// The served app as `run` builds it, with a front-end folder holding the app page.
fn full_app(dir: &Path, standalone: bool) -> (AppState, Router) {
    let web = dir.join("web");
    std::fs::create_dir_all(web.join("app")).unwrap();
    std::fs::create_dir_all(web.join("overlay")).unwrap();
    std::fs::write(web.join("app/index.html"), "<html>app</html>").unwrap();
    std::fs::write(web.join("overlay/index.html"), "<html>overlay</html>").unwrap();
    let cfg = test_config(dir);
    let st = standalone.then(|| StandaloneState::new(&cfg));
    let mut state = lazy_state(dir, st);
    let mut cfg = (*state.cfg).clone();
    cfg.web_dir = Some(web);
    cfg.standalone = standalone;
    state.cfg = Arc::new(cfg);
    if let Some(st) = &state.standalone {
        st.set_port(8088);
    }
    let app = crate::app_router(state.clone());
    (state, app)
}

/// A DNS-rebound page (`evil.example` resolving to 127.0.0.1) cannot use any route of the
/// open-mode API, the pages or the images: not only `/api/standalone/*`.
#[tokio::test]
async fn standalone_refuses_a_rebound_host_on_the_whole_app() {
    let tmp = tempfile::tempdir().unwrap();
    let (_state, app) = full_app(tmp.path(), true);
    for (method, path) in [
        (Method::GET, "/api/health"),
        (Method::GET, "/api/matches"),
        (Method::POST, "/api/admin/tokens"),
        (Method::POST, "/api/admin/reprocess"),
        (Method::POST, "/api/draft/reset"),
        (Method::GET, "/"),
        (Method::GET, "/cast/overlay"),
    ] {
        let req = Request::builder()
            .method(method.clone())
            .uri(path)
            .header(header::HOST, "evil.example:8088")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send(&app, req).await.0,
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
}

/// Any site can fire a body-less cross-origin POST (`fetch(…, {mode: 'no-cors'})`): with a
/// loopback Host but a foreign Origin, it is refused before the handler runs.
#[tokio::test]
async fn standalone_refuses_a_cross_site_post_on_the_whole_app() {
    let tmp = tempfile::tempdir().unwrap();
    let (_state, app) = full_app(tmp.path(), true);
    for path in [
        "/api/admin/reprocess",
        "/api/draft/reset",
        "/api/draft/undo",
        "/api/admin/tokens",
    ] {
        let req = Request::post(path)
            .header(header::HOST, "127.0.0.1:8088")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::FORBIDDEN, "{path}");
    }
}

/// OBS browser sources (GET, no `Origin`) and the app's pages keep working on every loopback
/// name the user may have typed.
#[tokio::test]
async fn standalone_serves_obs_sources_and_pages_on_loopback_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let (_state, app) = full_app(tmp.path(), true);
    for host in ["127.0.0.1:8088", "localhost:8088", "[::1]:8088"] {
        for path in ["/api/standalone/status", "/cast/overlay", "/"] {
            let req = Request::get(path)
                .header(header::HOST, host)
                .body(Body::empty())
                .unwrap();
            assert_eq!(send(&app, req).await.0, StatusCode::OK, "{host} {path}");
        }
    }
}

/// The box (behind its reverse proxy, public Host names) is unaffected.
#[tokio::test]
async fn server_mode_keeps_accepting_any_host() {
    let tmp = tempfile::tempdir().unwrap();
    let (_state, app) = full_app(tmp.path(), false);
    let req = Request::get("/cast/overlay")
        .header(header::HOST, "stats.example.org")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, req).await.0, StatusCode::OK);
}

// ── DB-backed (skipped without DATABASE_URL) ─────────────────────────────────

/// Serialises the DB-backed standalone tests: `app_settings.standalone_extra_folders` is a
/// single global row, read by every import task and written by the folders test.
static SETTINGS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db_state(
    dir: &Path,
    st: impl FnOnce(&Config) -> StandaloneState,
) -> Option<(AppState, tokio::sync::MutexGuard<'static, ()>)> {
    let mut state = crate::api_tests::test_state(None).await?;
    let guard = SETTINGS.lock().await;
    let mut cfg = (*state.cfg).clone();
    cfg.standalone = true;
    cfg.archive_dir = dir.join("archive");
    std::fs::create_dir_all(&cfg.archive_dir).unwrap();
    let name = format!("test-standalone-{}", uuid::Uuid::new_v4());
    state.standalone = Some(Arc::new(st(&cfg).with_token_name(name).without_upkeep()));
    state.cfg = Arc::new(cfg);
    Some((state, guard))
}

#[tokio::test]
async fn put_folders_persists_and_returns_status() {
    let tmp = tempfile::tempdir().unwrap();
    let extra = tmp.path().join("backup");
    std::fs::create_dir_all(extra.join("old")).unwrap();
    old_replay(&extra.join("old"), "x.StormReplay", b"x");
    let Some((state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    let mut commands = state.standalone.as_ref().unwrap().take_commands().unwrap();
    let app = app(&state);
    let before = get_status(&app).await;
    assert_eq!(before["extraFolders"], serde_json::json!([]), "{before}");
    let extra_str = extra.to_string_lossy().to_string();
    let (code, v) = send(&app, put_folders(&[&extra_str])).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    // the saved list as sent (the parent), next to its expansion in `folders`
    assert_eq!(v["extraFolders"], serde_json::json!([extra_str]));
    let after = get_status(&app).await;
    assert_eq!(after["extraFolders"], serde_json::json!([extra_str]));
    // the parent is resolved to the folder that holds replays
    let resolved = extra.join("old").to_string_lossy().to_string();
    assert_eq!(v["folders"], serde_json::json!([resolved]));
    let saved: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'standalone_extra_folders'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(saved, serde_json::json!([extra_str]));
    assert!(matches!(commands.try_recv(), Ok(Command::SetFolders(_))));
    sqlx::query("DELETE FROM app_settings WHERE key = 'standalone_extra_folders'")
        .execute(&state.db)
        .await
        .unwrap();
}

async fn saved_extras(db: &sqlx::PgPool) -> Option<serde_json::Value> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'standalone_extra_folders'")
        .fetch_optional(db)
        .await
        .unwrap()
}

async fn clear_saved_extras(db: &sqlx::PgPool) {
    sqlx::query("DELETE FROM app_settings WHERE key = 'standalone_extra_folders'")
        .execute(db)
        .await
        .unwrap();
}

/// Only the folders the user sent are saved: the detected folders (shown in `folders`) never
/// become saved extras, and a parent stays a parent (not its expansion).
#[tokio::test]
async fn put_folders_saves_only_the_sent_extras_not_the_detected_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let detected = tmp.path().join("Documents").join("Multiplayer");
    std::fs::create_dir_all(&detected).unwrap();
    let extra = tmp.path().join("backup");
    std::fs::create_dir_all(extra.join("old")).unwrap();
    old_replay(&extra.join("old"), "x.StormReplay", b"x");
    let found = detected.clone();
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c).with_detector(move || vec![found.clone()])
    })
    .await
    else {
        return;
    };
    clear_saved_extras(&state.db).await;
    let _commands = state.standalone.as_ref().unwrap().take_commands().unwrap();
    let app = app(&state);
    let extra_str = extra.to_string_lossy().to_string();
    let (code, v) = send(&app, put_folders(&[&extra_str])).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let detected_str = detected.to_string_lossy().to_string();
    let resolved = extra.join("old").to_string_lossy().to_string();
    assert_eq!(v["folders"], serde_json::json!([detected_str, resolved]));
    assert_eq!(v["extraFolders"], serde_json::json!([extra_str]));
    assert_eq!(
        saved_extras(&state.db).await,
        Some(serde_json::json!([extra_str]))
    );
    clear_saved_extras(&state.db).await;
}

/// A refused folder list (400) changes neither the saved extras nor the status.
#[tokio::test]
async fn put_folders_400_leaves_the_saved_extras_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let kept = tmp.path().join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    let Some((state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    let _commands = state.standalone.as_ref().unwrap().take_commands().unwrap();
    let app = app(&state);
    let kept_str = kept.to_string_lossy().to_string();
    let (code, _) = send(&app, put_folders(&[&kept_str])).await;
    assert_eq!(code, StatusCode::OK);
    let missing = tmp.path().join("missing").to_string_lossy().to_string();
    let (code, v) = send(&app, put_folders(&[&kept_str, &missing])).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(
        saved_extras(&state.db).await,
        Some(serde_json::json!([kept_str]))
    );
    assert_eq!(
        get_status(&app).await["extraFolders"],
        serde_json::json!([kept_str])
    );
    clear_saved_extras(&state.db).await;
}

/// The saved extras are in the status from the first request, while the import task is still
/// seeding: the settings page builds its `PUT` from `extraFolders`, so an empty list there
/// would make "add a folder" save over the user's extras.
#[tokio::test]
async fn saved_extras_are_in_the_status_while_seeding() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    save_extra_folders(&state.db, &["D:/Backup".to_string()])
        .await
        .unwrap();
    // what `main` builds at startup, before the import task runs
    let fresh = StandaloneState::new(&state.cfg);
    fresh.load_saved_extras(&state.db).await;
    let status = fresh.status();
    assert_eq!(status.phase, Phase::Seeding);
    assert_eq!(status.extra_folders, vec!["D:/Backup".to_string()]);
    clear_saved_extras(&state.db).await;
}

#[tokio::test]
async fn token_provisioning_is_idempotent() {
    let Some(state) = crate::api_tests::test_state(None).await else {
        return;
    };
    let db = &state.db;
    let name = format!("test-standalone-{}", uuid::Uuid::new_v4());
    let valid = |token: String| async move {
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        crate::upload::token_valide(db, &h).await
    };
    let rows = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM upload_tokens WHERE name = $1")
            .bind(&name)
            .fetch_one(db)
            .await
            .unwrap()
    };
    let first = token::provision(db, &name).await.unwrap();
    assert!(valid(first.clone()).await);
    let second = token::provision(db, &name).await.unwrap();
    assert_eq!(rows().await, 1, "no duplicate row across restarts");
    assert!(valid(second.clone()).await);
    assert!(
        !valid(first).await,
        "the previous session's token is retired"
    );
    // a revoked token is restored at the next start, never stuck
    sqlx::query("UPDATE upload_tokens SET revoked_at = now() WHERE name = $1")
        .bind(&name)
        .execute(db)
        .await
        .unwrap();
    assert!(!valid(second).await);
    let third = token::provision(db, &name).await.unwrap();
    assert!(valid(third).await);
    assert_eq!(rows().await, 1);
    sqlx::query("DELETE FROM upload_tokens WHERE name = $1")
        .bind(&name)
        .execute(db)
        .await
        .unwrap();
}

/// Full chain on a real listener: the orchestrator imports a folder through the server's own
/// `/api/upload` (token provisioned at startup), then the live watcher picks up a new file.
#[tokio::test]
async fn end_to_end_import_then_live_watch() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    std::fs::create_dir_all(&replays).unwrap();
    // unique content: never a duplicate of another test's upload
    let content = format!("not a replay {}", uuid::Uuid::new_v4());
    old_replay(&replays, "first.StormReplay", content.as_bytes());
    let dirs = vec![replays.clone()];
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c)
            .with_detector(move || dirs.clone())
            .with_free_space(|_| Ok(u64::MAX))
    })
    .await
    else {
        return;
    };
    let app = app(&state);
    let _server = start_server(&state).await;

    let v = wait_status(&app, 30, |v| v["phase"] == "ready").await;
    assert_eq!(v["total"], 1, "{v}");
    assert_eq!(v["done"], 1, "{v}");
    assert_eq!(v["failed"], serde_json::json!([]), "{v}");
    assert!(state.cfg.ledger_path().is_file(), "ledger written");

    let content = format!("not a replay either {}", uuid::Uuid::new_v4());
    std::fs::write(replays.join("second.StormReplay"), content).unwrap();
    let v = wait_status(&app, 30, |v| v["done"] == 2).await;
    assert_eq!(v["phase"], "ready");
    assert_eq!(v["failed"], serde_json::json!([]), "{v}");
}

/// Serves the app (standalone import task included) on an ephemeral port until shutdown.
async fn start_server(state: &AppState) -> tokio::task::JoinHandle<Result<(), String>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let router = app(state);
    let state = state.clone();
    tokio::spawn(
        async move { crate::serve(listener, router, &state, std::future::pending()).await },
    )
}

/// Entries of the ledger file on disk (panics if it is not valid JSON).
fn ledger_entries(path: &Path) -> usize {
    let text = std::fs::read_to_string(path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).expect("ledger is valid JSON");
    v["entries"].as_object().unwrap().len()
}

/// `n` fake replays with unique content (never a duplicate of another test's upload).
fn fake_replays(dir: &Path, n: usize) {
    std::fs::create_dir_all(dir).unwrap();
    let run = uuid::Uuid::new_v4();
    for i in 0..n {
        old_replay(
            dir,
            &format!("f{i:03}.StormReplay"),
            format!("{run}-{i}").as_bytes(),
        );
    }
}

#[tokio::test]
async fn disk_full_blocks_the_backfill_but_keeps_the_live_watcher() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    std::fs::create_dir_all(&replays).unwrap();
    old_replay(&replays, "a.StormReplay", &[7u8; 1000]);
    let dirs = vec![replays.clone()];
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c)
            .with_detector(move || dirs.clone())
            .with_free_space(|_| Ok(10))
    })
    .await
    else {
        return;
    };
    let app = app(&state);
    let _server = start_server(&state).await;
    let v = wait_status(&app, 20, |v| v["phase"] == "disk-full").await;
    assert_eq!(v["freeBytes"], 10);
    assert_eq!(
        v["neededBytes"],
        storm_codex_supervisor::required_free_bytes(1000)
    );
    assert_eq!(v["total"], 0, "the backfill never started: {v}");
    // a new game is still imported by the live watcher
    let content = format!("live game {}", uuid::Uuid::new_v4());
    std::fs::write(replays.join("live.StormReplay"), content).unwrap();
    let v = wait_status(&app, 30, |v| v["done"] == 1).await;
    assert_eq!(v["phase"], "disk-full");
    assert_eq!(v["failed"], serde_json::json!([]), "{v}");
}

#[tokio::test]
async fn disk_gate_sizes_only_the_replays_not_in_the_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    std::fs::create_dir_all(&replays).unwrap();
    for name in ["a.StormReplay", "b.StormReplay"] {
        old_replay(&replays, name, &[1u8; 5000]);
    }
    let dirs = vec![replays.clone()];
    let one_gib = storm_codex_supervisor::required_free_bytes(0);
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c)
            .with_detector(move || dirs.clone())
            // exactly enough when nothing is pending, not enough for the two files
            .with_free_space(move |_| Ok(one_gib))
    })
    .await
    else {
        return;
    };
    let mut ledger = Ledger::open(&state.cfg.ledger_path()).unwrap();
    for name in ["a.StormReplay", "b.StormReplay"] {
        ledger
            .record(name, storm_codex_watcher::ledger::Outcome::Uploaded)
            .unwrap();
    }
    drop(ledger);
    let app = app(&state);
    let _server = start_server(&state).await;
    let v = wait_status(&app, 20, |v| v["phase"] == "ready").await;
    assert_eq!(v["neededBytes"], one_gib, "{v}");
    assert_eq!(v["total"], 0, "{v}");
}

#[tokio::test]
async fn rescan_during_a_backfill_leaves_a_valid_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    fake_replays(&replays, 120);
    let dirs = vec![replays.clone()];
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c)
            .with_detector(move || dirs.clone())
            .with_free_space(|_| Ok(u64::MAX))
    })
    .await
    else {
        return;
    };
    let app = app(&state);
    let _server = start_server(&state).await;
    let v = wait_status(&app, 30, |v| v["done"].as_u64().unwrap_or(0) >= 1).await;
    let rescan = Request::post("/api/standalone/rescan")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, rescan).await.0, StatusCode::ACCEPTED);
    eprintln!("rescan sent at {}/{}", v["done"], v["total"]);
    let v = wait_status(&app, 60, |v| v["phase"] == "ready").await;
    assert_eq!(v["failed"], serde_json::json!([]), "{v}");
    assert_eq!(ledger_entries(&state.cfg.ledger_path()), 120);
}

#[tokio::test]
async fn shutdown_flushes_the_ledger_and_serve_returns() {
    let tmp = tempfile::tempdir().unwrap();
    let replays = tmp.path().join("replays");
    fake_replays(&replays, 120);
    let dirs = vec![replays.clone()];
    let Some((state, _settings)) = db_state(tmp.path(), move |c| {
        StandaloneState::new(c)
            .with_detector(move || dirs.clone())
            .with_free_space(|_| Ok(u64::MAX))
    })
    .await
    else {
        return;
    };
    let st = state.standalone.clone().unwrap();
    let app = app(&state);
    let server = start_server(&state).await;
    wait_status(&app, 30, |v| v["done"].as_u64().unwrap_or(0) >= 5).await;
    st.request_shutdown();
    let served = tokio::time::timeout(Duration::from_secs(20), server)
        .await
        .expect("serve returns after the shutdown request")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");
    let status = st.status();
    eprintln!("stopped at {}/{}", status.done, status.total);
    assert!(status.done >= 5);
    assert_eq!(
        ledger_entries(&state.cfg.ledger_path()),
        status.done - status.failed.len(),
        "every outcome recorded before the stop is on disk"
    );
}

/// Writes a snapshot holding `heroes` (names) at `file`, replacing it.
fn snapshot_with(file: &Path, heroes: &[&str]) {
    let gz = flate2::write::GzEncoder::new(
        std::fs::File::create(file).unwrap(),
        flate2::Compression::fast(),
    );
    let mut tar = tar::Builder::new(gz);
    let body = serde_json::to_string(
        &heroes
            .iter()
            .map(|h| serde_json::json!({"name": h, "role": "Support", "universe": "Nexus"}))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_size(body.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    tar.append_data(&mut h, "heroes.json", body.as_bytes())
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap();
}

async fn hero_exists(state: &AppState, name: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM dim_heroes WHERE id = $1)")
        .bind(name)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// An app update ships a newer bundled snapshot: it is ingested although `dim_heroes` is not
/// empty; the same snapshot is not ingested twice.
#[tokio::test]
async fn a_newer_bundled_snapshot_is_ingested_once() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((mut state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let (old, new) = (format!("Old {tag}"), format!("New {tag}"));
    let file = tmp.path().join("referential.tar.gz");
    let mut cfg = (*state.cfg).clone();
    cfg.referential_file = Some(file.clone());
    cfg.images_dir = tmp.path().join("images");
    state.cfg = Arc::new(cfg);

    snapshot_with(&file, &[&old]);
    orchestrator::seed_reference_data(&state).await;
    assert!(hero_exists(&state, &old).await, "first snapshot ingested");

    // release N+1: the bundled file changes, dim_heroes is not empty
    snapshot_with(&file, &[&old, &new]);
    orchestrator::seed_reference_data(&state).await;
    assert!(hero_exists(&state, &new).await, "newer snapshot ingested");

    // same snapshot at the next start: not ingested again
    sqlx::query("DELETE FROM dim_heroes WHERE id = $1")
        .bind(&new)
        .execute(&state.db)
        .await
        .unwrap();
    orchestrator::seed_reference_data(&state).await;
    assert!(
        !hero_exists(&state, &new).await,
        "unchanged snapshot skipped"
    );
    sqlx::query("DELETE FROM dim_heroes WHERE id = $1")
        .bind(&old)
        .execute(&state.db)
        .await
        .unwrap();
}

/// An update raised `PARSER_VERSION`: the stored replays parsed by the older parser are
/// re-processed in the background, with the progress in the status.
#[tokio::test]
async fn outdated_replays_are_reprocessed_with_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    let replay = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay");
    // a fingerprint of its own: other tests upload the same fixture
    let key = format!("reprocess-{}", uuid::Uuid::new_v4().simple());
    std::fs::copy(
        &replay,
        state.cfg.archive_dir.join(Config::archive_entry(&key)),
    )
    .unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO uploads (filename, fingerprint, archived_path, status, parser_version)
         VALUES ('old.StormReplay', $1, $2, 'parsed', $3) RETURNING id",
    )
    .bind(&key)
    .bind(Config::archive_entry(&key))
    .bind(crate::PARSER_VERSION - 1)
    .fetch_one(&state.db)
    .await
    .unwrap();

    orchestrator::reprocess_outdated(&state).await;

    let version: i32 = sqlx::query_scalar("SELECT parser_version FROM uploads WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let status = state.standalone.as_ref().unwrap().status();
    sqlx::query("DELETE FROM uploads WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(version, crate::PARSER_VERSION, "re-processed");
    assert!(status.reprocess_total >= 1, "{status:?}");
    assert_eq!(status.reprocess_done, status.reprocess_total, "{status:?}");
}

#[test]
fn only_the_seeded_operator_name_is_replaced() {
    use orchestrator::detected_names;
    let me = || Some("Me".to_string());
    assert_eq!(
        detected_names(&["matella".into()], me()),
        Some(vec!["Me".into()])
    );
    assert_eq!(detected_names(&[], me()), Some(vec!["Me".into()]));
    assert_eq!(
        detected_names(&["Mine".into()], me()),
        None,
        "user's names kept"
    );
    assert_eq!(
        detected_names(&["matella".into(), "Alt".into()], me()),
        None
    );
    assert_eq!(
        detected_names(&["matella".into()], None),
        None,
        "nothing imported"
    );
    assert_eq!(detected_names(&["matella".into()], Some(" ".into())), None);
}

async fn setting(state: &AppState, key: &str) -> Option<serde_json::Value> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .unwrap()
}

async fn put_setting(state: &AppState, key: &str, value: Option<serde_json::Value>) {
    match value {
        Some(v) => sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(v)
        .execute(&state.db)
        .await
        .unwrap(),
        None => sqlx::query("DELETE FROM app_settings WHERE key = $1")
            .bind(key)
            .execute(&state.db)
            .await
            .unwrap(),
    };
}

/// A fresh install carries the box operator's name (migration 0003): after the first import it
/// becomes the most frequent player, once; names the user set are never replaced.
#[tokio::test]
async fn operator_names_are_detected_once_after_the_import() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((state, _settings)) = db_state(tmp.path(), |c| {
        StandaloneState::new(c).with_detector(Vec::new)
    })
    .await
    else {
        return;
    };
    let saved = (
        setting(&state, "operator_names").await,
        setting(&state, "operator_names_detected").await,
    );
    let top: Option<String> = sqlx::query_scalar(
        "SELECT name FROM match_players WHERE name IS NOT NULL AND name <> ''
         GROUP BY name ORDER BY count(DISTINCT match_id) DESC, name LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .unwrap();

    // user's own names: kept, marked as done
    put_setting(&state, "operator_names_detected", None).await;
    put_setting(&state, "operator_names", Some(serde_json::json!(["Mine"]))).await;
    orchestrator::detect_operator_names(&state).await;
    let mine = setting(&state, "operator_names").await;
    let marked = setting(&state, "operator_names_detected").await.is_some();

    // the migration seed: replaced by the most frequent player (when any game exists)
    put_setting(&state, "operator_names_detected", None).await;
    put_setting(
        &state,
        "operator_names",
        Some(serde_json::json!(["matella"])),
    )
    .await;
    orchestrator::detect_operator_names(&state).await;
    let detected = setting(&state, "operator_names").await;
    // once: a later edit back to the seed is not replaced again
    put_setting(
        &state,
        "operator_names",
        Some(serde_json::json!(["matella"])),
    )
    .await;
    orchestrator::detect_operator_names(&state).await;
    let again = setting(&state, "operator_names").await;

    put_setting(&state, "operator_names", saved.0).await;
    put_setting(&state, "operator_names_detected", saved.1).await;
    assert_eq!(mine, Some(serde_json::json!(["Mine"])));
    assert!(marked);
    match top {
        Some(top) => {
            assert_eq!(detected, Some(serde_json::json!([top])));
            assert_eq!(again, Some(serde_json::json!(["matella"])));
        }
        None => assert_eq!(detected, Some(serde_json::json!(["matella"]))),
    }
}
