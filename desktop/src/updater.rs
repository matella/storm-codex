//! In-app updates: `None -> Checking -> Downloading -> Ready(version)`.
//!
//! At startup (once the window is up) and every 6 h the shell asks the update endpoint; an
//! announced update is downloaded in the background (`Update::download`, signature checked)
//! and its bytes kept in memory. `install_update` (web banner or tray "Restart to update")
//! stops the stack, then installs those bytes (`Update::install`, NSIS `passive` mode) and
//! restarts the app. A check or download failure goes back to `None` with a log line and is
//! retried on the next tick.
//!
//! Every network/installer step goes through [`UpdateSource`] so the state machine and the
//! install flow are tested with fakes.

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;

/// Time between two update checks.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// What `update_status` answers (camelCase JSON; the web banner parses it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// `"none"`, `"downloading"` or `"ready"`.
    pub state: &'static str,
    pub version: Option<String>,
    /// Plain-text release notes of the announced update (latest.json `notes`), `None` when
    /// the manifest has none.
    pub notes: Option<String>,
}

impl UpdateStatus {
    /// No update announced.
    pub fn none() -> Self {
        Self {
            state: "none",
            version: None,
            notes: None,
        }
    }
}

/// An update announced by the endpoint.
pub struct UpdateInfo<P> {
    pub version: String,
    /// Release notes (`Update::body`), as announced.
    pub notes: Option<String>,
    /// Source-specific handle (the Tauri `Update`).
    pub pending: P,
}

/// The update channel (the Tauri updater in production). Blocking calls: they run on the
/// updater thread or a command's blocking task.
pub trait UpdateSource: Send + Sync {
    type Pending: Send;
    /// `Ok(None)` when the app is up to date.
    fn check(&self) -> Result<Option<UpdateInfo<Self::Pending>>, String>;
    /// Downloads and verifies the package.
    fn download(&self, pending: &Self::Pending) -> Result<Vec<u8>, String>;
    /// Runs the installer. On Windows a successful install exits the process (the installer
    /// relaunches the app); it returns only on other platforms or on failure.
    fn install(&self, pending: &Self::Pending, bytes: &[u8]) -> Result<(), String>;
}

/// Version and notes of the update being handled.
#[derive(Clone)]
struct Announced {
    version: String,
    notes: Option<String>,
}

impl Announced {
    fn status(&self, state: &'static str) -> UpdateStatus {
        UpdateStatus {
            state,
            version: Some(self.version.clone()),
            notes: self.notes.clone(),
        }
    }
}

enum Phase<P> {
    None,
    Checking,
    Downloading(Announced),
    Ready {
        update: Announced,
        pending: P,
        bytes: Vec<u8>,
    },
    Installing(Announced),
}

/// Why `install_update` did nothing (the stack was not touched).
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("no update is ready to install")]
    NotReady,
    #[error("an update is already being installed")]
    AlreadyInstalling,
}

/// Result of [`Updater::install`].
#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Installed (non-Windows): the caller restarts the app.
    Installed,
    /// Refused before anything was stopped.
    Refused(Refused),
    /// The stack was stopped, then the install failed: the app cannot go on as it is.
    Failed(String),
}

/// Safe to return over IPC: source errors are logged separately, never embedded here.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    #[error("an update check is already in progress")]
    Busy,
    #[error("update check failed")]
    CheckFailed,
    #[error("update download failed")]
    DownloadFailed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TickOutcome {
    Status(UpdateStatus),
    /// This tick made a new update ready; notify the tray.
    Downloaded(UpdateStatus),
}

pub struct Updater<U: UpdateSource> {
    source: U,
    phase: Mutex<Phase<U::Pending>>,
}

