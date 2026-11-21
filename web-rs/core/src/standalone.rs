//! Standalone (desktop) mode: the import status served by `GET /api/standalone/status`, the welcome screen's
//! pure helpers, the desktop shell's update status (`update_status` Tauri command) and its backup commands
//! (`export_data` / `import_data`). Pure, so it is tested natively. On the box deployment the status route answers 404 and none of this is shown.

use serde::Deserialize;
use serde_json::Value;

/// Import phase (`phase` of the status, kebab-case on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Seeding,
    Importing,
    Ready,
    NoFolders,
    DiskFull,
    Error,
}

/// `{file, reason}` of a replay refused as bad content.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Rejected {
    pub file: String,
    pub reason: String,
}

/// Read-only identity from the replay projection; unknown folders have neither name nor toon.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FolderIdentity {
    pub folder: String,
    pub toon: Option<String>,
    pub name: Option<String>,
    pub aliases: Vec<String>,
    /// `true` when this toon is in the persisted confirmed set (absent from older servers).
    #[serde(default)]
    pub confirmed: bool,
}

/// One choice per verified toon, plus the manual names (which keep name-based matching).
pub fn account_options(manual: &[String], folders: &[FolderIdentity]) -> Vec<(String, String)> {
    let mut options: Vec<_> = manual
        .iter()
        .map(|name| (name.clone(), name.clone()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for folder in folders {
        if let (Some(toon), Some(name)) = (&folder.toon, &folder.name) {
            if seen.insert(toon) {
                options.push((format!("toon:{toon}"), format!("{name} ({toon})")));
            }
        }
    }
    options
}

/// Body of `GET /api/standalone/status` (camelCase on the wire, see docs/spec/04-serveur.md).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// Effective folders scanned: detected HotS folders plus the replay folders found under each extra.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Extra folders saved by the user (`PUT /api/standalone/folders`), the list the settings edit. Absent from
    /// older servers: empty.
    #[serde(default)]
    pub extra_folders: Vec<String>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub duplicates: u64,
    #[serde(default)]
    pub rejected: Vec<Rejected>,
    #[serde(default)]
    pub failed: Vec<String>,
    /// Free bytes on the archive's disk at the last disk gate (`None` before it ran).
    #[serde(default)]
    pub free_bytes: Option<u64>,
    /// Bytes the backfill needs free (`None` before the disk gate ran).
    #[serde(default)]
    pub needed_bytes: Option<u64>,
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub version: String,
    /// Stored replays parsed by an older parser, re-processed in the background after an update (0 when none or
    /// from older servers).
    #[serde(default)]
    pub reprocess_total: u64,
    #[serde(default)]
    pub reprocess_done: u64,
}

/// Standalone reference sync is independent of replay import and the app updater.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReferencePhase {
    Waiting,
    Syncing,
    Ready,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct ReferenceStatus {
    pub enabled: bool,
    pub phase: ReferencePhase,
    pub version: Option<String>,
    pub revision: String,
    /// The active dataset's artwork is complete on this PC (absent from older servers).
    #[serde(default)]
    pub artwork: bool,
}

impl ReferenceStatus {
    /// Even a disabled, syncing or failed check can have an active dataset.
    pub fn active_revision(&self) -> Option<&str> {
        self.version.as_ref().map(|_| self.revision.as_str())
    }
}

/// Key of the reads that follow reference activation. `generation` moves only when the active revision changes
/// between two known statuses: the first status (unknown → known) describes the data the mounted pages have just
/// read, so it must not refetch them, while a first activation after a known "no dataset" status still does.
/// Without a status (the box, or an older standalone server) the generation never moves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RevisionWatch {
    known: Option<Option<String>>,
    pub generation: u64,
}

impl RevisionWatch {
    /// `status` is the current status's active revision, `None` while no status is known.
    pub fn observe(&self, status: Option<Option<&str>>) -> Self {
        let Some(current) = status else {
            return self.clone();
        };
        let changed = self
            .known
            .as_ref()
            .is_some_and(|known| known.as_deref() != current);
        Self {
            known: Some(current.map(str::to_owned)),
            generation: self.generation + u64::from(changed),
        }
    }
}

