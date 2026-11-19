//! The single task that owns the upload ledger in standalone mode.
//!
//! Sequence: detect the folders (HotS folders + saved extras) →
//! open the ledger → live watcher → list the batch and check disk space → upload token →
//! backfill → loop on live watcher events and HTTP commands. The watcher starts before listing
//! the batch; the replays it reports meanwhile wait in its channel and
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
        // The status' `extraFolders` were set by `main` before listening; not re-set here, so a
        // `PUT /folders` made meanwhile is not overwritten with this older read.
        self.folders = self.resolve_current().await;

        // After the first import: the
        // re-process and the backfill would otherwise compete for `parse_sem`, and a saturated
        // pool answers uploads 202 before their parse result is known.
        let mut reprocess = self.st.upkeep();
        loop {
            if let Flow::Stop = self.import().await {
                return;
            }
            if std::mem::take(&mut reprocess) {
                let app = self.app.clone();
                tokio::spawn(async move { reprocess_outdated(&app).await });
            }
            // Live phase (or a blocked one): new replays and commands until the next import.
            loop {
                tokio::select! {
                    () = st.stopping() => return,
                    command = self.commands.recv() => match command {
                        Some(command) => {
                            self.apply(command).await;
                            break;
                        }
                        None => return,
                    },
                    Some(path) = self.events_rx.recv() => self.handle_live(&path).await,
                }
            }
        }
    }

    async fn apply(&mut self, command: Command) {
        let folders = match command {
            Command::SetFolders(folders) => folders,
            // Re-detect too: a HotS replay folder (first game, new account or region) may have
            // appeared since the last resolution.
            Command::Rescan => self.resolve_current().await,
        };
        if folders != self.folders {
            self.folders = folders;
            self.watch = None; // restarted on the new folders by `import`
        }
    }

    /// Detected HotS folders plus the saved extra folders, resolved off the runtime.
    async fn resolve_current(&self) -> Vec<PathBuf> {
        let extras = load_extra_folders(&self.app.db).await;
        let resolver = self.st.clone();
        match tokio::task::spawn_blocking(move || resolve_folders(resolver.detect(), &extras)).await
        {
            Ok(folders) => folders,
            Err(e) => {
                tracing::error!(error = %e, "replay folder detection failed");
                Vec::new()
            }
        }
    }

    /// One import attempt: ledger, disk gate, token, watcher (re)start, backfill.
    async fn import(&mut self) -> Flow {
        let st = self.st.clone();
        loop {
            if self.folders.is_empty() {
                self.st.set_folders(&self.folders);
                self.watch = None;
                self.st.set_phase(Phase::NoFolders);
                return Flow::Continue;
            }
            if let Err(e) = self.open_ledger().await {
                return self.fail(&e);
            }
            // Watch before taking the batch snapshot so arrivals after it are not lost.
            if self.watch.is_none() {
                self.restart_watch();
            }
            let (candidates, bytes) = match self.pending_batch().await {
                Ok(batch) => batch,
                Err(e) => return self.fail(&e),
            };
            let keep_ready = candidates.is_empty() && self.st.ready_for(&self.folders);
            self.st.set_folders(&self.folders);
            let blocked = match if keep_ready {
                Ok(())
            } else {
                self.st.disk_gate(&self.app.cfg.archive_dir, bytes)
            } {
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
            if blocked {
                // New games stay small: the live watcher keeps importing them.
                self.st.set_phase(Phase::DiskFull);
                return Flow::Continue;
            }

            let empty = candidates.is_empty();
            let interrupted = if empty {
                None
            } else {
                let (Some(uploader), Some(ledger)) = (self.uploader.as_ref(), self.ledger.as_mut())
                else {
                    return self.fail("import state missing");
                };
                let progress = Arc::new(Progress::new(candidates.len()));
                self.st.start_run(progress.clone());
                tokio::select! {
                    () = backfill::run_pending(candidates, uploader, ledger, &progress) => None,
                    command = self.commands.recv() => Some(command),
                    () = st.stopping() => Some(None),
                }
            };
            match interrupted {
                None => {
                    if empty && !keep_ready {
                        self.st.finish_empty_run();
                    } else {
                        self.st.set_phase(Phase::Ready);
                    }
                    return Flow::Continue;
                }
                Some(None) => return Flow::Stop,
                Some(Some(command)) => {
                    tracing::info!("import restarted on request");
                    self.apply(command).await;
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

    /// Candidate list and disk budget from the same off-runtime scan.
    async fn pending_batch(&mut self) -> Result<(Vec<PathBuf>, u64), String> {
        let Some(ledger) = self.ledger.take() else {
            return Err("upload ledger not open".into());
        };
        let dirs = self.folders.clone();
        let (ledger, candidates, bytes) = tokio::task::spawn_blocking(move || {
            let candidates = backfill::pending_files(&dirs, &ledger);
            let bytes = replay_bytes(&candidates);
            (ledger, candidates, bytes)
        })
        .await
        // The ledger is lost with the panicked task: reopened from disk at the next import.
        .map_err(|e| format!("sizing the replay folders failed: {e}"))?;
        self.ledger = Some(ledger);
        Ok((candidates, bytes))
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

    /// Uploads a replay from the current folders unless the ledger already has it. Abandoned
    /// (not recorded, retried at the next start) when a stop is requested meanwhile.
    async fn handle_live(&mut self, path: &Path) {
        // Replacing the watcher does not remove events it has already queued.
        if !self.folders.iter().any(|folder| {
            if path.starts_with(folder) {
                return true;
            }
            // notify's FSEvents backend canonicalizes roots before watching them.
            cfg!(target_os = "macos")
                && folder
                    .canonicalize()
                    .is_ok_and(|root| path.starts_with(root))
        }) {
            return;
        }
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

/// After an update that raised `PARSER_VERSION`: re-processes, in the background and bounded
/// by `parse_sem`, the stored replays parsed by an older parser (uploads, then scouting), with
/// the progress in the status (`reprocessTotal` / `reprocessDone`). Idempotent (`project_match`
/// replaces); stops between two replays on shutdown, the rest is picked up at the next start.
pub(super) async fn reprocess_outdated(app: &AppState) {
    let Some(st) = app.standalone.clone() else {
        return;
    };
    let targets = match crate::admin::outdated_uploads(&app.db).await {
        Ok(targets) => targets,
        Err(e) => {
            tracing::error!("re-process after update: selection failed: {e}");
            return;
        }
    };
    if !targets.is_empty() {
        tracing::info!(
            "re-processing {} replay(s) parsed by an older parser",
            targets.len()
        );
    }
    st.start_reprocess(targets.len());
    for (upload_id, path) in targets {
        let one = async {
            let _permit = app.parse_sem.acquire().await;
            crate::admin::reprocess_one(app, upload_id, &path).await
        };
        tokio::select! {
            ok = one => {
                if !ok {
                    crate::admin::mark_reprocess_failed(&app.db, upload_id).await;
                }
                st.reprocess_step();
            }
            () = st.stopping() => return,
        }
    }
    tokio::select! {
        n = crate::scouting::api::reprocess_stale(app) => {
            if n > 0 {
                tracing::info!("scouting re-process after update: {n} replay(s)");
            }
        }
        () = st.stopping() => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use axum::{http::StatusCode, routing::post, Router};

    #[tokio::test]
    async fn queued_events_only_upload_from_active_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("replays");
        let removed = tmp.path().join("replays-old");
        let stale = removed.join("removed.StormReplay");
        let live = kept.join("nested/kept.StormReplay");
        for path in [&stale, &live] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"replay").unwrap();
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let router = Router::new().route("/api/upload", post(|| async { StatusCode::OK }));
            axum::serve(listener, router).await.unwrap();
        });
        let app = super::super::tests::lazy_state(tmp.path(), None);
        let st = Arc::new(StandaloneState::new(&app.cfg));
        let ledger_path = app.cfg.ledger_path();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let mut worker = Orchestrator {
            app,
            st: st.clone(),
            uploader: Some(Uploader::new(base_url.clone())),
            base_url,
            ledger: Some(Ledger::open(&ledger_path).unwrap()),
            ledger_path,
            folders: vec![kept.clone(), removed],
            watch: None,
            events_tx,
            events_rx,
            commands: st.take_commands().unwrap(),
        };
        worker.events_tx.send(stale).unwrap();
        worker.events_tx.send(live).unwrap();
        worker.apply(Command::SetFolders(vec![kept])).await;
        while let Ok(path) = worker.events_rx.try_recv() {
            worker.handle_live(&path).await;
        }
        server.abort();

        let ledger = worker.ledger.unwrap();
        assert!(!ledger.contains("removed.StormReplay"));
        assert!(ledger.contains("kept.StormReplay"));
        assert_eq!(st.status().done, 1);
    }
}
