//! The single task that owns the upload ledger in standalone mode.
//!
//! Sequence: seed the reference data → detect the folders (HotS folders + saved extras) →
//! open the ledger → disk gate (sized on the replays the ledger does not have yet) → upload
//! token → live watcher → backfill → loop on live watcher events and HTTP commands. The watcher
//! starts right before the backfill; the replays it reports meanwhile wait in its channel and
//! are handled once the backfill is over (the ledger check skips those the backfill already
//! sent). When the disk gate blocks the backfill, the watcher still runs: new games keep being
//! imported. A command received during the backfill cancels it (the ledger is flushed on drop)
//! and the import restarts with the new folders. A stop request ([`StandaloneState::stop_import`],
//! sent on shutdown) abandons the in-flight uploads, flushes the ledger once and ends the task.
//! Every upload goes through the server's own `POST /api/upload` at the actually bound address.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use storm_codex_supervisor::DiskError;
use storm_codex_watcher::backfill::{self, Progress};
use storm_codex_watcher::ledger::{Ledger, Outcome};
use storm_codex_watcher::upload::{UploadResult, Uploader};
use storm_codex_watcher::watch::{self, WatchHandle};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use super::StandaloneState;
use super::{load_extra_folders, replay_bytes, resolve_folders, token, Command, Phase};
use crate::AppState;

/// Starts the import task. `base_url` is the server's bound address (`http://127.0.0.1:<port>`).
/// Does nothing (finished task) when `state.standalone` is unset or already started.
pub fn spawn(state: AppState, base_url: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Some(st) = state.standalone.clone() else {
            return;
        };
        let Some(commands) = st.take_commands() else {
            tracing::error!("standalone import task already started");
            return;
        };
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Orchestrator {
            ledger_path: state.cfg.ledger_path(),
            app: state,
            st,
            base_url,
            uploader: None,
            ledger: None,
            folders: Vec::new(),
            watch: None,
            events_tx,
            events_rx,
            commands,
        }
        .run()
        .await;
    })
}

struct Orchestrator {
    app: AppState,
    st: Arc<StandaloneState>,
    base_url: String,
    ledger_path: PathBuf,
    uploader: Option<Uploader>,
    ledger: Option<Ledger>,
    folders: Vec<PathBuf>,
    watch: Option<WatchHandle>,
    events_tx: UnboundedSender<PathBuf>,
    events_rx: UnboundedReceiver<PathBuf>,
    commands: UnboundedReceiver<Command>,
}

/// What the task does after an import attempt.
enum Flow {
    /// Wait for live events and commands.
    Continue,
    /// Stop requested (or the state is gone): flush and end.
    Stop,
}

impl Orchestrator {
    async fn run(mut self) {
        self.main().await;
        // Single exit path: stop watching and persist what was recorded.
        self.watch = None;
        if let Some(ledger) = self.ledger.as_mut() {
            if let Err(e) = ledger.flush_async().await {
                tracing::warn!(error = %e, "could not persist the upload ledger");
            }
        }
        tracing::info!("standalone import task stopped");
    }

    async fn main(&mut self) {
        let st = self.st.clone();
        self.st.set_phase(Phase::Seeding);
        tokio::select! {
            () = seed_reference_data(&self.app) => {},
            () = st.stopping() => return,
        }

        let extras = load_extra_folders(&self.app.db).await;
        let resolver = self.st.clone();
        self.folders =
            match tokio::task::spawn_blocking(move || resolve_folders(resolver.detect(), &extras))
                .await
            {
                Ok(folders) => folders,
                Err(e) => {
                    tracing::error!(error = %e, "replay folder detection failed");
                    Vec::new()
                }
            };

        loop {
            if let Flow::Stop = self.import().await {
                return;
            }
            // Live phase (or a blocked one): new replays and commands until the next import.
            loop {
                tokio::select! {
                    () = st.stopping() => return,
                    command = self.commands.recv() => match command {
                        Some(command) => {
                            self.apply(command);
                            break;
                        }
                        None => return,
                    },
                    Some(path) = self.events_rx.recv() => self.handle_live(&path).await,
                }
            }
        }
    }

    fn apply(&mut self, command: Command) {
        if let Command::SetFolders(folders) = command {
            self.folders = folders;
            self.watch = None; // restarted on the new folders by `import`
        }
    }

    /// One import attempt: ledger, disk gate, token, watcher (re)start, backfill.
    async fn import(&mut self) -> Flow {
        let st = self.st.clone();
        loop {
            self.st.set_folders(&self.folders);
            if self.folders.is_empty() {
                self.watch = None;
                self.st.set_phase(Phase::NoFolders);
                return Flow::Continue;
            }
            if let Err(e) = self.open_ledger().await {
                return self.fail(&e);
            }
            let bytes = match self.pending_bytes().await {
                Ok(bytes) => bytes,
                Err(e) => return self.fail(&e),
            };
            let blocked = match self.st.disk_gate(&self.app.cfg.archive_dir, bytes) {
                Ok(()) => false,
                Err(DiskError::Insufficient { free, needed }) => {
                    tracing::warn!(free, needed, "not enough free disk space: backfill blocked");
                    true
                }
                // Unmeasurable (unusual filesystem): documented, the import is not blocked.
                Err(e) => {
                    tracing::warn!(error = %e, "free disk space unknown, import not blocked");
                    false
                }
            };
            if let Err(e) = self.provision_uploader().await {
                return self.fail(&e);
            }
            self.restart_watch();
            if blocked {
                // New games stay small: the live watcher keeps importing them.
                self.st.set_phase(Phase::DiskFull);
                return Flow::Continue;
            }

            let progress = Arc::new(Progress::default());
            self.st.start_run(progress.clone());
            self.st.set_phase(Phase::Importing);
            let (Some(uploader), Some(ledger)) = (self.uploader.as_ref(), self.ledger.as_mut())
            else {
                return self.fail("import state missing");
            };
            let interrupted = tokio::select! {
                () = backfill::run(&self.folders, uploader, ledger, &progress) => None,
                command = self.commands.recv() => Some(command),
                () = st.stopping() => Some(None),
            };
            match interrupted {
                None => {
                    self.st.set_phase(Phase::Ready);
                    return Flow::Continue;
                }
                Some(None) => return Flow::Stop,
                Some(Some(command)) => {
                    tracing::info!("import restarted on request");
                    self.apply(command);
                }
            }
        }
    }

