//! Standalone (installer) mode: the server imports the local replay folders itself.
//!
//! Only built when `Config.standalone`. One background task ([`orchestrator`]) owns the upload
//! ledger and runs seed → folder detection → disk gate → backfill → live watch, uploading
//! through the server's own `POST /api/upload` with a token provisioned at startup
//! ([`token`]). The HTTP side ([`api`]) reads a shared [`StandaloneState`] and sends
//! [`Command`]s to the orchestrator; it never touches the ledger.

pub mod api;
mod orchestrator;
pub(crate) mod token;

#[cfg(test)]
mod tests;

pub use orchestrator::spawn;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde::Serialize;
use storm_codex_supervisor::{required_free_bytes, DiskError};
use storm_codex_watcher::backfill::Progress;
use storm_codex_watcher::detector::find_replay_dirs_under;
use storm_codex_watcher::upload::UploadResult;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{watch, Notify};

/// `app_settings` key of the extra replay folders chosen by the user (JSON array of paths).
pub const EXTRA_FOLDERS_KEY: &str = "standalone_extra_folders";

/// Import phase reported by `GET /api/standalone/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Reference data being seeded from the bundled snapshot.
    Seeding,
    /// Backfill of the replay folders in progress.
    Importing,
    /// Backfill done; the live watcher uploads new games.
    Ready,
    /// No replay folder found (game not installed, nothing chosen yet).
    NoFolders,
    /// Not enough free space for the backfill: nothing is imported.
    DiskFull,
    /// The import cannot run (token provisioning or ledger failure, see the logs).
    Error,
}

/// Requests from the HTTP side to the orchestrator.
#[derive(Debug)]
pub enum Command {
    /// Re-runs the disk gate and the backfill over the current folders.
    Rescan,
    /// New effective folder list (detected + extras): restarts the watcher, then rescans.
    SetFolders(Vec<PathBuf>),
}

/// `{file, reason}` of a rejected replay.
#[derive(Debug, Serialize)]
pub struct Rejected {
    pub file: String,
    pub reason: String,
}

/// Body of `GET /api/standalone/status` (camelCase JSON).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// Effective folders: detected HotS folders plus the replay folders found under each extra.
    pub folders: Vec<String>,
    /// Extra folders as saved by `PUT /api/standalone/folders` (empty when none): the list the
    /// settings edit.
    pub extra_folders: Vec<String>,
    pub total: usize,
    pub done: usize,
    pub duplicates: usize,
    pub rejected: Vec<Rejected>,
    /// Files that exhausted their upload retries this session (retried at the next start).
    pub failed: Vec<String>,
    /// Free bytes on the archive's disk at the last disk gate (`null` before it ran).
    pub free_bytes: Option<u64>,
    /// Bytes the backfill needs free (`2 × folders + 1 GiB`; `null` before the gate ran).
    pub needed_bytes: Option<u64>,
    pub data_dir: String,
    pub version: &'static str,
    /// Stored replays parsed by an older parser, re-processed in the background after an
    /// update (0 when none).
    pub reprocess_total: usize,
    /// Of `reprocess_total`, those already re-processed (successfully or not).
    pub reprocess_done: usize,
}

/// Outcomes of the live watcher since the current backfill run started.
#[derive(Debug, Default)]
struct LiveCounters {
    done: usize,
    duplicates: usize,
    rejected: Vec<(String, String)>,
    failed: Vec<String>,
}

#[derive(Debug)]
struct View {
    phase: Phase,
    folders: Vec<PathBuf>,
    extra_folders: Vec<String>,
    free_bytes: Option<u64>,
    needed_bytes: Option<u64>,
}

type FreeSpace = Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync>;
type Detector = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

/// State shared between the standalone routes and the orchestrator task.
pub struct StandaloneState {
    data_dir: PathBuf,
    view: Mutex<View>,
    /// Counters of the current backfill run (replaced at each run).
    progress: Mutex<Arc<Progress>>,
    live: Mutex<LiveCounters>,
    commands: UnboundedSender<Command>,
    /// Taken once by the orchestrator.
    command_rx: Mutex<Option<UnboundedReceiver<Command>>>,
    shutdown: Notify,
    /// `true` once the import task must stop (server shutting down).
    stop: watch::Sender<bool>,
    /// Port the server is bound to (checked against the `Host` header).
    port: OnceLock<u16>,
    free_space: FreeSpace,
    detector: Detector,
    /// `upload_tokens.name` of the watcher's token ([`token::TOKEN_NAME`]).
    token_name: String,
    /// Background re-process after a parser bump: (total, done).
    reprocess: Mutex<(usize, usize)>,
}