/// Shared ordering of status polls and settings writes within a tab.
#[derive(Default)]
pub struct StatusRequests {
    issued: u64,
    applied: u64,
    saving: bool,
}

impl StatusRequests {
    pub fn begin_poll(&mut self) -> Option<u64> {
        if self.saving {
            return None;
        }
        self.issued += 1;
        Some(self.issued)
    }

    pub fn accept_poll(&mut self, request: u64) -> bool {
        if self.saving || request <= self.applied {
            return false;
        }
        self.applied = request;
        true
    }

    pub fn begin_save(&mut self) -> bool {
        if self.saving {
            return false;
        }
        self.saving = true;
        self.applied = self.issued; // discard every poll sent before the write
        true
    }

    pub fn finish_save(&mut self) {
        self.saving = false;
    }

    pub fn saving(&self) -> bool {
        self.saving
    }
}

/// Line shown while the server re-processes the stored replays after an update (`None` when it is not).
pub fn reprocess_message(s: &Status) -> Option<String> {
    (s.reprocess_done < s.reprocess_total).then(|| {
        format!(
            "Updating your stats for this version: {} of {} replays. Pages may show older numbers until then.",
            s.reprocess_done, s.reprocess_total
        )
    })
}

/// Share of the import done, 0..=100 (0 when there is nothing to import yet).
pub fn progress_percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    let pct = (u128::from(done) * 100 / u128::from(total)).min(100);
    u8::try_from(pct).unwrap_or(100)
}

/// One-line explanation of the phase shown on the welcome screen.
pub fn phase_message(s: &Status) -> String {
    match s.phase {
        Phase::Seeding => "Preparing the hero and talent data…".into(),
        Phase::Importing => format!(
            "Importing your replays: {} of {}. You can already browse what is imported.",
            s.done, s.total
        ),
        Phase::Ready => {
            let outcome = match (s.rejected.is_empty(), s.failed.is_empty()) {
                (true, true) => "",
                (false, true) => " Rejected replays were not imported.",
                (true, false) => " Some replays failed to import.",
                (false, false) => " Rejected replays were not imported, and some other replays failed to import.",
            };
            format!("Replay scan complete.{outcome} New games are added automatically.")
        }
        Phase::NoFolders => {
            "No replays found: add a folder that holds your Heroes of the Storm replays.".into()
        }
        Phase::DiskFull => match (s.free_bytes, s.needed_bytes) {
            (Some(free), Some(needed)) => format!(
                "Not enough free disk space to import your replays: {} free, {} needed. Free up some space, then rescan.",
                human_bytes(free),
                human_bytes(needed)
            ),
            _ => "Not enough free disk space to import your replays. Free up some space, then rescan.".into(),
        },
        Phase::Error => {
            "The import could not start. Restart Storm Codex; if it happens again, report the problem with the log files from the data folder.".into()
        }
    }
}

/// Phases that send the user to the welcome screen on the first load of the session.
pub fn redirects_to_welcome(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::Seeding | Phase::Importing | Phase::NoFolders | Phase::DiskFull
    )
}

/// Status poll period: every second while the import moves, every 10 s otherwise.
pub fn poll_ms(phase: Phase) -> u32 {
    if matches!(phase, Phase::Seeding | Phase::Importing) {
        1_000
    } else {
        10_000
    }
}

/// `1.5 GB`: binary multiples with the familiar labels (what Windows Explorer shows).
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// Folder list to `PUT` with `path` appended (trimmed). An empty path is an error shown inline; a folder already
/// in the list leaves it unchanged.
pub fn folders_with(folders: &[String], path: &str) -> Result<Vec<String>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Enter the path of a folder.".into());
    }
    let mut out = folders.to_vec();
    if !out.iter().any(|f| f == path) {
        out.push(path.to_string());
    }
    Ok(out)
}

/// Folder list to `PUT` without `path`.
pub fn folders_without(folders: &[String], path: &str) -> Vec<String> {
    folders.iter().filter(|f| *f != path).cloned().collect()
}