impl<U: UpdateSource> Updater<U> {
    pub fn new(source: U) -> Self {
        Self {
            source,
            phase: Mutex::new(Phase::None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Phase<U::Pending>> {
        self.phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn status(&self) -> UpdateStatus {
        match &*self.lock() {
            Phase::None | Phase::Checking => UpdateStatus::none(),
            Phase::Downloading(update) => update.status("downloading"),
            // Installing reads as ready: the banner button stays disabled meanwhile.
            Phase::Ready { update, .. } | Phase::Installing(update) => update.status("ready"),
        }
    }

    /// Run one cycle, or respond from the current phase without starting a second one.
    /// Return the result of this attempt, not a later snapshot of the shared state.
    pub fn tick(&self, log: &dyn Fn(&str)) -> Result<TickOutcome, CheckError> {
        {
            let mut phase = self.lock();
            match &*phase {
                Phase::None => *phase = Phase::Checking,
                Phase::Checking => return Err(CheckError::Busy),
                Phase::Downloading(update) => {
                    return Ok(TickOutcome::Status(update.status("downloading")));
                }
                Phase::Ready { update, .. } | Phase::Installing(update) => {
                    return Ok(TickOutcome::Status(update.status("ready")));
                }
            }
        }
        let info = match self.source.check() {
            Ok(Some(info)) => info,
            Ok(None) => {
                *self.lock() = Phase::None;
                return Ok(TickOutcome::Status(UpdateStatus::none()));
            }
            Err(e) => {
                log(&format!("update check failed: {e}"));
                *self.lock() = Phase::None;
                return Err(CheckError::CheckFailed);
            }
        };
        let announced = Announced {
            version: info.version,
            notes: info
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string),
        };
        *self.lock() = Phase::Downloading(announced.clone());
        match self.source.download(&info.pending) {
            Ok(bytes) => {
                log(&format!(
                    "update {} downloaded ({} bytes), ready to install",
                    announced.version,
                    bytes.len()
                ));
                let status = announced.status("ready");
                *self.lock() = Phase::Ready {
                    update: announced,
                    pending: info.pending,
                    bytes,
                };
                Ok(TickOutcome::Downloaded(status))
            }
            Err(e) => {
                log(&format!(
                    "update {} download failed: {e}",
                    announced.version
                ));
                *self.lock() = Phase::None;
                Err(CheckError::DownloadFailed)
            }
        }
    }

    /// Installs the ready update: refused unless `Ready` (one install at a time); otherwise
    /// `stop_stack` (server then Postgres, full budget), then the installer.
    pub fn install(&self, stop_stack: impl FnOnce() -> Result<(), String>) -> InstallOutcome {
        let (version, pending, bytes) = {
            let mut phase = self.lock();
            match std::mem::replace(&mut *phase, Phase::None) {
                Phase::Ready {
                    update,
                    pending,
                    bytes,
                } => {
                    let version = update.version.clone();
                    *phase = Phase::Installing(update);
                    (version, pending, bytes)
                }
                Phase::Installing(v) => {
                    *phase = Phase::Installing(v);
                    return InstallOutcome::Refused(Refused::AlreadyInstalling);
                }
                other => {
                    *phase = other;
                    return InstallOutcome::Refused(Refused::NotReady);
                }
            }
        };
        if let Err(e) = stop_stack() {
            return InstallOutcome::Failed(format!(
                "could not stop the database and server before installing {version}: {e}"
            ));
        }
        match self.source.install(&pending, &bytes) {
            Ok(()) => InstallOutcome::Installed,
            Err(e) => InstallOutcome::Failed(format!("installing {version} failed: {e}")),
        }
    }
}

/// True when `key` looks like a minisign public key as `tauri signer generate` writes it
/// (base64 of text starting with `untrusted comment:`). The placeholder shipped in
/// `tauri.conf.json` is not: updates are then disabled.
pub fn pubkey_is_usable(key: &str) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(key.trim())
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .is_some_and(|text| {
            text.starts_with("untrusted comment:")
                && text.lines().nth(1).is_some_and(|l| !l.trim().is_empty())
        })
}

/// The Tauri updater plugin as an [`UpdateSource`].
pub struct TauriSource {
    pub app: tauri::AppHandle,
}

impl UpdateSource for TauriSource {
    type Pending = tauri_plugin_updater::Update;

    fn check(&self) -> Result<Option<UpdateInfo<Self::Pending>>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let updater = self.app.updater().map_err(|e| e.to_string())?;
        let update = tauri::async_runtime::block_on(updater.check()).map_err(|e| e.to_string())?;
        Ok(update.map(|u| UpdateInfo {
            version: u.version.clone(),
            notes: u.body.clone(),
            pending: u,
        }))
    }

    fn download(&self, pending: &Self::Pending) -> Result<Vec<u8>, String> {
        tauri::async_runtime::block_on(pending.download(|_, _| {}, || {}))
            .map_err(|e| e.to_string())
    }

    fn install(&self, pending: &Self::Pending, bytes: &[u8]) -> Result<(), String> {
        pending.install(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex as StdMutex};

    #[derive(Default)]
    struct Script {
        check: Vec<Result<Option<String>, String>>,
        download: Vec<Result<Vec<u8>, String>>,
        install: Vec<Result<(), String>>,
    }

    /// A scripted source; the first check or the download can be held by the test.
    struct FakeSource {
        script: StdMutex<Script>,
        calls: Arc<StdMutex<Vec<String>>>,
        hold_check: StdMutex<Option<(Sender<()>, Receiver<()>)>>,
        hold_download: Option<(Sender<()>, StdMutex<Receiver<()>>)>,
    }

    impl FakeSource {
        fn new(script: Script, calls: &Arc<StdMutex<Vec<String>>>) -> Self {
            Self {
                script: StdMutex::new(script),
                calls: calls.clone(),
                hold_check: StdMutex::new(None),
                hold_download: None,
            }
        }
        fn call(&self, s: impl Into<String>) {
            self.calls.lock().expect("calls").push(s.into());
        }
    }

    impl UpdateSource for FakeSource {
        type Pending = String;
        fn check(&self) -> Result<Option<UpdateInfo<String>>, String> {
            self.call("check");
            let next = self.script.lock().expect("script").check.remove(0);
            let gate = self.hold_check.lock().expect("check gate").take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("entered");
                release.recv().expect("release");
            }
            // `"<version>|<notes>"` announces notes with the version.
            next.map(|v| {
                v.map(|spec| {
                    let (version, notes) = match spec.split_once('|') {
                        Some((v, n)) => (v.to_string(), Some(n.to_string())),
                        None => (spec, None),
                    };
                    UpdateInfo {
                        pending: format!("pending-{version}"),
                        version,
                        notes,
                    }
                })
            })
        }
        fn download(&self, pending: &String) -> Result<Vec<u8>, String> {
            self.call(format!("download({pending})"));
            if let Some((entered, release)) = &self.hold_download {
                entered.send(()).expect("entered");
                release.lock().expect("rx").recv().expect("release");
            }
            self.script.lock().expect("script").download.remove(0)
        }
        fn install(&self, pending: &String, bytes: &[u8]) -> Result<(), String> {
            self.call(format!("install({pending}, {} bytes)", bytes.len()));
            self.script.lock().expect("script").install.remove(0)
        }
    }

    fn calls() -> Arc<StdMutex<Vec<String>>> {
        Arc::new(StdMutex::new(Vec::new()))
    }

    fn no_log(_: &str) {}

    #[test]
    fn a_manifest_check_in_flight_does_not_start_another_or_report_up_to_date() {
        let calls = calls();
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let mut source = FakeSource::new(
            Script {
                check: vec![Ok(None), Ok(None)],
                ..Default::default()
            },
            &calls,
        );
        source.hold_check = StdMutex::new(Some((entered_tx, release_rx)));
        let updater = Arc::new(Updater::new(source));
        let ticker = {
            let updater = updater.clone();
            std::thread::spawn(move || updater.tick(&no_log))
        };
        entered_rx.recv().expect("manifest check started");
        let status = updater.status();
        let concurrent = updater.tick(&no_log);
        release_tx.send(()).expect("release");
        let finished = ticker.join().expect("join");

        assert_eq!(status, UpdateStatus::none()); // polling retains the old payload
        assert_eq!(*calls.lock().expect("calls"), ["check"]);
        assert_eq!(concurrent, Err(CheckError::Busy));
        assert_eq!(finished, Ok(TickOutcome::Status(UpdateStatus::none())));
        assert_eq!(updater.tick(&no_log), finished);
    }

    #[test]
    fn manual_check_rejects_check_and_download_failures_without_source_details() {
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![
                    Err("private endpoint detail".into()),
                    Ok(Some("0.2.0".into())),
                    Ok(None),
                ],
                download: vec![Err("private download detail".into())],
                ..Default::default()
            },
            &calls(),
        ));
        let check = updater.tick(&no_log).map_err(|e| e.to_string());
        let download = updater.tick(&no_log).map_err(|e| e.to_string());
        assert_eq!(
            [check, download],
            [
                Err("update check failed".into()),
                Err("update download failed".into())
            ],
        );
        assert_eq!(
            updater.tick(&no_log),
            Ok(TickOutcome::Status(UpdateStatus::none()))
        );
    }

    #[test]
    fn update_state_machine_none_to_downloading_to_ready() {
        let calls = calls();
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let mut source = FakeSource::new(
            Script {
                check: vec![Ok(Some("0.2.0".into()))],
                download: vec![Ok(vec![1, 2, 3])],
                ..Default::default()
            },
            &calls,
        );
        source.hold_download = Some((entered_tx, StdMutex::new(release_rx)));
        let updater = Arc::new(Updater::new(source));
        assert_eq!(updater.status(), UpdateStatus::none());
        let ticker = {
            let updater = updater.clone();
            std::thread::spawn(move || updater.tick(&no_log))
        };
        entered_rx.recv().expect("download started");
        let downloading = UpdateStatus {
            state: "downloading",
            version: Some("0.2.0".into()),
            notes: None,
        };
        assert_eq!(updater.status(), downloading);
        assert_eq!(updater.tick(&no_log), Ok(TickOutcome::Status(downloading)));
        release_tx.send(()).expect("release");
        let ready = UpdateStatus {
            state: "ready",
            version: Some("0.2.0".into()),
            notes: None,
        };
        assert_eq!(
            ticker.join().expect("join"),
            Ok(TickOutcome::Downloaded(ready.clone()))
        );
        assert_eq!(updater.status(), ready);
        // While ready, later ticks do not check again or notify the tray again.
        assert_eq!(updater.tick(&no_log), Ok(TickOutcome::Status(ready)));
        assert_eq!(
            *calls.lock().expect("calls"),
            ["check", "download(pending-0.2.0)"]
        );
        // The JSON contract of the web banner.
        let json = serde_json::to_value(updater.status()).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"state": "ready", "version": "0.2.0", "notes": null})
        );
    }

    #[test]
    fn release_notes_are_forwarded_trimmed_and_blank_notes_dropped() {
        let calls = calls();
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(Some("0.2.0|  New:\n- Faster scouting\n".into()))],
                download: vec![Ok(vec![1])],
                install: vec![Err("installer refused to start".into())],
            },
            &calls,
        ));
        updater.tick(&no_log).expect("tick");
        let json = serde_json::to_value(updater.status()).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "state": "ready",
                "version": "0.2.0",
                "notes": "New:\n- Faster scouting"
            })
        );
        // Still shown while installing.
        updater.install(|| Ok(()));
        assert_eq!(
            updater.status().notes.as_deref(),
            Some("New:\n- Faster scouting")
        );
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(Some("0.3.0| \n ".into()))],
                download: vec![Ok(vec![1])],
                ..Default::default()
            },
            &calls,
        ));
        updater.tick(&no_log).expect("tick");
        assert_eq!(updater.status().notes, None);
    }

    #[test]
    fn failures_return_to_none_with_a_log_and_are_retried_next_tick() {
        let calls = calls();
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![
                    Err("network down".into()),
                    Ok(Some("0.2.0".into())),
                    Ok(None),
                    Ok(Some("0.2.0".into())),
                ],
                download: vec![Err("bad signature".into()), Ok(vec![9])],
                ..Default::default()
            },
            &calls,
        ));
        let lines = StdMutex::new(Vec::<String>::new());
        let log = |m: &str| lines.lock().expect("lines").push(m.to_string());
        assert_eq!(updater.tick(&log), Err(CheckError::CheckFailed));
        assert_eq!(updater.status().state, "none");
        assert_eq!(updater.tick(&log), Err(CheckError::DownloadFailed));
        assert_eq!(updater.status().state, "none");
        assert_eq!(
            updater.tick(&log),
            Ok(TickOutcome::Status(UpdateStatus::none()))
        ); // up to date: no log
        assert_eq!(
            updater.tick(&log),
            Ok(TickOutcome::Downloaded(UpdateStatus {
                state: "ready",
                version: Some("0.2.0".into()),
                notes: None
            }))
        );
        assert_eq!(updater.status().state, "ready");
        let lines = lines.into_inner().expect("lines");
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("network down"));
        assert!(lines[1].contains("bad signature"));
        assert!(lines[2].contains("ready to install"));
    }

    #[test]
    fn install_is_refused_unless_ready() {
        let calls = calls();
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(None)],
                ..Default::default()
            },
            &calls,
        ));
        let stopped = StdMutex::new(false);
        let stop = || {
            *stopped.lock().expect("stopped") = true;
            Ok(())
        };
        assert_eq!(
            updater.install(stop),
            InstallOutcome::Refused(Refused::NotReady)
        );
        updater.tick(&no_log).expect("tick");
        assert_eq!(
            updater.install(|| {
                *stopped.lock().expect("stopped") = true;
                Ok(())
            }),
            InstallOutcome::Refused(Refused::NotReady)
        );
        assert!(!*stopped.lock().expect("stopped"), "the stack was touched");
        assert!(calls
            .lock()
            .expect("calls")
            .iter()
            .all(|c| !c.starts_with("install")));
        assert_eq!(
            Refused::NotReady.to_string(),
            "no update is ready to install"
        );
    }

    #[test]
    fn install_stops_the_stack_then_installs_once() {
        let calls = calls();
        let updater = Arc::new(Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(Some("0.2.0".into()))],
                download: vec![Ok(vec![7; 10])],
                install: vec![Ok(())],
            },
            &calls,
        )));
        updater.tick(&no_log).expect("tick");
        let order = calls.clone();
        let (in_stop_tx, in_stop_rx) = channel();
        let (go_tx, go_rx) = channel::<()>();
        let first = {
            let updater = updater.clone();
            std::thread::spawn(move || {
                updater.install(move || {
                    order.lock().expect("calls").push("stop".into());
                    in_stop_tx.send(()).expect("send");
                    go_rx.recv().expect("go");
                    Ok(())
                })
            })
        };
        in_stop_rx.recv().expect("stop started");
        // A second click while the first install runs is refused, nothing else is stopped.
        assert_eq!(
            updater.install(|| Err("must not be called".into())),
            InstallOutcome::Refused(Refused::AlreadyInstalling)
        );
        assert_eq!(updater.status().state, "ready");
        go_tx.send(()).expect("go");
        assert_eq!(first.join().expect("join"), InstallOutcome::Installed);
        assert_eq!(
            *calls.lock().expect("calls"),
            [
                "check",
                "download(pending-0.2.0)",
                "stop",
                "install(pending-0.2.0, 10 bytes)"
            ]
        );
    }

    #[test]
    fn install_failure_after_the_stop_is_reported_as_failed() {
        let calls = calls();
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(Some("0.2.0".into()))],
                download: vec![Ok(vec![1])],
                install: vec![Err("installer refused to start".into())],
            },
            &calls,
        ));
        updater.tick(&no_log).expect("tick");
        match updater.install(|| Ok(())) {
            InstallOutcome::Failed(msg) => {
                assert!(msg.contains("0.2.0"), "{msg}");
                assert!(msg.contains("installer refused to start"), "{msg}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        // A failed stop never reaches the installer.
        let updater = Updater::new(FakeSource::new(
            Script {
                check: vec![Ok(Some("0.3.0".into()))],
                download: vec![Ok(vec![1])],
                ..Default::default()
            },
            &calls,
        ));
        updater.tick(&no_log).expect("tick");
        assert!(matches!(
            updater.install(|| Err("Postgres did not stop".into())),
            InstallOutcome::Failed(m) if m.contains("Postgres did not stop")
        ));
        assert!(!calls
            .lock()
            .expect("calls")
            .iter()
            .any(|c| c.starts_with("install(pending-0.3.0")));
    }

    #[test]
    fn placeholder_pubkey_disables_updates() {
        assert!(!pubkey_is_usable(
            "PLACEHOLDER-UPDATER-PUBKEY-generated-by-the-operator-see-docs-spec-09-operations"
        ));
        assert!(!pubkey_is_usable(""));
        // base64 of something that is not a minisign key.
        assert!(!pubkey_is_usable("aGVsbG8gd29ybGQ="));
        // Shape written by `tauri signer generate` (dummy key material).
        let key = base64::engine::general_purpose::STANDARD.encode(
            "untrusted comment: minisign public key: 0123456789ABCDEF\nRWQBI0VniavN7wEjRWeJq83vASNFZ4mrze8BI0VniavN7wEjRWeJ\n",
        );
        assert!(pubkey_is_usable(&key));
    }

    #[test]
    fn shipped_config_has_the_updater_contract() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("conf");
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
        let up = &conf["plugins"]["updater"];
        assert_eq!(
            up["endpoints"],
            serde_json::json!([
                "https://github.com/matella/storm-codex/releases/latest/download/latest.json"
            ])
        );
        assert_eq!(up["windows"]["installMode"], "passive");
        assert!(up["pubkey"].is_string());
    }
}
