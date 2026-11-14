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
        // after the seed (projection reads the reference data); runs beside the import
        let app = self.app.clone();
        if self.st.upkeep() {
            tokio::spawn(async move { reprocess_outdated(&app).await });
        }

        let extras = load_extra_folders(&self.app.db).await;
        self.st.set_extra_folders(
            &extras
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        );
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
                    if self.st.upkeep() {
                        detect_operator_names(&self.app).await;
                    }
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

/// `operator_names` written by migration 0003 for the box's operator: on a player's PC it is not
/// their name.
const SEEDED_OPERATOR: &str = "matella";
/// `app_settings` key set once the operator names were detected (or found set by the user).
const NAMES_DETECTED_KEY: &str = "operator_names_detected";

/// What [`detect_operator_names`] writes: `Some(names)` replaces `operator_names`, `None`
/// keeps them. Only the migration seed (or nothing) is replaced, by the most frequent player.
pub(super) fn detected_names(current: &[String], top: Option<String>) -> Option<Vec<String>> {
    if !is_seeded(current) {
        return None;
    }
    top.filter(|t| !t.trim().is_empty()).map(|t| vec![t])
}

/// True when `operator_names` is still the migration seed (or empty).
fn is_seeded(current: &[String]) -> bool {
    current.is_empty() || (current.len() == 1 && current[0] == SEEDED_OPERATOR)
}

/// Once per install, after the first completed import: the "me" of every stats page is the
/// player of (almost) every imported game, as the box's onboarding expects the operator to set
/// it. Names the user already set are kept; nothing is done (and it is retried at the next
/// `ready`) while no game is imported. The user edits them on the welcome page or in Settings.
pub(super) async fn detect_operator_names(app: &AppState) {
    let done =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM app_settings WHERE key = $1)")
            .bind(NAMES_DETECTED_KEY)
            .fetch_one(&app.db)
            .await
            .unwrap_or(true);
    if done {
        return;
    }
    let current: Vec<String> = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM app_settings WHERE key = 'operator_names'",
    )
    .fetch_optional(&app.db)
    .await
    .ok()
    .flatten()
    .and_then(|v| serde_json::from_value(v).ok())
    .unwrap_or_default();
    let top: Option<String> = match sqlx::query_scalar(
        "SELECT name FROM match_players WHERE name IS NOT NULL AND name <> ''
         GROUP BY name ORDER BY count(DISTINCT match_id) DESC, name LIMIT 1",
    )
    .fetch_optional(&app.db)
    .await
    {
        Ok(top) => top,
        Err(e) => {
            tracing::warn!("operator names: detection failed: {e}");
            return;
        }
    };
    let names = detected_names(&current, top);
    if is_seeded(&current) && names.is_none() {
        return; // nothing imported yet: retried at the next `ready`
    }
    if let Err(e) = save_detected_names(&app.db, names.as_deref()).await {
        tracing::warn!("operator names: {e}");
    }
}

/// Writes the detected names (if any) and the "detected" mark in one transaction.
async fn save_detected_names(
    db: &sqlx::PgPool,
    names: Option<&[String]>,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    if let Some(names) = names {
        tracing::info!(?names, "operator names detected from the imported games");
        // conditional: a `PUT /api/admin/settings` that landed since the read is kept
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('operator_names', $1)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
             WHERE app_settings.value IN (jsonb_build_array($2::text), '[]'::jsonb)",
        )
        .bind(serde_json::json!(names))
        .bind(SEEDED_OPERATOR)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, 'true'::jsonb)
         ON CONFLICT (key) DO NOTHING",
    )
    .bind(NAMES_DETECTED_KEY)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
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
            crate::admin::reprocess_one(app, upload_id, &path).await;
        };
        tokio::select! {
            () = one => st.reprocess_step(),
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

/// `app_settings` key holding the SHA-256 (hex) of the last bundled snapshot ingested.
const SNAPSHOT_KEY: &str = "reference_snapshot_sha256";

/// Ingests the bundled snapshot (`REFERENTIAL_FILE`) when `dim_heroes` is empty or the file
/// differs from the one last ingested (an app update ships a newer one); no network and no
/// 24 h refresh in standalone mode. The hash is saved only after a successful ingest, so a
/// failed one is retried at the next start. Upserts: running it again is harmless.
pub(super) async fn seed_reference_data(app: &AppState) {
    let Some(file) = app.cfg.referential_file.clone() else {
        return;
    };
    let hashed = tokio::task::spawn_blocking({
        let file = file.clone();
        move || -> Result<String, String> {
            use sha2::{Digest, Sha256};
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok(hex::encode(Sha256::digest(&bytes)))
        }
    })
    .await;
    let hash = match hashed {
        Ok(Ok(hash)) => hash,
        Ok(Err(e)) => {
            tracing::warn!("reference data: bundled snapshot unreadable ({e})");
            return;
        }
        Err(e) => {
            tracing::warn!("reference data: hashing the snapshot failed ({e})");
            return;
        }
    };
    let empty = sqlx::query_scalar::<_, bool>("SELECT NOT EXISTS (SELECT 1 FROM dim_heroes)")
        .fetch_one(&app.db)
        .await
        .unwrap_or(false);
    let stored = sqlx::query_scalar::<_, Option<String>>(
        "SELECT value #>> '{}' FROM app_settings WHERE key = $1",
    )
    .bind(SNAPSHOT_KEY)
    .fetch_optional(&app.db)
    .await
    .ok()
    .flatten()
    .flatten();
    if !empty && stored.as_deref() == Some(hash.as_str()) {
        return;
    }
    tracing::info!(
        "reference data: ingesting the bundled snapshot {}",
        &hash[..12]
    );
    if crate::dim::ingest_snapshot_file(&app.db, &app.cfg.images_dir, &file)
        .await
        .is_err()
    {
        return;
    }
    if let Err(e) = sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, to_jsonb($2::text))
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(SNAPSHOT_KEY)
    .bind(&hash)
    .execute(&app.db)
    .await
    {
        tracing::warn!("reference data: could not record the snapshot hash ({e})");
    }
}