/// Body list of `PUT /api/standalone/folders` adding `path`: the saved extras plus `path`, never the scanned
/// (detected or expanded) folders.
pub fn extras_with(s: &Status, path: &str) -> Result<Vec<String>, String> {
    folders_with(&s.extra_folders, path)
}

/// Body list of `PUT /api/standalone/folders` removing `path` from the saved extras.
pub fn extras_without(s: &Status, path: &str) -> Vec<String> {
    folders_without(&s.extra_folders, path)
}

/// Inline message of a failed `install_update` call; `reason` is the shell's rejection.
pub fn update_failed_message(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "Update failed: unknown error".into()
    } else {
        format!("Update failed: {reason}")
    }
}

/// `error` of a `{error}` answer, or `HTTP <status>` when the body is something else.
pub fn error_message(status: u16, body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("HTTP {status}"))
}

/// State of the desktop shell's updater.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    #[default]
    None,
    Downloading,
    Ready,
}

/// Answer of the `update_status` Tauri command: `{state: "none"|"downloading"|"ready", version: string|null,
/// notes?: string|null}` (`notes`, plain-text release notes, is absent from older shells).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateStatus {
    pub state: UpdateState,
    pub version: Option<String>,
    pub notes: Option<String>,
}

/// Reads an `update_status` answer defensively: anything unexpected (other shape, unknown state) means "no update".
pub fn parse_update_status(v: &Value) -> UpdateStatus {
    let state = match v.get("state").and_then(Value::as_str) {
        Some("downloading") => UpdateState::Downloading,
        Some("ready") => UpdateState::Ready,
        _ => UpdateState::None,
    };
    let text = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    UpdateStatus {
        state,
        version: text("version"),
        notes: text("notes"),
    }
}

/// `v<version>` label shown next to the running build's version; blank (not expected, defensive) shows nothing.
pub fn version_label(version: &str) -> Option<String> {
    let v = version.trim();
    (!v.is_empty()).then(|| format!("v{v}"))
}

/// Inline message after a manual "Check for updates" completes: `None` while an update is downloading or
/// ready (its own UI already shows that), `Some` to say the app is up to date.
pub fn check_now_message(status: &UpdateStatus) -> Option<String> {
    match status.state {
        UpdateState::None => Some("You're up to date.".to_string()),
        UpdateState::Downloading | UpdateState::Ready => None,
    }
}

/// GitHub release page of a desktop version (`desktop-vX.Y.Z`); `None` unless the version is a plain
/// `X.Y.Z[-pre][+build]`-like string (it comes from the update manifest and ends up in a link).
pub fn release_page_url(version: &str) -> Option<String> {
    let ok = !version.is_empty()
        && version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    ok.then(|| format!("https://github.com/matella/storm-codex/releases/tag/desktop-v{version}"))
}

/// In-game names typed as one comma-separated field: trimmed, empty entries dropped, duplicates removed (first kept),
/// as the server stores them (`PUT /api/admin/settings` `{operator_names}`).
pub fn parse_names(input: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for n in input.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if !names.iter().any(|m| m == n) {
            names.push(n.to_string());
        }
    }
    names
}

/// The names as the field shows them.
pub fn names_text(names: &[String]) -> String {
    names.join(", ")
}

/// "Start with Windows" setting of the desktop shell: `get_autostart` (no argument, answers a boolean) and
/// `set_autostart` (`{enabled}`). Off by default (operator decision); the shell writes the per-user Run key.
pub const GET_AUTOSTART: &str = "get_autostart";
pub const SET_AUTOSTART: &str = "set_autostart";

/// Argument object of `set_autostart`.
pub fn autostart_args(enabled: bool) -> Value {
    serde_json::json!({ "enabled": enabled })
}

/// Reads the answer of `get_autostart`: `None` unless it is a boolean.
pub fn parse_autostart(v: &Value) -> Option<bool> {
    v.as_bool()
}

/// Inline message when the shell refuses to read or change the setting.
pub fn autostart_error(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "Could not change “Start with Windows”.".into()
    } else {
        format!("Could not change “Start with Windows”: {reason}")
    }
}

/// Backup commands of the desktop shell. Both take **no argument**: the shell opens the native save/open dialogs
/// and the import confirmation itself; the page only starts the command and shows how it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupAction {
    Export,
    Import,
}

