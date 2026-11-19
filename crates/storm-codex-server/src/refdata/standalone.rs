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
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tower_http::services::ServeFile;
use uuid::Uuid;

const ENABLED: &str = "standalone_reference_enabled";
const ACTIVE: &str = "standalone_reference_active";
const DAILY: Duration = Duration::from_secs(24 * 3600);
/// The startup hash of the active generation stands in for the first network check's own hash
/// only while it is this recent (sync can be off at startup and turned on much later).
const CHECKED_FOR: Duration = Duration::from_secs(5 * 60);
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
    /// The active generation's artwork is complete on this PC, checked locally (no network).
    artwork: bool,
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
        self.complete_unless(root, || false) == Some(true)
    }

    /// [`Active::complete`], giving up (`None`) as soon as `stop` returns true between images.
    fn complete_unless(&self, root: &Path, stop: impl Fn() -> bool) -> Option<bool> {
        let dir = self.dir(root);
        if !dir.is_dir() {
            return Some(false);
        }
        for (path, sha) in &self.images {
            if stop() {
                return None;
            }
            if load::check_file_hash(&dir.join(path), sha).is_err() {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Cheap startup check (no hashing): every listed image is a non-empty file. The worker
    /// runs [`Active::complete`] in the background before its first check relies on it.
    fn present(&self, root: &Path) -> bool {
        let dir = self.dir(root);
        !self.images.is_empty()
            && self
                .images
                .keys()
                .all(|path| fs::metadata(dir.join(path)).is_ok_and(|m| m.is_file() && m.len() > 0))
    }
}

struct View {
    status: Status,
    settings_loaded: bool,
    active: Option<Arc<Active>>,
    /// Bumped on every preference change. A network round may only fetch or activate
    /// while the epoch it started with is still current, even if sync was re-enabled since.
    epoch: u64,
    /// The worker's background hash of the active generation at startup (revision, complete,
    /// when). The first network round takes it instead of hashing the same files again, unless it
    /// is older than [`CHECKED_FOR`].
    checked: Option<(String, bool, Instant)>,
    /// Set by Retry (`POST …/refresh`); the worker consumes it before its next round and forgets
    /// a refused release, so a user retry downloads it once more. Automatic checks never set it.
    retry: bool,
}

impl View {
    fn current(&self, epoch: u64) -> bool {
        self.status.enabled && self.epoch == epoch
    }

    /// The startup hash of `revision`, if it is still recent enough to stand in for a new one.
    fn fresh_check(checked: Option<(String, bool, Instant)>, revision: &str) -> Option<bool> {
        checked
            .filter(|(rev, _, at)| rev == revision && at.elapsed() < CHECKED_FOR)
            .map(|(_, complete, _)| complete)
    }
}

pub(crate) struct ReferenceState {
    view: Mutex<View>,
    /// Serializes the final activation and preference writes, not downloads.
    control: tokio::sync::Mutex<()>,
    check: Notify,
    assets: PathBuf,
}

impl ReferenceState {
    pub(crate) fn new(cfg: &crate::config::Config) -> Self {
        Self {
            view: Mutex::new(View {
                status: Status {
                    enabled: true,
                    phase: Phase::Waiting,
                    version: None,
                    revision: Uuid::new_v4().to_string(),
                    artwork: false,
                },
                settings_loaded: false,
                active: None,
                epoch: 0,
                checked: None,
                retry: false,
            }),
            control: tokio::sync::Mutex::new(()),
            check: Notify::new(),
            assets: assets_dir(cfg),
        }
    }

    fn view(&self) -> std::sync::MutexGuard<'_, View> {
        self.view.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Publishes a full local check of generation `revision`, if it is still the active one.
    fn set_artwork(&self, revision: &str, complete: bool) {
        let mut view = self.view();
        if view.active.as_ref().is_some_and(|a| a.revision == revision) {
            view.status.artwork = complete;
        }
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
        // A restored DB can name a generation whose files were not restored with it. Startup
        // only checks presence; hashing every image would delay the server bind.
        let artwork = match &active {
            Some(active) => {
                let (active, root) = (active.clone(), self.assets.clone());
                tokio::task::spawn_blocking(move || active.present(&root))
                    .await
                    .unwrap_or(false)
            }
            None => false,
        };
        let mut view = self.view();
        view.status.artwork = artwork;
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
    if view.status.enabled != body.enabled {
        view.epoch += 1;
    }
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
    let mut view = st.reference.view();
    if !view.status.enabled {
        return error(StatusCode::CONFLICT, "reference sync is disabled".into());
    }
    view.retry = true;
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

fn assets_dir(cfg: &crate::config::Config) -> PathBuf {
    cfg.data_dir().join("reference-assets")
}

fn asset_root(app: &AppState) -> PathBuf {
    assets_dir(&app.cfg)
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
        // Startup only checked that the files exist; hash them before anything relies on it.
        // Quitting stops the hash between images rather than waiting for every file.
        let active = state.view().active.clone();
        if let Some(active) = active {
            let (generation, root, quit) = (active.clone(), asset_root(&app), st.clone());
            let hashed = tokio::task::spawn_blocking(move || {
                generation.complete_unless(&root, || quit.stop_requested())
            })
            .await;
            let complete = match hashed {
                Ok(Some(complete)) => complete,
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("reference sync: artwork check failed ({e})");
                    false
                }
            };
            state.set_artwork(&active.revision, complete);
            state.view().checked = Some((active.revision.clone(), complete, Instant::now()));
        }
        if let Some(file) = &app.cfg.referential_file {
            if let Err(e) = bundle(&app, state, file).await {
                tracing::warn!("reference sync: legacy bundle refused ({e})");
            }
        }
        let mut failures = 0;
        // In memory only: a release refused for its content (budget, shape, hash) is not
        // downloaded again while the manifest still names it. A restart, a new version or the
        // user's Retry clears it; automatic checks keep it.
        let mut refused: Option<String> = None;
        // A first hash mismatch may be transfer corruption, not the release: only a second
        // consecutive one on the same version refuses it.
        let mut hash_failed: Option<String> = None;
        // The first check stopped by `refused` warns; repeats log at debug.
        let mut refused_warned = false;
        loop {
            let enabled = {
                let mut view = state.view();
                if std::mem::take(&mut view.retry) {
                    refused = None;
                    hash_failed = None;
                }
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
            let (version, result) = round_with(&app, state, src.clone(), refused.as_deref()).await;
            if version.is_some() && version != refused {
                refused = None;
            }
            if refused.is_none() {
                refused_warned = false;
            }
            // Anything but another hash mismatch of the same version breaks the sequence; a
            // cancelled round is not a check.
            let hash_mismatch = matches!(result, Err(SyncError::Data(RefdataError::Hash { .. })));
            let cancelled = matches!(result, Err(SyncError::Superseded | SyncError::Stopped));
            if !hash_mismatch && !cancelled {
                hash_failed = None;
            }
            let delay = match result {
                Ok(()) => {
                    failures = 0;
                    DAILY
                }
                // A preference change cancelled it; the check that change queued runs next.
                Err(SyncError::Superseded) => DAILY,
                // The app is quitting: not a failure.
                Err(SyncError::Stopped) => return,
                Err(e) => {
                    match &e {
                        SyncError::Data(RefdataError::Budget { .. } | RefdataError::Shape(_)) => {
                            refused = version;
                        }
                        SyncError::Data(RefdataError::Hash { .. }) => {
                            if version.is_some() && hash_failed == version {
                                refused = version;
                                hash_failed = None;
                            } else {
                                hash_failed = version;
                            }
                        }
                        _ => {}
                    }
                    if matches!(e, SyncError::Refused(_)) && refused_warned {
                        tracing::debug!("reference sync: check failed ({e})");
                    } else {
                        refused_warned |= matches!(e, SyncError::Refused(_));
                        tracing::warn!("reference sync: check failed ({e})");
                    }
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
    #[error("reference check cancelled by a preference change")]
    Superseded,
    #[error("reference check cancelled by shutdown")]
    Stopped,
    #[error("dataset {0} was refused earlier; waiting for a newer release")]
    Refused(String),
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
    let (active, checked) = {
        let view = state.view();
        (view.active.clone(), view.checked.clone())
    };
    // An older desktop resource must never replace a managed network generation.
    if active.as_ref().is_some_and(|a| a.bundle_hash.is_none()) {
        return Ok(());
    }
    let file = file.to_owned();
    let root = asset_root(app);
    let revision = active.as_ref().map(|a| a.revision.clone());
    let (verified, staged) = tokio::task::spawn_blocking(move || {
        let hash = match load::file_sha256(&file, load::Limits::DEFAULT.archive_bytes) {
            Ok(hash) => hash,
            Err(e) => return (None, Err(e)),
        };
        // Same bundle as the active generation: only its local files need checking.
        let verified = active
            .as_ref()
            .filter(|a| a.bundle_hash.as_ref() == Some(&hash))
            .map(|a| View::fresh_check(checked, &a.revision).unwrap_or_else(|| a.complete(&root)));
        if verified == Some(true) {
            return (verified, Ok(None));
        }
        let staged = stage(&root, Some(hash), |dir| load::stage_bundle(&file, dir)).map(Some);
        (verified, staged)
    })
    .await?;
    if let (Some(revision), Some(complete)) = (&revision, verified) {
        state.set_artwork(revision, complete);
    }
    if let Some(prepared) = staged? {
        activate(app, state, prepared, None).await?;
    }
    Ok(())
}

struct EnabledSource {
    st: Arc<crate::standalone::StandaloneState>,
    source: Arc<dyn load::Source>,
    epoch: u64,
}

impl EnabledSource {
    fn check(&self) -> Result<(), RefdataError> {
        if !self.st.stop_requested() && self.st.reference.view().current(self.epoch) {
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

    fn limits(&self) -> load::Limits {
        self.source.limits()
    }
}

/// One check without refusal memory (the worker uses [`round_with`]).
#[cfg(test)]
async fn round(
    app: &AppState,
    state: &ReferenceState,
    src: Arc<dyn load::Source>,
) -> Result<(), SyncError> {
    round_with(app, state, src, None).await.1
}

/// What the download thread hands back.
struct Outcome {
    /// `datasetVersion` of the manifest, once read.
    version: Option<String>,
    /// Full local check of the active generation, when the manifest names its version.
    verified: Option<bool>,
    result: Result<Option<Prepared>, SyncError>,
}

/// One network check. Returns the manifest's `datasetVersion` (once read) so the worker can
/// remember a refused release; when the manifest still names `refused`, the round stops there.
async fn round_with(
    app: &AppState,
    state: &ReferenceState,
    src: Arc<dyn load::Source>,
    refused: Option<&str>,
) -> (Option<String>, Result<(), SyncError>) {
    use load::Source;
    let Some(st) = &app.standalone else {
        return (None, Ok(()));
    };
    let (epoch, active, checked) = {
        let mut view = state.view();
        let active = view.active.clone();
        let checked = view
            .checked
            .take()
            .zip(active.as_ref())
            .and_then(|(checked, a)| View::fresh_check(Some(checked), &a.revision));
        (view.epoch, active, checked)
    };
    let src = EnabledSource {
        st: st.clone(),
        source: src,
        epoch,
    };
    let refused = refused.map(str::to_owned);
    let root = asset_root(app);
    let generation = active.clone();
    // The blocking HTTP client can be in a socket read when the app quits. This thread
    // owns staging only (never the DB), so runtime shutdown need not wait for that read.
    let (send, recv) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("reference-download".into())
        .spawn(move || {
            let (mut version, mut verified) = (None, None);
            let result = (|| -> Result<Option<Prepared>, SyncError> {
                let manifest = parse_manifest(&src.get("manifest.json")?)?;
                version = Some(manifest.dataset_version.clone());
                if let Some(a) = generation
                    .as_ref()
                    .filter(|a| a.version == manifest.dataset_version)
                {
                    let complete = checked.unwrap_or_else(|| a.complete(&root));
                    verified = Some(complete);
                    if complete {
                        return Ok(None);
                    }
                }
                if refused.as_deref() == Some(manifest.dataset_version.as_str()) {
                    return Err(SyncError::Refused(manifest.dataset_version));
                }
                let ds = Dataset::from_files(manifest, |path| src.get(load::asset_name(path)))?;
                let rows = super::build(&ds)?;
                // `build` refuses a release without artwork, so the archive is always staged.
                let prepared = stage(&root, None, |dir| {
                    load::fetch_images_via(&src, &rows.images, dir, &dir.join("dataset.tar.gz"))?;
                    Ok((rows, ds.manifest))
                })?;
                Ok(Some(prepared))
            })();
            let _ = send.send(Outcome {
                version,
                verified,
                result,
            });
        });
    let outcome = match spawned {
        Ok(_) => recv.await.unwrap_or(Outcome {
            version: None,
            verified: None,
            result: Err(SyncError::DownloadWorkerClosed),
        }),
        Err(e) => Outcome {
            version: None,
            verified: None,
            result: Err(SyncError::DownloadWorkerStart(e)),
        },
    };
    if let (Some(active), Some(complete)) = (&active, outcome.verified) {
        state.set_artwork(&active.revision, complete);
    }
    let result = match outcome.result {
        Ok(Some(prepared)) => activate(app, state, prepared, Some(epoch)).await,
        Ok(None) => {
            let mut view = state.view();
            view.status.phase = idle(&view.status);
            Ok(())
        }
        Err(e) => Err(e),
    };
    // A round cut short by shutdown or a preference change failed because it was cancelled.
    let result = result.map_err(|e| {
        if st.stop_requested() {
            SyncError::Stopped
        } else if !state.view().current(epoch) {
            SyncError::Superseded
        } else {
            e
        }
    });
    (outcome.version, result)
}

async fn activate(
    app: &AppState,
    state: &ReferenceState,
    mut staged: Prepared,
    // The network round's epoch; a bundle activation is not tied to the preference.
    epoch: Option<u64>,
) -> Result<(), SyncError> {
    let stale = || epoch.is_some_and(|e| !state.view().current(e));
    if stale() {
        return Err(SyncError::Superseded);
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
    if stale() {
        return Err(SyncError::Superseded);
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
    view.status.artwork = true;
    view.active = Some(Arc::new(staged.active.clone()));
    Ok(())
}

#[cfg(test)]
mod tests;
