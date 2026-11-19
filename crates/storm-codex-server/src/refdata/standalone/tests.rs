#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::refdata::tests::{
    bundle_with_version, drop_db, fixture_dir_copy, isolated_db, DirSource,
};
use crate::standalone::StandaloneState;
use axum::body::Body;
use http_body_util::BodyExt;
use std::future::IntoFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

const STATUS: &str = "/api/standalone/reference";
const REFRESH: &str = "/api/standalone/reference/refresh";
const IMAGE: &str = "/images/heroes/abathur.png";

async fn call(router: &Router, method: &str, path: &str, body: Body) -> (StatusCode, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

async fn get_json(router: &Router, path: &str) -> Value {
    let (status, bytes) = call(router, "GET", path, Body::empty()).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

async fn wait_for(router: &Router, path: &str, check: impl Fn(&Value) -> bool) -> Value {
    let start = tokio::time::Instant::now();
    loop {
        let v = get_json(router, path).await;
        if check(&v) {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{path}: {v}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn setup(db: &PgPool, root: &Path, folders: Vec<PathBuf>) -> AppState {
    let cfg = crate::standalone::tests::test_config(root);
    fs::create_dir_all(&cfg.archive_dir).unwrap();
    let st = StandaloneState::new(&cfg)
        .with_detector(move || folders.clone())
        .with_free_space(|_| Ok(u64::MAX))
        .without_upkeep();
    let mut app = crate::test_app_state(cfg, db.clone());
    app.standalone = Some(Arc::new(st));
    app
}

// Model an unreachable feed; on startup we can count failures without depending on DNS or GitHub.
#[derive(Default)]
struct Offline(AtomicUsize);
impl load::Source for Offline {
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
        assert_eq!(name, "manifest.json");
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(RefdataError::Io("offline".into()))
    }
    fn download(&self, _: &str, _: &Path) -> Result<(), RefdataError> {
        panic!("offline source cannot download an archive")
    }
}

fn set_version(feed: &Path, version: &str, schema: u64) {
    let path = feed.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["datasetVersion"] = json!(version);
    manifest["schemaVersion"] = json!(schema);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

/// A fresh profile can import and browse via the actual watcher and upload endpoint with no
/// reference data, even as the retry loop is running. Activation repairs its existing match.
#[tokio::test]
async fn first_offline_replay_stays_browsable_and_is_corrected_in_place() {
    let Some((db, db_name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("replays");
    fs::create_dir(&folder).unwrap();
    let replay = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../storm-stats/tests/data/silver-city-aram.StormReplay");
    crate::standalone::tests::old_replay(&folder, "first.StormReplay", &fs::read(replay).unwrap());
    let app = setup(&db, tmp.path(), vec![folder]);
    let st = app.standalone.as_ref().unwrap();
    st.reference.load(&db).await;
    let router = crate::app_router(app.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    st.set_port(address.port());
    let server = tokio::spawn(axum::serve(listener, router.clone()).into_future());
    let importer = crate::standalone::spawn(app.clone(), format!("http://{address}"));
    let offline = Arc::new(Offline::default());
    let worker = tokio::spawn(run(
        app.clone(),
        offline.clone(),
        [Duration::from_millis(15); 3],
    ));
    wait_for(&router, STATUS, |v| v["phase"] == "error").await;
    wait_for(&router, "/api/standalone/status", |v| {
        v["phase"] == "ready" && v["done"] == 1
    })
    .await;
    assert!(get_json(&router, "/api/patches").await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        call(&router, "GET", IMAGE, Body::empty()).await.0,
        StatusCode::NOT_FOUND
    );
    let match_id: i64 = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(id) =
                sqlx::query_scalar("SELECT match_id FROM uploads WHERE status = 'parsed'")
                    .fetch_optional(&db)
                    .await
                    .unwrap()
            {
                break id;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the first replay is projected while the feed is offline");
    let path = format!("/api/matches/{match_id}");
    let before = get_json(&router, &path).await;
    let revision = get_json(&router, STATUS).await["revision"].clone();
    tokio::time::timeout(Duration::from_secs(3), async {
        while offline.0.load(Ordering::SeqCst) < 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert_eq!(
        offline.0.load(Ordering::SeqCst),
        4,
        "three short retries, then daily"
    );
    worker.abort();
    let _ = worker.await;

    let feed = fixture_dir_copy(tmp.path());
    let source = Arc::new(DirSource::new(feed.clone()));
    let dataset = load::prepare(source.as_ref(), None).unwrap().unwrap();
    let rows = super::super::build(&dataset).unwrap();
    let lut = rows
        .talents
        .into_iter()
        .map(|t| (t.tree_id, t.hero_id))
        .collect();
    let players = before["players"].as_object().unwrap();
    let corrections = crate::project::hero_corrections(players, &lut);
    if corrections.is_empty() {
        // The committed replay is real; the trimmed fixture may not cover a bad
        // attribution. Inject a fixture mapping for the actual talent IDs to exercise it.
        let p = players
            .values()
            .find(|p| {
                p["hero"] != "Abathur" && p["talents"].as_object().is_some_and(|o| !o.is_empty())
            })
            .unwrap();
        let mut game = dataset.game;
        game["heroes"]["Abathur"]["structure"]["talents"]["Level20"] = Value::Array(
            p["talents"]
                .as_object()
                .unwrap()
                .values()
                .map(|id| json!({"talentId": id}))
                .collect(),
        );
        let bytes = serde_json::to_vec(&game).unwrap();
        fs::write(feed.join("game.json"), &bytes).unwrap();
        let manifest = feed.join("manifest.json");
        let mut doc: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        doc["files"]["game.json"] = json!(super::super::sha256_hex(&bytes));
        fs::write(manifest, serde_json::to_vec(&doc).unwrap()).unwrap();
    }
    round(&app, &st.reference, source.clone()).await.unwrap();
    let after = get_json(&router, &path).await;
    let status = get_json(&router, STATUS).await;
    assert_eq!(status["phase"], "ready");
    assert_ne!(status["revision"], revision);
    assert!(!get_json(&router, "/api/patches").await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(after["id"], before["id"]);
    let fixed = after["players"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(toon, p)| p["hero"] != before["players"][*toon]["hero"])
        .collect::<Vec<_>>();
    assert!(!fixed.is_empty(), "fixture must exercise hero repair");
    for (toon, p) in &fixed {
        assert_eq!(p["heroCorrectedFrom"], before["players"][*toon]["hero"]);
        let promoted: String = sqlx::query_scalar(
            "SELECT hero FROM match_players WHERE match_id = $1 AND toon_handle = $2",
        )
        .bind(match_id)
        .bind(toon)
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(p["hero"], promoted);
    }
    for event in after["match"]["takedowns"].as_array().unwrap() {
        for player in std::iter::once(&event["victim"]).chain(event["killers"].as_array().unwrap())
        {
            if let Some((_, fixed)) = fixed
                .iter()
                .find(|(toon, _)| player["player"].as_str() == Some(toon))
            {
                assert_eq!(player["hero"], fixed["hero"]);
            }
        }
    }
    set_version(&feed, "another-revision", 1);
    round(&app, &st.reference, source).await.unwrap();
    assert_eq!(
        get_json(&router, &path).await,
        after,
        "repair is idempotent"
    );
    st.stop_import();
    importer.await.unwrap();
    server.abort();
    drop_db(db, &db_name).await;
}

#[test]
fn retries_are_bounded_before_the_daily_schedule() {
    assert_eq!(
        RETRIES,
        [
            Duration::from_secs(60),
            Duration::from_secs(300),
            Duration::from_secs(1800)
        ]
    );
    assert_eq!(DAILY, Duration::from_secs(86400));
}

#[tokio::test]
async fn refresh_retains_last_good_on_failure_and_recovers_same_version_after_restore() {
    let Some((db, db_name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = fixture_dir_copy(tmp.path());
    let app = setup(&db, tmp.path(), Vec::new());
    let st = app.standalone.as_ref().unwrap();
    let router = crate::app_router(app.clone());
    let source = || Arc::new(DirSource::new(dir.clone()));
    set_version(&dir, "one", 1);
    round(&app, &st.reference, source()).await.unwrap();
    let initial = get_json(&router, STATUS).await;
    assert_eq!(initial["version"], "one");
    let (code, image) = call(&router, "GET", IMAGE, Body::empty()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(!image.is_empty());
    let unchanged = source();
    round(&app, &st.reference, unchanged.clone()).await.unwrap();
    assert_eq!(unchanged.asked(), ["manifest.json"]);
    assert_eq!(get_json(&router, STATUS).await, initial);
    set_version(&dir, "two", 2);
    assert!(matches!(
        round(&app, &st.reference, source()).await,
        Err(SyncError::Data(RefdataError::Schema(2)))
    ));
    set_version(&dir, "two", 1);
    let archive = dir.join("dataset.tar.gz");
    let good = fs::read(&archive).unwrap();
    fs::write(&archive, &good[..1024]).unwrap();
    assert!(matches!(
        round(&app, &st.reference, source()).await,
        Err(SyncError::Data(RefdataError::Hash { .. }))
    ));
    // An internally valid gzip and matching download checksum still cannot omit a required image.
    let checksum_path = dir.join("dataset.tar.gz.sha256");
    let checksum = fs::read(&checksum_path).unwrap();
    let empty = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let absent = tar::Builder::new(empty)
        .into_inner()
        .unwrap()
        .finish()
        .unwrap();
    fs::write(&archive, &absent).unwrap();
    fs::write(&checksum_path, super::super::sha256_hex(&absent)).unwrap();
    assert!(round(&app, &st.reference, source()).await.is_err());
    assert_eq!(load::last_version(&db).await.as_deref(), Some("one"));
    assert_eq!(get_json(&router, STATUS).await, initial);
    assert_eq!(call(&router, "GET", IMAGE, Body::empty()).await.1, image);
    fs::write(&archive, &good).unwrap();
    fs::write(&checksum_path, checksum).unwrap();
    sqlx::query("ALTER TABLE dim_heroes ADD CONSTRAINT refuse_test_write CHECK (false) NOT VALID")
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        round(&app, &st.reference, source()).await,
        Err(SyncError::Database(_))
    ));
    assert_eq!(get_json(&router, STATUS).await, initial);
    assert_eq!(call(&router, "GET", IMAGE, Body::empty()).await.1, image);
    assert_eq!(load::last_version(&db).await.as_deref(), Some("one"));
    sqlx::query("ALTER TABLE dim_heroes DROP CONSTRAINT refuse_test_write")
        .execute(&db)
        .await
        .unwrap();
    round(&app, &st.reference, source()).await.unwrap();
    let updated = get_json(&router, STATUS).await;
    assert_eq!(updated["version"], "two");
    assert_ne!(updated["revision"], initial["revision"]);

    let (code, disabled) = call(&router, "PUT", STATUS, Body::from(r#"{"enabled":false}"#)).await;
    assert_eq!(code, StatusCode::OK);
    let disabled: Value = serde_json::from_slice(&disabled).unwrap();
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["phase"], "ready");
    assert_eq!(disabled["revision"], updated["revision"]);
    assert_eq!(
        call(&router, "POST", REFRESH, Body::empty()).await.0,
        StatusCode::CONFLICT
    );
    let dest = tempfile::tempdir().unwrap();
    let restarted = setup(&db, tmp.path(), Vec::new());
    restarted
        .standalone
        .as_ref()
        .unwrap()
        .reference
        .load(&db)
        .await;
    let restarted_router = crate::app_router(restarted.clone());
    assert_eq!(get_json(&restarted_router, STATUS).await, disabled);
    assert_eq!(
        call(&restarted_router, "GET", IMAGE, Body::empty()).await.1,
        image
    );

    // The backup restores the DB, but not managed images, into a new data root.
    let restored = setup(&db, dest.path(), Vec::new());
    let st = restored.standalone.as_ref().unwrap();
    st.reference.load(&db).await;
    let router = crate::app_router(restored.clone());
    assert_eq!(get_json(&router, STATUS).await, disabled);
    assert_eq!(
        call(&router, "GET", IMAGE, Body::empty()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&router, "PUT", STATUS, Body::from(r#"{"enabled":true}"#))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&router, "POST", REFRESH, Body::empty()).await.0,
        StatusCode::ACCEPTED
    );
    let foreign = Request::post(REFRESH)
        .header(header::HOST, "localhost")
        .header(header::ORIGIN, "http://elsewhere.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(foreign).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let restore_src = source();
    let worker = tokio::spawn(run(restored.clone(), restore_src.clone(), RETRIES));
    let recovered = wait_for(&router, STATUS, |v| {
        v["phase"] == "ready" && v["revision"] != updated["revision"]
    })
    .await;
    assert_eq!(recovered["version"], updated["version"]);
    assert!(restore_src.asked().contains(&"dataset.tar.gz".into()));
    assert_eq!(call(&router, "GET", IMAGE, Body::empty()).await.1, image);
    st.stop_import();
    worker.await.unwrap();
    drop_db(db, &db_name).await;
}

#[tokio::test]
async fn legacy_bundle_ingests_once_per_version_and_cannot_downgrade_a_network_generation() {
    let Some((db, db_name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let app = setup(&db, tmp.path(), Vec::new());
    let st = app.standalone.as_ref().unwrap();
    let path = tmp.path().join("legacy.tar.gz");
    let router = crate::app_router(app.clone());
    // Simulate 0.1.1's DB version marker and flat assets (no managed generation).
    let legacy_dir = fixture_dir_copy(&tmp.path().join("old-release"));
    set_version(&legacy_dir, "old", 1);
    load::refresh(
        &db,
        &app.cfg.images_dir,
        Arc::new(DirSource::new(legacy_dir)),
    )
    .await;
    st.reference.load(&db).await;
    let before = get_json(&router, STATUS).await;
    assert_eq!(before["version"], "old");
    assert_eq!(
        call(&router, "GET", IMAGE, Body::empty()).await.0,
        StatusCode::OK
    );
    bundle_with_version(&path, "old");
    bundle(&app, &st.reference, &path).await.unwrap();
    let old = get_json(&router, STATUS).await;
    assert_eq!(old["version"], "old");
    assert_ne!(old["revision"], before["revision"]);
    assert_eq!(
        call(&router, "GET", IMAGE, Body::empty()).await.0,
        StatusCode::OK
    );
    bundle(&app, &st.reference, &path).await.unwrap();
    assert_eq!(get_json(&router, STATUS).await, old);
    bundle_with_version(&path, "new-bundle");
    bundle(&app, &st.reference, &path).await.unwrap();
    assert_eq!(get_json(&router, STATUS).await["version"], "new-bundle");
    let dir = fixture_dir_copy(tmp.path());
    set_version(&dir, "network", 1);
    round(&app, &st.reference, Arc::new(DirSource::new(dir)))
        .await
        .unwrap();
    bundle(&app, &st.reference, &path).await.unwrap();
    assert_eq!(get_json(&router, STATUS).await["version"], "network");
    drop_db(db, &db_name).await;
}

/// CI's metadata-only fixture still runs the existing adapter tests. With the matching pinned
/// archive available this also exercises the actual full release through standalone activation.
#[tokio::test]
async fn pinned_full_archive_activates_and_checks_only_manifest_next_round() {
    let Some(dir) = std::env::var_os("REFDATA_DATASET_DIR").map(PathBuf::from) else {
        return;
    };
    if !dir.join("dataset.tar.gz").is_file() {
        return;
    }
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let app = setup(&db, tmp.path(), Vec::new());
    let state = &app.standalone.as_ref().unwrap().reference;
    let start = std::time::Instant::now();
    round(&app, state, Arc::new(DirSource::new(dir.clone())))
        .await
        .unwrap();
    let active = state.view().active.clone().unwrap();
    assert!(active.complete(&asset_root(&app)));
    assert!(active.images.len() >= 120);
    let check = Arc::new(DirSource::new(dir));
    round(&app, state, check.clone()).await.unwrap();
    assert_eq!(check.asked(), ["manifest.json"]);
    eprintln!(
        "pinned dataset: {} images checked and activated, then unchanged check in {:?}",
        active.images.len(),
        start.elapsed()
    );
    drop_db(db, &name).await;
}

/// A manual check can be interrupted mid-transfer without publishing partial files or clearing
/// a cached release. Turning sync off prevents even its next request (the archive checksum).
#[tokio::test]
async fn disabling_during_a_manual_download_keeps_the_active_generation() {
    struct HeldDownload {
        files: DirSource,
        started: Notify,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl load::Source for HeldDownload {
        fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
            self.files.get(name)
        }
        fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError> {
            fs::write(file, b"partial").unwrap();
            self.started.notify_one();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(15))
                .unwrap();
            self.files.download(name, file)
        }
    }
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = fixture_dir_copy(tmp.path());
    let app = setup(&db, tmp.path(), Vec::new());
    let st = app.standalone.as_ref().unwrap();
    st.reference.load(&db).await;
    let router = crate::app_router(app.clone());
    set_version(&dir, "before", 1);
    round(&app, &st.reference, Arc::new(DirSource::new(dir.clone())))
        .await
        .unwrap();
    let initial = get_json(&router, STATUS).await;
    let image = call(&router, "GET", IMAGE, Body::empty()).await.1;
    let (release, ready) = std::sync::mpsc::channel();
    let source = Arc::new(HeldDownload {
        files: DirSource::new(dir.clone()),
        started: Notify::new(),
        release: Mutex::new(ready),
    });
    let worker = tokio::spawn(run(app.clone(), source.clone(), RETRIES));
    wait_for(&router, STATUS, |v| {
        v["phase"] == "ready" && !source.files.asked().is_empty()
    })
    .await;
    assert_eq!(source.files.asked(), ["manifest.json"]);
    set_version(&dir, "after", 1);
    assert_eq!(
        call(&router, "POST", REFRESH, Body::empty()).await.0,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(10), source.started.notified())
        .await
        .unwrap();
    let syncing = get_json(&router, STATUS).await;
    assert_eq!(syncing["phase"], "syncing");
    assert_eq!(syncing["revision"], initial["revision"]);
    assert_eq!(call(&router, "GET", IMAGE, Body::empty()).await.1, image);
    assert_eq!(
        call(&router, "PUT", STATUS, Body::from(r#"{"enabled":false}"#))
            .await
            .0,
        StatusCode::OK
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while fs::read_dir(asset_root(&app)).unwrap().count() != 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let disabled = get_json(&router, STATUS).await;
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["phase"], "ready");
    assert_eq!(disabled["revision"], initial["revision"]);
    assert_eq!(load::last_version(&db).await.as_deref(), Some("before"));
    assert!(!source
        .files
        .asked()
        .contains(&"dataset.tar.gz.sha256".into()));
    st.stop_import();
    worker.await.unwrap();
    drop_db(db, &name).await;
}

/// Checksum-consistent but incomplete releases, and archives over a resource budget, are refused
/// before activation: staging is removed and the working generation keeps serving.
#[tokio::test]
async fn incomplete_or_oversized_updates_keep_the_active_generation() {
    use crate::refdata::tests::rewrite_listed;
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = fixture_dir_copy(tmp.path());
    let app = setup(&db, tmp.path(), Vec::new());
    let st = app.standalone.as_ref().unwrap();
    st.reference.load(&db).await;
    let router = crate::app_router(app.clone());
    set_version(&dir, "one", 1);
    round(&app, &st.reference, Arc::new(DirSource::new(dir.clone())))
        .await
        .unwrap();
    let initial = get_json(&router, STATUS).await;
    let image = call(&router, "GET", IMAGE, Body::empty()).await.1;
    let root = asset_root(&app);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    async fn kept(router: &Router, db: &PgPool, root: &Path, initial: &Value, image: &[u8]) {
        assert_eq!(&get_json(router, STATUS).await, initial);
        assert_eq!(call(router, "GET", IMAGE, Body::empty()).await.1, image);
        assert_eq!(load::last_version(db).await.as_deref(), Some("one"));
        assert_eq!(fs::read_dir(root).unwrap().count(), 1, "staging is removed");
    }

    set_version(&dir, "two", 1);
    let good_assets = fs::read(dir.join("assets.json")).unwrap();
    let mut assets: Value = serde_json::from_slice(&good_assets).unwrap();
    assets
        .as_object_mut()
        .unwrap()
        .remove("heroportraits/storm_ui_ingame_hero_loadingscreen_abathur.png");
    rewrite_listed(&dir, "assets.json", &serde_json::to_vec(&assets).unwrap());
    let got = round(&app, &st.reference, Arc::new(DirSource::new(dir.clone()))).await;
    assert!(
        matches!(got, Err(SyncError::Data(RefdataError::Shape(_)))),
        "{got:?}"
    );
    kept(&router, &db, &root, &initial, &image).await;
    rewrite_listed(&dir, "assets.json", &good_assets);

    let good_patches = fs::read(dir.join("patches.json")).unwrap();
    rewrite_listed(&dir, "patches.json", b"[]");
    let got = round(&app, &st.reference, Arc::new(DirSource::new(dir.clone()))).await;
    assert!(
        matches!(got, Err(SyncError::Data(RefdataError::Shape(_)))),
        "{got:?}"
    );
    kept(&router, &db, &root, &initial, &image).await;
    rewrite_listed(&dir, "patches.json", &good_patches);

    let open = load::Limits {
        archive_bytes: u64::MAX,
        expanded_bytes: u64::MAX,
        image_bytes: u64::MAX,
    };
    for limits in [
        load::Limits {
            archive_bytes: 64,
            ..open
        },
        load::Limits {
            expanded_bytes: 2048,
            ..open
        },
        load::Limits {
            image_bytes: 8,
            ..open
        },
    ] {
        let src = Arc::new(DirSource::new(dir.clone()).with_limits(limits));
        let got = round(&app, &st.reference, src).await;
        assert!(
            matches!(got, Err(SyncError::Data(RefdataError::Budget { .. }))),
            "{limits:?}: {got:?}"
        );
        kept(&router, &db, &root, &initial, &image).await;
    }

    round(&app, &st.reference, Arc::new(DirSource::new(dir)))
        .await
        .unwrap();
    assert_eq!(get_json(&router, STATUS).await["version"], "two");
    drop_db(db, &name).await;
}