impl BackupAction {
    /// Tauri command name (`window.__TAURI__.core.invoke(<name>)`, no arguments).
    pub fn command(self) -> &'static str {
        match self {
            BackupAction::Export => "export_data",
            BackupAction::Import => "import_data",
        }
    }

    /// Button text while the command runs.
    pub fn busy_label(self) -> &'static str {
        match self {
            BackupAction::Export => "Exporting…",
            BackupAction::Import => "Importing…",
        }
    }
}

/// How a backup command ended, as shown under the buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupOutcome {
    /// Finished; an import replaced the data, so the page reloads.
    Done(String),
    /// The user closed a dialog or declined the confirmation (the shell's `export cancelled` / `import cancelled`).
    Cancelled(String),
    /// Refused or failed; the text carries the shell's reason.
    Failed(String),
}

impl BackupOutcome {
    pub fn text(&self) -> &str {
        match self {
            BackupOutcome::Done(t) | BackupOutcome::Cancelled(t) | BackupOutcome::Failed(t) => t,
        }
    }
}

/// Reads the result of `export_data` / `import_data` (`Err` = the shell's rejection text).
pub fn backup_outcome(action: BackupAction, result: Result<(), &str>) -> BackupOutcome {
    let (cancelled, done, failed) = match action {
        BackupAction::Export => ("export cancelled", "✓ Backup saved.", "Export failed"),
        BackupAction::Import => (
            "import cancelled",
            "✓ Backup imported. Reloading…",
            "Import failed",
        ),
    };
    match result {
        Ok(()) => BackupOutcome::Done(done.into()),
        Err(reason) => {
            let reason = reason.trim();
            if reason.eq_ignore_ascii_case(cancelled) {
                BackupOutcome::Cancelled("Cancelled.".into())
            } else if reason.is_empty() {
                BackupOutcome::Failed(format!("{failed}: unknown error"))
            } else {
                BackupOutcome::Failed(format!("{failed}: {reason}"))
            }
        }
    }
}

