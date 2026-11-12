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
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use storm_codex_supervisor::{required_free_bytes, DiskError};
use storm_codex_watcher::backfill::Progress;
use storm_codex_watcher::detector::{find_replay_dirs_under, is_storm_replay};
use storm_codex_watcher::upload::UploadResult;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;

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
    pub folders: Vec<String>,
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
    free_space: FreeSpace,
    detector: Detector,
}

impl StandaloneState {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let (commands, command_rx) = mpsc::unbounded_channel();
        StandaloneState {
            data_dir: cfg.data_dir(),
            view: Mutex::new(View {
                phase: Phase::Seeding,
                folders: Vec::new(),
                free_bytes: None,
                needed_bytes: None,
            }),
            progress: Mutex::new(Arc::new(Progress::default())),
            live: Mutex::new(LiveCounters::default()),
            commands,
            command_rx: Mutex::new(Some(command_rx)),
            shutdown: Notify::new(),
            free_space: Arc::new(storm_codex_supervisor::free_space),
            detector: Arc::new(storm_codex_watcher::detector::detect_replay_dirs),
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

    pub fn status(&self) -> Status {
        let view = lock(&self.view);
        let progress = lock(&self.progress).clone();
        let live = lock(&self.live);
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
            total: progress.total() + live.done,
            done: progress.done() + live.done,
            duplicates: progress.duplicates() + live.duplicates,
            rejected,
            failed,
            free_bytes: view.free_bytes,
            needed_bytes: view.needed_bytes,
            data_dir: self.data_dir.to_string_lossy().into_owned(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        lock(&self.view).phase = phase;
    }

    pub(crate) fn set_folders(&self, folders: &[PathBuf]) {
        lock(&self.view).folders = folders.to_vec();
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

/// Total size of the replays directly in `dirs` (what the backfill reads). Blocking.
pub(crate) fn replay_bytes(dirs: &[PathBuf]) -> u64 {
    let mut total = 0u64;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if is_storm_replay(&entry.path()) {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        total = total.saturating_add(meta.len());
                    }
                }
            }
        }
    }
    total
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
