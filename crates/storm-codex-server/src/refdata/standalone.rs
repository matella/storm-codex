//! Standalone reference sync. A complete immutable image generation precedes the DB commit
//! that selects it. In-flight downloads never write to either the active or the legacy image tree.

use super::{load, parse_manifest, safe_image_dest, Dataset, Manifest, RefdataError, Rows};
use crate::{standalone::api::error, AppState};
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tower_http::services::ServeFile;
use uuid::Uuid;

const ENABLED: &str = "standalone_reference_enabled";
const ACTIVE: &str = "standalone_reference_active";
const DAILY: Duration = Duration::from_secs(24 * 3600);
const RETRIES: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(30 * 60),
];

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Waiting,
    Syncing,
    Ready,
    Error,
}

#[derive(Clone, Serialize)]
struct Status {
    enabled: bool,
    phase: Phase,
    version: Option<String>,
    revision: String,
}

/// Committed in the same transaction as the reference rows. The UUID is both the
/// public opaque revision and the path of the immutable image generation.
#[derive(Clone, Deserialize, Serialize)]
struct Active {
    version: String,
    revision: String,
    images: BTreeMap<String, String>,
    bundle_hash: Option<String>,
}

impl Active {
    fn dir(&self, root: &Path) -> PathBuf {
        root.join(&self.revision)
    }

    fn complete(&self, root: &Path) -> bool {
        let dir = self.dir(root);
        dir.is_dir()
            && self
                .images
                .iter()
                .all(|(path, sha)| load::check_file_hash(&dir.join(path), sha).is_ok())
    }
}

struct View {
    status: Status,
    settings_loaded: bool,
    active: Option<Arc<Active>>,
}

pub(crate) struct ReferenceState {
    view: Mutex<View>,
    /// Serializes the final activation and preference writes, not downloads.
    control: tokio::sync::Mutex<()>,
    check: Notify,
}

impl Default for ReferenceState {
    fn default() -> Self {
        Self {
            view: Mutex::new(View {
                status: Status {
                    enabled: true,
                    phase: Phase::Waiting,
                    version: None,
                    revision: Uuid::new_v4().to_string(),
                },
                settings_loaded: false,
                active: None,
            }),
            control: tokio::sync::Mutex::new(()),
            check: Notify::new(),
        }
    }
}

impl ReferenceState {
    fn view(&self) -> std::sync::MutexGuard<'_, View> {
        self.view.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) async fn load(&self, db: &PgPool) {
        let settings: BTreeMap<String, Value> = match sqlx::query_as::<_, (String, Value)>(
            "SELECT key, value FROM app_settings WHERE key = ANY($1)",
        )
        .bind(&[ENABLED, ACTIVE, load::VERSION_KEY][..])
        .fetch_all(db)
        .await
        {
            Ok(rows) => rows.into_iter().collect(),
            Err(e) => {
                tracing::warn!("reference sync: could not load saved settings ({e})");
                let mut view = self.view();
                view.status.enabled = false;
                view.status.phase = Phase::Error;
                return;
            }
        };
        let version = settings.get(load::VERSION_KEY).and_then(Value::as_str);
        let active = settings
            .get(ACTIVE)
            .and_then(|v| serde_json::from_value::<Active>(v.clone()).ok())
            .filter(|a| {
                Some(a.version.as_str()) == version
                    && Uuid::parse_str(&a.revision).is_ok_and(|id| id.to_string() == a.revision)
                    && a.images.keys().all(|path| safe_image_dest(path))
            });
        let mut view = self.view();
        if let Some(active) = active {
            view.status.revision = active.revision.clone();
            view.active = Some(Arc::new(active));
        }
        view.settings_loaded = true;
        view.status.enabled = settings
            .get(ENABLED)
            .and_then(Value::as_bool)
            .unwrap_or(true);
        view.status.version = version.map(str::to_owned);
        view.status.phase = idle(&view.status);
    }
}