/// True when the page must reload after the outcome: a finished import replaced every row the page shows.
pub fn reloads_after(action: BackupAction, outcome: &BackupOutcome) -> bool {
    action == BackupAction::Import && matches!(outcome, BackupOutcome::Done(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_revision_is_opaque_and_absent_until_activation() {
        let waiting: ReferenceStatus = serde_json::from_str(
            r#"{"enabled":true,"phase":"waiting","version":null,"revision":"empty:0"}"#,
        )
        .unwrap();
        assert_eq!(waiting.active_revision(), None);
        assert!(!waiting.artwork);

        let cached: ReferenceStatus = serde_json::from_str(
            r#"{"enabled":false,"phase":"error","version":"data-123","revision":"restored:abc","artwork":true}"#,
        )
        .unwrap();
        assert_eq!(cached.active_revision(), Some("restored:abc"));
        assert!(cached.artwork);
        assert!(!cached.enabled);
        assert_eq!(cached.phase, ReferencePhase::Error);

        assert!(serde_json::from_str::<ReferenceStatus>(
            r#"{"enabled":true,"phase":"ready","version":"data-123","revision":7}"#,
        )
        .is_err());
    }

    #[test]
    fn revision_watch_skips_the_first_status_only() {
        let first = RevisionWatch::default().observe(None);
        assert_eq!(first.generation, 0, "no status: the box never refetches");
        let polled = first.observe(Some(Some("r1")));
        assert_eq!(
            polled.generation, 0,
            "the first poll is what pages already read"
        );
        assert_eq!(polled.observe(Some(Some("r1"))).generation, 0);
        let activated = polled.observe(Some(Some("r2")));
        assert_eq!(activated.generation, 1, "a real activation refetches");
        assert_eq!(
            activated.observe(None).generation,
            1,
            "losing the status keeps the key"
        );
        assert_eq!(
            activated.observe(None).observe(Some(Some("r3"))).generation,
            2
        );

        let empty = RevisionWatch::default().observe(Some(None));
        assert_eq!(empty.generation, 0, "first poll without a dataset");
        assert_eq!(
            empty.observe(Some(Some("r1"))).generation,
            1,
            "the first download after a known empty status refetches"
        );
    }

    /// Shape of the server's `Status` (crates/storm-codex-server/src/standalone/mod.rs), as serialised.
    const FIXTURE: &str = r#"{
        "phase": "disk-full",
        "folders": ["C:\\Users\\me\\Documents\\Heroes of the Storm\\Accounts\\1\\2-Hero-1-3\\Replays\\Multiplayer", "D:\\Backup\\2023"],
        "extraFolders": ["D:\\Backup"],
        "total": 120,
        "done": 30,
        "duplicates": 4,
        "rejected": [{"file": "a.StormReplay", "reason": "unsupported build"}],
        "failed": ["b.StormReplay"],
        "freeBytes": 1073741824,
        "neededBytes": 3221225472,
        "dataDir": "C:\\Users\\me\\AppData\\Local\\StormCodex",
        "version": "0.6.0"
    }"#;

    fn status(phase: Phase) -> Status {
        let mut s: Status = serde_json::from_str(FIXTURE).unwrap();
        s.phase = phase;
        s
    }

    #[test]
    fn names_field_round_trip() {
        assert_eq!(parse_names(" Me , Alt,,me, Me "), vec!["Me", "Alt", "me"]);
        assert!(parse_names(" , ").is_empty());
        assert_eq!(names_text(&parse_names("Me,Alt")), "Me, Alt");
    }

    #[test]
    fn autostart_contract() {
        assert_eq!(GET_AUTOSTART, "get_autostart");
        assert_eq!(SET_AUTOSTART, "set_autostart");
        assert_eq!(autostart_args(true), serde_json::json!({"enabled": true}));
        assert_eq!(autostart_args(false), serde_json::json!({"enabled": false}));
        assert_eq!(parse_autostart(&serde_json::json!(true)), Some(true));
        assert_eq!(parse_autostart(&serde_json::json!(false)), Some(false));
        assert_eq!(parse_autostart(&Value::Null), None);
        assert_eq!(parse_autostart(&serde_json::json!("true")), None);
        assert_eq!(
            autostart_error(" registry denied "),
            "Could not change “Start with Windows”: registry denied"
        );
        assert_eq!(
            autostart_error(""),
            "Could not change “Start with Windows”."
        );
    }

    #[test]
    fn reprocess_message_only_while_it_runs() {
        let mut s = status(Phase::Ready);
        assert_eq!(
            (s.reprocess_total, s.reprocess_done),
            (0, 0),
            "absent from older servers"
        );
        assert_eq!(reprocess_message(&s), None);
        let v: Status =
            serde_json::from_str(r#"{"phase":"ready","reprocessTotal":40,"reprocessDone":12}"#)
                .unwrap();
        assert_eq!((v.reprocess_total, v.reprocess_done), (40, 12));
        assert_eq!(
            reprocess_message(&v).as_deref(),
            Some("Updating your stats for this version: 12 of 40 replays. Pages may show older numbers until then.")
        );
        s.reprocess_total = 40;
        s.reprocess_done = 40;
        assert_eq!(reprocess_message(&s), None, "finished");
    }

    #[test]
    fn status_deserializes_server_json() {
        let s: Status = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(s.phase, Phase::DiskFull);
        assert_eq!(s.folders.len(), 2);
        assert_eq!(s.extra_folders, vec![r"D:\Backup".to_string()]);
        assert_eq!((s.total, s.done, s.duplicates), (120, 30, 4));
        assert_eq!(
            s.rejected,
            vec![Rejected {
                file: "a.StormReplay".into(),
                reason: "unsupported build".into()
            }]
        );
        assert_eq!(s.failed, vec!["b.StormReplay".to_string()]);
        assert_eq!(s.free_bytes, Some(1 << 30));
        assert_eq!(s.needed_bytes, Some(3 << 30));
        assert_eq!(s.data_dir, "C:\\Users\\me\\AppData\\Local\\StormCodex");
        assert_eq!(s.version, "0.6.0");
        // before the first disk gate the byte counts are null; every phase name parses
        for (name, phase) in [
            ("seeding", Phase::Seeding),
            ("importing", Phase::Importing),
            ("ready", Phase::Ready),
            ("no-folders", Phase::NoFolders),
            ("disk-full", Phase::DiskFull),
            ("error", Phase::Error),
        ] {
            let json = format!(
                r#"{{"phase":"{name}","folders":[],"total":0,"done":0,"duplicates":0,"rejected":[],"failed":[],"freeBytes":null,"neededBytes":null,"dataDir":"","version":"1"}}"#
            );
            let s: Status = serde_json::from_str(&json).unwrap();
            assert_eq!(s.phase, phase);
            assert_eq!((s.free_bytes, s.needed_bytes), (None, None));
            // a server without `extraFolders` still parses: no saved extras
            assert!(s.extra_folders.is_empty());
        }
    }

    #[test]
    fn late_poll_cannot_replace_a_newer_snapshot() {
        let mut order = StatusRequests::default();
        let old = order.begin_poll().unwrap();
        let new = order.begin_poll().unwrap();
        assert!(order.accept_poll(new));
        assert!(!order.accept_poll(old));
    }

    #[test]
    fn folder_save_invalidates_inflight_polls() {
        let mut order = StatusRequests::default();
        let old = order.begin_poll().unwrap();
        assert!(order.begin_save());
        assert!(!order.accept_poll(old));
        assert!(order.begin_poll().is_none());
        assert!(!order.begin_save());
        order.finish_save();
        assert!(!order.accept_poll(old));
        let fresh = order.begin_poll().unwrap();
        assert!(order.accept_poll(fresh));
    }

    #[test]
    fn progress_percent_handles_zero_total() {
        assert_eq!(progress_percent(0, 0), 0);
        assert_eq!(progress_percent(5, 0), 0);
    }

    #[test]
    fn progress_percent_caps_at_100() {
        assert_eq!(progress_percent(30, 120), 25);
        assert_eq!(progress_percent(1, 3), 33);
        assert_eq!(progress_percent(120, 120), 100);
        assert_eq!(progress_percent(500, 120), 100);
        assert_eq!(progress_percent(u64::MAX, 1), 100);
    }

    #[test]
    fn phase_message_for_no_folders_tells_user_to_add_one() {
        let m = phase_message(&status(Phase::NoFolders));
        assert!(m.contains("No replays found"), "{m}");
        assert!(m.contains("add a folder"), "{m}");
    }

    #[test]
    fn phase_message_for_disk_full_shows_free_and_needed_space() {
        let m = phase_message(&status(Phase::DiskFull));
        assert!(m.contains("1.0 GB free"), "{m}");
        assert!(m.contains("3.0 GB needed"), "{m}");
        let mut s = status(Phase::DiskFull);
        s.free_bytes = None;
        assert!(phase_message(&s).contains("Not enough free disk space"));
        assert!(phase_message(&status(Phase::Importing)).contains("30 of 120"));
        assert!(!phase_message(&status(Phase::Error)).is_empty());
    }

    #[test]
    fn only_unfinished_phases_redirect_and_poll_fast() {
        assert!(redirects_to_welcome(Phase::Seeding));
        assert!(redirects_to_welcome(Phase::Importing));
        assert!(redirects_to_welcome(Phase::NoFolders));
        assert!(redirects_to_welcome(Phase::DiskFull));
        assert!(!redirects_to_welcome(Phase::Ready));
        assert!(!redirects_to_welcome(Phase::Error));
        assert_eq!(poll_ms(Phase::Seeding), 1_000);
        assert_eq!(poll_ms(Phase::Importing), 1_000);
        assert_eq!(poll_ms(Phase::Ready), 10_000);
        assert_eq!(poll_ms(Phase::DiskFull), 10_000);
    }

    #[test]
    fn bytes_are_shown_in_human_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_bytes(3 << 30), "3.0 GB");
        assert_eq!(human_bytes(u64::MAX), "16384.0 PB");
    }

    #[test]
    fn folders_are_added_and_removed() {
        let f = vec!["C:\\a".to_string()];
        assert_eq!(
            folders_with(&f, "  D:\\b "),
            Ok(vec!["C:\\a".to_string(), "D:\\b".to_string()])
        );
        assert_eq!(folders_with(&f, "C:\\a"), Ok(f.clone()));
        assert!(folders_with(&f, "   ").is_err());
        assert_eq!(folders_without(&f, "C:\\a"), Vec::<String>::new());
        assert_eq!(folders_without(&f, "X"), f);
    }

    #[test]
    fn folder_edits_touch_only_the_saved_extras() {
        let s = status(Phase::Ready);
        let detected = s.folders[0].clone();
        let added = extras_with(&s, r"E:\Replays").unwrap();
        assert_eq!(
            added,
            vec![r"D:\Backup".to_string(), r"E:\Replays".to_string()]
        );
        // neither the detected folder nor the expansion of the extra is sent
        assert!(!added.contains(&detected));
        assert!(!added.iter().any(|f| f == r"D:\Backup\2023"));
        assert_eq!(extras_without(&s, r"D:\Backup"), Vec::<String>::new());
        // removing a scanned (detected) folder leaves the saved extras unchanged
        assert_eq!(
            extras_without(&s, &detected),
            vec![r"D:\Backup".to_string()]
        );
        assert!(extras_with(&s, " ").is_err());
    }

    #[test]
    fn install_failures_read_as_a_message() {
        assert_eq!(
            update_failed_message("signature mismatch"),
            "Update failed: signature mismatch"
        );
        assert_eq!(update_failed_message("  "), "Update failed: unknown error");
    }

    #[test]
    fn server_errors_are_read_from_the_body() {
        assert_eq!(
            error_message(400, r#"{"error":"not an existing folder: \"X\""}"#),
            "not an existing folder: \"X\""
        );
        assert_eq!(error_message(500, "oops"), "HTTP 500");
    }

    #[test]
    fn backup_commands_take_the_shell_names() {
        assert_eq!(BackupAction::Export.command(), "export_data");
        assert_eq!(BackupAction::Import.command(), "import_data");
        assert_eq!(BackupAction::Export.busy_label(), "Exporting…");
        assert_eq!(BackupAction::Import.busy_label(), "Importing…");
    }

    #[test]
    fn backup_outcomes_read_the_shell_rejection() {
        let ex = BackupAction::Export;
        let im = BackupAction::Import;
        assert_eq!(
            backup_outcome(ex, Ok(())),
            BackupOutcome::Done("✓ Backup saved.".into())
        );
        assert_eq!(
            backup_outcome(im, Ok(())),
            BackupOutcome::Done("✓ Backup imported. Reloading…".into())
        );
        // the shell's own cancel texts are not failures
        assert_eq!(
            backup_outcome(ex, Err("export cancelled")),
            BackupOutcome::Cancelled("Cancelled.".into())
        );
        assert_eq!(
            backup_outcome(im, Err(" import cancelled ")),
            BackupOutcome::Cancelled("Cancelled.".into())
        );
        // another action's cancel text, or a longer message, is shown as a failure with the reason
        assert_eq!(
            backup_outcome(ex, Err("import cancelled")),
            BackupOutcome::Failed("Export failed: import cancelled".into())
        );
        let refused = backup_outcome(
            im,
            Err("this backup was made by a newer version of Storm Codex; update Storm Codex, then import it again"),
        );
        assert!(matches!(refused, BackupOutcome::Failed(_)));
        assert!(refused
            .text()
            .starts_with("Import failed: this backup was made by a newer version"));
        assert_eq!(
            backup_outcome(ex, Err("  ")),
            BackupOutcome::Failed("Export failed: unknown error".into())
        );
    }

    #[test]
    fn only_a_finished_import_reloads_the_page() {
        let im = BackupAction::Import;
        let ex = BackupAction::Export;
        assert!(reloads_after(im, &backup_outcome(im, Ok(()))));
        assert!(!reloads_after(ex, &backup_outcome(ex, Ok(()))));
        assert!(!reloads_after(
            im,
            &backup_outcome(im, Err("import cancelled"))
        ));
        assert!(!reloads_after(im, &backup_outcome(im, Err("disk full"))));
    }

    #[test]
    fn update_status_is_parsed_defensively() {
        let p = |s: &str| parse_update_status(&serde_json::from_str(s).unwrap());
        assert_eq!(
            p(r#"{"state":"ready","version":"1.2.0"}"#),
            UpdateStatus {
                state: UpdateState::Ready,
                version: Some("1.2.0".into()),
                notes: None
            }
        );
        assert_eq!(
            p(r#"{"state":"ready","version":"1.2.0","notes":"  New:\n- Faster\n+ 2 more (see the release page).\n"}"#)
                .notes
                .as_deref(),
            Some("New:\n- Faster\n+ 2 more (see the release page).")
        );
        assert_eq!(
            p(r#"{"state":"ready","version":"1.2.0","notes":null}"#).notes,
            None
        );
        assert_eq!(
            p(r#"{"state":"ready","version":"1.2.0","notes":"  "}"#).notes,
            None
        );
        assert_eq!(
            p(r#"{"state":"ready","version":"1.2.0","notes":["x"]}"#).notes,
            None
        );
        assert_eq!(
            p(r#"{"state":"downloading","version":null}"#).state,
            UpdateState::Downloading
        );
        assert_eq!(
            p(r#"{"state":"none","version":null}"#),
            UpdateStatus::default()
        );
        assert_eq!(p(r#"{"state":"weird"}"#), UpdateStatus::default());
        assert_eq!(p(r#"{"state":"ready","version":""}"#).version, None);
        assert_eq!(p("null"), UpdateStatus::default());
        assert_eq!(p(r#""ready""#), UpdateStatus::default());
        assert_eq!(p(r#"{"state":1,"version":2}"#), UpdateStatus::default());
    }

    #[test]
    fn version_label_formats_or_hides_blank_versions() {
        assert_eq!(version_label("0.1.3").as_deref(), Some("v0.1.3"));
        assert_eq!(version_label("  0.1.3  ").as_deref(), Some("v0.1.3"));
        assert_eq!(version_label(""), None);
        assert_eq!(version_label("   "), None);
    }

    #[test]
    fn check_now_message_only_reports_up_to_date() {
        assert_eq!(
            check_now_message(&UpdateStatus::default()),
            Some("You're up to date.".to_string())
        );
        assert_eq!(
            check_now_message(&UpdateStatus {
                state: UpdateState::Downloading,
                version: Some("1.2.0".into()),
                notes: None,
            }),
            None
        );
        assert_eq!(
            check_now_message(&UpdateStatus {
                state: UpdateState::Ready,
                version: Some("1.2.0".into()),
                notes: None,
            }),
            None
        );
    }

    #[test]
    fn release_page_url_only_for_plain_versions() {
        assert_eq!(
            release_page_url("1.2.0").as_deref(),
            Some("https://github.com/matella/storm-codex/releases/tag/desktop-v1.2.0")
        );
        assert!(release_page_url("1.2.0-rc.1").is_some());
        assert_eq!(release_page_url(""), None);
        assert_eq!(release_page_url("v1.2.0"), None);
        assert_eq!(release_page_url("1.2.0/../../x"), None);
        assert_eq!(release_page_url("1.2.0?a=b"), None);
        assert_eq!(release_page_url("1.2 .0"), None);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn account_options_keep_manual_names_and_show_only_latest_labels_for_verified_toons() {
        let folders: Vec<FolderIdentity> = serde_json::from_value(serde_json::json!([
            {"folder":"primary","toon":"2-Hero-1-10","name":"New","aliases":["New","Old"]},
            {"folder":"backup","toon":"2-Hero-1-10","name":"New","aliases":["New","Old"]},
            {"folder":"flat","toon":null,"name":null,"aliases":[]},
            {"folder":"alt","toon":"1-Hero-1-99","name":"New","aliases":["New"]}
        ]))
        .unwrap();
        assert_eq!(
            account_options(&["Manual".into()], &folders),
            [
                ("Manual".into(), "Manual".into()),
                ("toon:2-Hero-1-10".into(), "New (2-Hero-1-10)".into()),
                ("toon:1-Hero-1-99".into(), "New (1-Hero-1-99)".into()),
            ]
        );
    }
}