    fn fail(&mut self, error: &str) -> Flow {
        tracing::error!(%error, "standalone import cannot run");
        self.watch = None;
        self.st.set_phase(Phase::Error);
        Flow::Continue
    }

    /// Opens the ledger once (retried at the next import when it failed).
    async fn open_ledger(&mut self) -> Result<(), String> {
        if self.ledger.is_some() {
            return Ok(());
        }
        let path = self.ledger_path.clone();
        let ledger = tokio::task::spawn_blocking(move || open_ledger(&path))
            .await
            .map_err(|e| format!("upload ledger: {e}"))?
            .map_err(|e| format!("upload ledger {}: {e}", self.ledger_path.display()))?;
        self.ledger = Some(ledger);
        Ok(())
    }

    /// Size of the replays the backfill would send (those the ledger does not have yet).
    async fn pending_bytes(&mut self) -> Result<u64, String> {
        let Some(ledger) = self.ledger.take() else {
            return Err("upload ledger not open".into());
        };
        let dirs = self.folders.clone();
        let (ledger, bytes) = tokio::task::spawn_blocking(move || {
            let bytes = replay_bytes(&backfill::pending_files(&dirs, &ledger));
            (ledger, bytes)
        })
        .await
        // The ledger is lost with the panicked task: reopened from disk at the next import.
        .map_err(|e| format!("sizing the replay folders failed: {e}"))?;
        self.ledger = Some(ledger);
        Ok(bytes)
    }

    /// Provisions the upload token once (retried at the next import when it failed).
    async fn provision_uploader(&mut self) -> Result<(), String> {
        if self.uploader.is_none() {
            let token = token::provision(&self.app.db, self.st.token_name())
                .await
                .map_err(|e| format!("upload token provisioning: {e}"))?;
            self.uploader = Some(Uploader::new(self.base_url.clone()).with_token(token));
        }
        Ok(())
    }

    fn restart_watch(&mut self) {
        self.watch = None;
        let dirs: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|d| d.is_dir())
            .cloned()
            .collect();
        match watch::start(&dirs, self.events_tx.clone()) {
            Ok(handle) => self.watch = Some(handle),
            // The backfill still runs; new games are picked up by the next rescan or start.
            Err(e) => tracing::error!(error = %e, "replay watcher not started"),
        }
    }

    /// Uploads a replay reported by the watcher unless the ledger already has it. Abandoned
    /// (not recorded, retried at the next start) when a stop is requested meanwhile.
    async fn handle_live(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (Some(uploader), Some(ledger)) = (self.uploader.as_ref(), self.ledger.as_mut()) else {
            return;
        };
        if name.is_empty() || ledger.contains(&name) {
            return;
        }
        let result = tokio::select! {
            result = uploader.upload(path) => result,
            () = self.st.stopping() => return,
        };
        self.st.record_live(&name, &result);
        let outcome = match result {
            UploadResult::Uploaded => Outcome::Uploaded,
            UploadResult::Duplicate => Outcome::Duplicate,
            UploadResult::Rejected(reason) => {
                tracing::warn!(file = %name, %reason, "replay rejected by the server");
                Outcome::Rejected(reason)
            }
            UploadResult::Retryable(error) => {
                tracing::warn!(file = %name, %error, "upload failed, retried at next start");
                return;
            }
        };
        if let Err(e) = ledger.record(&name, outcome) {
            tracing::warn!(error = %e, "could not persist the upload ledger");
        }
    }
}

/// Opens the ledger; a corrupt file is moved aside (`.corrupt`) and replaced by an empty
/// ledger: the server dedups the replays sent again.
fn open_ledger(path: &Path) -> io::Result<Ledger> {
    match Ledger::open(path) {
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(".corrupt");
            tracing::warn!(error = %e, "corrupt upload ledger moved aside, starting over");
            std::fs::rename(path, PathBuf::from(aside))?;
            Ledger::open(path)
        }
        other => other,
    }
}

/// Seeds an empty `dim_heroes` once from the bundled snapshot (`REFERENTIAL_FILE`); no network
/// and no 24 h refresh in standalone mode.
async fn seed_reference_data(app: &AppState) {
    let Some(file) = app.cfg.referential_file.as_deref() else {
        return;
    };
    let empty = sqlx::query_scalar::<_, bool>("SELECT NOT EXISTS (SELECT 1 FROM dim_heroes)")
        .fetch_one(&app.db)
        .await
        .unwrap_or(false);
    if empty {
        crate::dim::ingest_snapshot_file(&app.db, &app.cfg.images_dir, file).await;
    }
}