impl StandaloneState {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let (commands, command_rx) = mpsc::unbounded_channel();
        StandaloneState {
            data_dir: cfg.data_dir(),
            view: Mutex::new(View {
                phase: Phase::Seeding,
                folders: Vec::new(),
                extra_folders: Vec::new(),
                free_bytes: None,
                needed_bytes: None,
            }),
            progress: Mutex::new(Arc::new(Progress::default())),
            live: Mutex::new(LiveCounters::default()),
            commands,
            command_rx: Mutex::new(Some(command_rx)),
            shutdown: Notify::new(),
            stop: watch::Sender::new(false),
            port: OnceLock::new(),
            free_space: Arc::new(storm_codex_supervisor::free_space),
            detector: Arc::new(storm_codex_watcher::detector::detect_replay_dirs),
            token_name: token::TOKEN_NAME.to_string(),
            reprocess: Mutex::new((0, 0)),
        }
    }

    /// Test seam: free bytes reported for the archive's disk.
    #[cfg(test)]
    pub fn with_free_space(
        mut self,
        f: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    ) -> Self {
        self.free_space = Arc::new(f);
        self
    }

    /// Test seam: replaces the HotS replay folder detection.
    #[cfg(test)]
    pub fn with_detector(mut self, f: impl Fn() -> Vec<PathBuf> + Send + Sync + 'static) -> Self {
        self.detector = Arc::new(f);
        self
    }

    /// Test seam: a token name of its own, so concurrent tests sharing a database do not
    /// rotate each other's token (one server per database in production).
    #[cfg(test)]
    pub fn with_token_name(mut self, name: String) -> Self {
        self.token_name = name;
        self
    }

    pub(crate) fn token_name(&self) -> &str {
        &self.token_name
    }

    pub fn status(&self) -> Status {
        let view = lock(&self.view);
        let progress = lock(&self.progress).clone();
        let live = lock(&self.live);
        let (reprocess_total, reprocess_done) = *lock(&self.reprocess);
        let rejected = progress
            .rejected_files()
            .into_iter()
            .chain(live.rejected.iter().cloned())
            .map(|(file, reason)| Rejected { file, reason })
            .collect();
        let failed = progress
            .failed_files()
            .into_iter()
            .map(|(file, _)| file)
            .chain(live.failed.iter().cloned())
            .collect();
        Status {
            phase: view.phase,
            folders: view
                .folders
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            extra_folders: view.extra_folders.clone(),
            total: progress.total() + live.done,
            done: progress.done() + live.done,
            duplicates: progress.duplicates() + live.duplicates,
            rejected,
            failed,
            free_bytes: view.free_bytes,
            needed_bytes: view.needed_bytes,
            data_dir: self.data_dir.to_string_lossy().into_owned(),
            version: env!("CARGO_PKG_VERSION"),
            reprocess_total,
            reprocess_done,
        }
    }

    /// Starts counting a background re-process of `total` uploads.
    pub(crate) fn start_reprocess(&self, total: usize) {
        *lock(&self.reprocess) = (total, 0);
    }

    /// One upload of the background re-process handled.
    pub(crate) fn reprocess_step(&self) {
        let mut r = lock(&self.reprocess);
        r.1 = (r.1 + 1).min(r.0);
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        lock(&self.view).phase = phase;
    }

    pub(crate) fn set_folders(&self, folders: &[PathBuf]) {
        lock(&self.view).folders = folders.to_vec();
    }

    /// Records the saved extra folders shown by the status (`extraFolders`).
    /// Puts the saved extra folders in the status. `main` calls it before the server listens,
    /// so `extraFolders` is right while the import task is still seeding (the settings page
    /// builds its `PUT` from it: an empty list there would save over the user's extras).
    pub async fn load_saved_extras(&self, db: &sqlx::PgPool) {
        let extras: Vec<String> = load_extra_folders(db)
            .await
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        self.set_extra_folders(&extras);
    }

    pub(crate) fn set_extra_folders(&self, extras: &[String]) {
        lock(&self.view).extra_folders = extras.to_vec();
    }

    /// Starts a new backfill run: its progress replaces the previous one, live counts reset.
    pub(crate) fn start_run(&self, progress: Arc<Progress>) {
        *lock(&self.progress) = progress;
        *lock(&self.live) = LiveCounters::default();
    }

    /// Counts the outcome of a replay uploaded by the live watcher.
    pub(crate) fn record_live(&self, file: &str, result: &UploadResult) {
        let mut live = lock(&self.live);
        live.done += 1;
        match result {
            UploadResult::Uploaded => {}
            UploadResult::Duplicate => live.duplicates += 1,
            UploadResult::Rejected(reason) => live.rejected.push((file.into(), reason.clone())),
            UploadResult::Retryable(_) => live.failed.push(file.into()),
        }
    }

    /// Disk gate: the archive's disk must have [`required_free_bytes`] of `replay_bytes` free.
    /// The measured and needed bytes are kept for the status either way.
    pub(crate) fn disk_gate(&self, archive: &Path, replay_bytes: u64) -> Result<(), DiskError> {
        let free = (self.free_space)(archive)?;
        let needed = required_free_bytes(replay_bytes);
        {
            let mut view = lock(&self.view);
            view.free_bytes = Some(free);
            view.needed_bytes = Some(needed);
        }
        if free < needed {
            return Err(DiskError::Insufficient { free, needed });
        }
        Ok(())
    }

    pub(crate) fn detect(&self) -> Vec<PathBuf> {
        (self.detector)()
    }

    /// Queues a command for the orchestrator (`false` when it is gone).
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// The orchestrator's end of the command channel (only the first call gets it).
    pub fn take_commands(&self) -> Option<UnboundedReceiver<Command>> {
        lock(&self.command_rx).take()
    }

    /// Asks the server for a graceful shutdown (same path as Ctrl-C).
    pub fn request_shutdown(&self) {
        // `notify_one` keeps a permit when nobody waits yet: the request is never lost.
        self.shutdown.notify_one();
    }

    /// Completes when [`StandaloneState::request_shutdown`] was called.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await;
    }

    /// Tells the import task to stop: in-flight uploads are abandoned, the ledger is flushed.
    pub fn stop_import(&self) {
        self.stop.send_replace(true);
    }

    /// Completes once [`StandaloneState::stop_import`] was called (immediately if it already was).
    pub async fn stopping(&self) {
        let mut rx = self.stop.subscribe();
        // `wait_for` only fails when the sender is dropped, which `self` prevents.
        let _ = rx.wait_for(|stop| *stop).await;
    }

    /// Records the port the server is bound to (first call wins).
    pub fn set_port(&self, port: u16) {
        let _ = self.port.set(port);
    }

    pub fn port(&self) -> Option<u16> {
        self.port.get().copied()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Plain counters and lists: a panic while holding the lock cannot leave them inconsistent.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Effective folders: the detected ones plus, for each extra folder, every folder under it that
/// holds replays (the extra folder itself when none does yet, so new games are still watched).
/// Sorted, without duplicates. Blocking (walks directories).
pub(crate) fn resolve_folders(detected: Vec<PathBuf>, extras: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = detected;
    for extra in extras {
        if !extra.is_dir() {
            tracing::warn!(dir = %extra.display(), "saved replay folder no longer exists");
            continue;
        }
        let found = find_replay_dirs_under(extra);
        if found.is_empty() {
            out.push(extra.clone());
        } else {
            out.extend(found);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Total size of `files` (the replays the backfill will send). A file that vanished since it
/// was listed counts for nothing. Blocking.
pub(crate) fn replay_bytes(files: &[PathBuf]) -> u64 {
    files
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .fold(0u64, |total, meta| total.saturating_add(meta.len()))
}

/// Extra folders saved by `PUT /api/standalone/folders` (empty when unset or unreadable).
pub(crate) async fn load_extra_folders(db: &sqlx::PgPool) -> Vec<PathBuf> {
    match sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM app_settings WHERE key = $1",
    )
    .bind(EXTRA_FOLDERS_KEY)
    .fetch_optional(db)
    .await
    {
        Ok(Some(value)) => serde_json::from_value::<Vec<String>>(value)
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        Ok(None) => Vec::new(),
        Err(e) => {
            tracing::warn!(error = %e, "cannot read the saved replay folders");
            Vec::new()
        }
    }
}

pub(crate) async fn save_extra_folders(
    db: &sqlx::PgPool,
    folders: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, $2)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(EXTRA_FOLDERS_KEY)
    .bind(serde_json::json!(folders))
    .execute(db)
    .await
    .map(|_| ())
}