fn idle(status: &Status) -> Phase {
    if status.version.is_some() {
        Phase::Ready
    } else {
        Phase::Waiting
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/standalone/reference", get(status).put(set_enabled))
        .route("/api/standalone/reference/refresh", post(refresh))
        .route("/images/heroes/{name}", get(hero_image))
        .route("/images/battlegrounds/{name}", get(map_image))
}

async fn status(State(app): State<AppState>) -> Response {
    match &app.standalone {
        Some(st) => Json(st.reference.view().status.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    enabled: bool,
}

async fn set_enabled(State(app): State<AppState>, Json(body): Json<Preference>) -> Response {
    let Some(st) = &app.standalone else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let state = &st.reference;
    let _guard = state.control.lock().await;
    if let Err(e) = sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, $2)
                               ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(ENABLED)
    .bind(json!(body.enabled))
    .execute(&app.db)
    .await
    {
        tracing::warn!("reference sync: could not save preference ({e})");
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not save reference preference".into(),
        );
    }
    let mut view = state.view();
    view.status.enabled = body.enabled;
    if !body.enabled {
        view.status.phase = idle(&view.status);
    }
    state.check.notify_one();
    Json(view.status.clone()).into_response()
}

async fn refresh(State(app): State<AppState>) -> Response {
    let Some(st) = &app.standalone else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let view = st.reference.view();
    if !view.status.enabled {
        return error(StatusCode::CONFLICT, "reference sync is disabled".into());
    }
    st.reference.check.notify_one();
    StatusCode::ACCEPTED.into_response()
}

async fn hero_image(
    State(app): State<AppState>,
    RoutePath(name): RoutePath<String>,
    req: Request,
) -> Response {
    image(&app, "heroes", &name, req).await
}

async fn map_image(
    State(app): State<AppState>,
    RoutePath(name): RoutePath<String>,
    req: Request,
) -> Response {
    image(&app, "battlegrounds", &name, req).await
}

async fn image(app: &AppState, group: &str, name: &str, mut req: Request) -> Response {
    if name.contains('/') || !safe_image_dest(name) || !name.ends_with(".png") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = format!("{group}/{name}");
    let active = app
        .standalone
        .as_ref()
        .and_then(|st| st.reference.view().active.clone());
    // A URL can now change content without changing its name. The ServeFile validator has
    // one-second granularity and does not distinguish two generations with the same timestamp.
    req.headers_mut().remove(header::IF_MODIFIED_SINCE);
    let (parts, _) = req.into_parts();
    let managed = active.filter(|a| a.images.contains_key(&path));
    let mut response = if let Some(active) = managed {
        serve_image(
            active.dir(&asset_root(app)).join(&path),
            &parts.method,
            &parts.headers,
        )
        .await
    } else {
        StatusCode::NOT_FOUND.into_response()
    };
    if response.status() == StatusCode::NOT_FOUND {
        response = serve_image(app.cfg.images_dir.join(path), &parts.method, &parts.headers).await;
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

async fn serve_image(path: PathBuf, method: &Method, headers: &HeaderMap) -> Response {
    let mut file_request = Request::new(axum::body::Body::empty());
    *file_request.method_mut() = method.clone();
    *file_request.headers_mut() = headers.clone();
    match ServeFile::new(path).try_call(file_request).await {
        Ok(response) => response.into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!("reference image: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn asset_root(app: &AppState) -> PathBuf {
    app.cfg.data_dir().join("reference-assets")
}

pub(crate) fn spawn(app: AppState) {
    tokio::spawn(run(app, Arc::new(load::Http::standalone()), RETRIES));
}

async fn run(app: AppState, src: Arc<dyn load::Source>, retries: [Duration; 3]) {
    let Some(st) = &app.standalone else { return };
    let state = &st.reference;
    let worker = async {
        let (loaded, keep) = {
            let view = state.view();
            (
                view.settings_loaded,
                view.active.as_ref().map(|a| a.revision.clone()),
            )
        };
        if loaded {
            let root = asset_root(&app);
            let _ = tokio::task::spawn_blocking(move || prune(&root, keep.as_deref())).await;
        }
        if let Some(file) = &app.cfg.referential_file {
            if let Err(e) = bundle(&app, state, file).await {
                tracing::warn!("reference sync: legacy bundle refused ({e})");
            }
        }
        let mut failures = 0;
        loop {
            let enabled = {
                let mut view = state.view();
                if view.status.enabled {
                    view.status.phase = Phase::Syncing;
                }
                view.status.enabled
            };
            if !enabled {
                failures = 0;
                state.check.notified().await;
                continue;
            }
            let delay = match round(&app, state, src.clone()).await {
                Ok(()) => {
                    failures = 0;
                    DAILY
                }
                Err(e) => {
                    tracing::warn!("reference sync: check failed ({e})");
                    let mut view = state.view();
                    if view.status.enabled {
                        view.status.phase = Phase::Error;
                    }
                    let delay = retries.get(failures).copied().unwrap_or(DAILY);
                    failures = (failures + 1).min(retries.len());
                    delay
                }
            };
            tokio::select! {
                () = tokio::time::sleep(delay) => {},
                () = state.check.notified() => { failures = 0; },
            }
        }
    };
    tokio::select! {
        () = worker => {},
        () = st.stopping() => {},
    }
}

fn prune(root: &Path, keep: Option<&str>) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().is_some_and(|name| {
                Some(name) != keep && Uuid::parse_str(name).is_ok_and(|id| id.to_string() == name)
            }) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum SyncError {
    #[error(transparent)]
    Data(#[from] RefdataError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Worker(#[from] tokio::task::JoinError),
    #[error("reference download worker exited without a result")]
    DownloadWorkerClosed,
    #[error("could not start reference download worker: {0}")]
    DownloadWorkerStart(#[source] std::io::Error),
}

struct Prepared {
    rows: Rows,
    active: Active,
    dir: PathBuf,
    /// A failed or cancelled COMMIT has an ambiguous outcome; leave the directory
    /// for next startup to reconcile against the database, rather than deleting it.
    keep: bool,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

fn stage(
    root: &Path,
    bundle_hash: Option<String>,
    prepare: impl FnOnce(&Path) -> Result<(Rows, Manifest), RefdataError>,
) -> Result<Prepared, RefdataError> {
    let revision = Uuid::new_v4().to_string();
    let dir = root.join(&revision);
    fs::create_dir_all(root).map_err(|e| load::io_err(root, e))?;
    fs::create_dir(&dir).map_err(|e| load::io_err(&dir, e))?;
    let result = (|| {
        let (rows, manifest) = prepare(&dir)?;
        let mut images = BTreeMap::new();
        for image in &rows.images {
            if manifest.files.get(&image.source) != Some(&image.sha256) {
                return Err(RefdataError::Manifest(format!(
                    "inconsistent image digest: {}",
                    image.source
                )));
            }
            let path = dir.join(&image.dest);
            load::check_file_hash(&path, &image.sha256)?;
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|f| f.sync_all())
                .map_err(|e| load::io_err(&path, e))?;
            images.insert(image.dest.clone(), image.sha256.clone());
        }
        // Windows does not support flushing directory handles this way; file contents
        // are flushed above and the completeness check repairs missing entries on restart.
        #[cfg(unix)]
        for path in [
            dir.join("heroes"),
            dir.join("battlegrounds"),
            dir.clone(),
            root.to_owned(),
            root.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_owned(),
        ] {
            if path.is_dir() {
                fs::File::open(&path)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| load::io_err(&path, e))?;
            }
        }
        Ok(Prepared {
            active: Active {
                version: rows.dataset_version.clone(),
                revision,
                images,
                bundle_hash,
            },
            rows,
            dir: dir.clone(),
            keep: false,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    result
}

async fn bundle(app: &AppState, state: &ReferenceState, file: &Path) -> Result<(), SyncError> {
    let active = state.view().active.clone();
    // An older desktop resource must never replace a managed network generation.
    if active.as_ref().is_some_and(|a| a.bundle_hash.is_none()) {
        return Ok(());
    }
    let file = file.to_owned();
    let root = asset_root(app);
    let staged = tokio::task::spawn_blocking(move || {
        let bytes = fs::read(&file).map_err(|e| load::io_err(&file, e))?;
        let hash = super::sha256_hex(&bytes);
        if active
            .as_ref()
            .is_some_and(|a| a.bundle_hash.as_ref() == Some(&hash) && a.complete(&root))
        {
            return Ok(None);
        }
        stage(&root, Some(hash), |dir| load::stage_bundle(&file, dir)).map(Some)
    })
    .await??;
    if let Some(prepared) = staged {
        activate(app, state, prepared, false).await?;
    }
    Ok(())
}

struct EnabledSource {
    st: Arc<crate::standalone::StandaloneState>,
    source: Arc<dyn load::Source>,
}

impl EnabledSource {
    fn check(&self) -> Result<(), RefdataError> {
        if !self.st.stop_requested() && self.st.reference.view().status.enabled {
            Ok(())
        } else {
            Err(RefdataError::Io("reference sync disabled".into()))
        }
    }
}

impl load::Source for EnabledSource {
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
        self.check()?;
        self.source.get(name)
    }

    fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError> {
        self.check()?;
        self.source.download(name, file)
    }
}

async fn round(
    app: &AppState,
    state: &ReferenceState,
    src: Arc<dyn load::Source>,
) -> Result<(), SyncError> {
    use load::Source;
    let Some(st) = &app.standalone else {
        return Ok(());
    };
    let src = EnabledSource {
        st: st.clone(),
        source: src,
    };
    let active = state.view().active.clone();
    let root = asset_root(app);
    // The blocking HTTP client can be in a socket read when the app quits. This thread
    // owns staging only (never the DB), so runtime shutdown need not wait for that read.
    let (send, recv) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("reference-download".into())
        .spawn(move || {
            let prepare = || {
                let manifest = parse_manifest(&src.get("manifest.json")?)?;
                if active
                    .as_ref()
                    .is_some_and(|a| a.version == manifest.dataset_version && a.complete(&root))
                {
                    return Ok(None);
                }
                let ds = Dataset::from_files(manifest, |path| src.get(load::asset_name(path)))?;
                let rows = super::build(&ds)?;
                stage(&root, None, |dir| {
                    if !rows.images.is_empty() {
                        load::fetch_images_via(
                            &src,
                            &rows.images,
                            dir,
                            &dir.join("dataset.tar.gz"),
                        )?;
                    }
                    Ok((rows, ds.manifest))
                })
                .map(Some)
            };
            let _ = send.send(prepare());
        })
        .map_err(SyncError::DownloadWorkerStart)?;
    let staged = recv.await.map_err(|_| SyncError::DownloadWorkerClosed)??;
    if let Some(prepared) = staged {
        activate(app, state, prepared, true).await?;
    } else {
        let mut view = state.view();
        view.status.phase = idle(&view.status);
    }
    Ok(())
}

async fn activate(
    app: &AppState,
    state: &ReferenceState,
    mut staged: Prepared,
    network: bool,
) -> Result<(), SyncError> {
    if network && !state.view().status.enabled {
        return Ok(());
    }
    let mut tx = app.db.begin().await?;
    load::write_rows(&mut tx, &staged.rows).await?;
    // project_match takes a ROW EXCLUSIVE lock on matches before it reads dim_talents.
    // All earlier projections read the prior committed lookup; fence them before the repair.
    sqlx::query("LOCK TABLE matches IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let repaired = crate::project::repair_reference_heroes(&mut tx).await?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, $2)
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(ACTIVE)
    .bind(sqlx::types::Json(&staged.active))
    .execute(&mut *tx)
    .await?;
    let _guard = state.control.lock().await;
    if network && !state.view().status.enabled {
        return Ok(());
    }
    staged.keep = true;
    tx.commit().await?;

    if repaired > 0 {
        crate::insights::cache::invalidate().await;
        tracing::info!(repaired, "reference sync: repaired hero attribution");
    }
    let mut view = state.view();
    view.status.version = Some(staged.active.version.clone());
    view.status.revision = staged.active.revision.clone();
    view.status.phase = Phase::Ready;
    view.active = Some(Arc::new(staged.active.clone()));
    Ok(())
}

#[cfg(test)]
mod tests;
