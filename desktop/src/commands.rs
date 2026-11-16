//! IPC commands of the shell, callable from the main window only (runtime capability
//! [`main_capability`]: the exact origin `http://127.0.0.1:<port>`, these commands and nothing
//! else), plus the pure helpers behind the navigation guard, `--minimized` and the export /
//! import dialogs.
//!
//! Long commands are `async` and run their work on a blocking task: a synchronous command
//! would run on the main (UI) thread.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::ipc::CapabilityBuilder;
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::backup;
use crate::supervise::StopBudget;
use crate::updater::{InstallOutcome, UpdateStatus};
use crate::Shell;

/// Autostart is never enabled without the user's action (Settings).
pub const AUTOSTART_DEFAULT: bool = false;
/// Marker file (data root) written once [`AUTOSTART_DEFAULT`] has been applied.
pub const AUTOSTART_MARKER: &str = "autostart-initialised";

/// On the first launch with this data folder, sets autostart to [`AUTOSTART_DEFAULT`] (this
/// also clears a `Run` entry an earlier install left behind), then writes `marker`; later
/// launches keep the user's choice. Returns whether the default was applied now.
pub fn apply_autostart_default(
    marker: &std::path::Path,
    set: impl FnOnce(bool) -> Result<(), String>,
) -> Result<bool, String> {
    if marker.exists() {
        return Ok(false);
    }
    set(AUTOSTART_DEFAULT)?;
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(marker, b"").map_err(|e| format!("{}: {e}", marker.display()))?;
    Ok(true)
}

/// Turns the autostart entry (`HKCU\...\Run`, launched with [`MINIMIZED_ARG`]) on or off.
pub fn set_autostart_now(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| e.to_string())
}
/// Argument the autostart entry launches the app with: start in the tray, no window.
pub const MINIMIZED_ARG: &str = "--minimized";

/// True when the command line (program name first) asks for a tray-only start.
pub fn starts_minimized<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .skip(1)
        .any(|a| a.as_ref() == OsStr::new(MINIMIZED_ARG))
}

/// The main window may only show the local server: `http://127.0.0.1:<port>` exactly.
pub fn is_app_url(url: &Url, port: u16) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port_or_known_default() == Some(port)
        && url.username().is_empty()
        && url.password().is_none()
}

/// Links leaving the app open in the system browser, web links only (never `file:` or
/// another scheme handler).
pub fn opens_in_browser(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Navigation guard of the main window: the app origin stays in the window; anything else
/// goes to [`open_outside`].
pub fn guard_navigation(shell: &Shell, url: &Url, port: u16) -> bool {
    if is_app_url(url, port) {
        return true;
    }
    open_outside(shell, url);
    false
}

/// Web links open in the system browser; any other scheme is dropped (logged).
pub fn open_outside(shell: &Shell, url: &Url) {
    if opens_in_browser(url) {
        if let Err(e) = open::that_detached(url.as_str()) {
            shell.log(&format!("could not open {url} in the browser: {e}"));
        }
    } else {
        shell.log(&format!("blocked navigation to a {} URL", url.scheme()));
    }
}

fn shell(app: &AppHandle) -> Arc<Shell> {
    app.state::<Arc<Shell>>().inner().clone()
}

async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("internal error: {e}"))?
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    match shell(&app).updater.get() {
        Some(updater) => updater.status(),
        None => UpdateStatus {
            state: "none",
            version: None,
        },
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    blocking(move || run_install(&app)).await
}

/// The install path shared by the command and the tray item. On a failure after the stack
/// was stopped the app cannot go on: a native dialog says so, then the app exits (the next
/// launch starts the stack normally, on the old version).
pub fn run_install(app: &AppHandle) -> Result<(), String> {
    let shell = shell(app);
    let Some(updater) = shell.updater.get() else {
        return Err("updates are not available in this build".into());
    };
    let outcome = updater.install(|| {
        shell.log("installing the update: stopping the stack");
        crate::stop_stack_checked(&shell, "update", StopBudget::Full)
    });
    match outcome {
        InstallOutcome::Refused(r) => Err(r.to_string()),
        InstallOutcome::Installed => {
            shell.log("update installed, restarting");
            app.restart();
        }
        InstallOutcome::Failed(e) => {
            shell.log(&format!("update failed: {e}"));
            for window in app.webview_windows().values() {
                let _ = window.hide();
            }
            app.dialog()
                .message(format!(
                    "The update could not be installed.\n\n{e}\n\nStorm Codex has stopped its \
                     database and server and will now close. Start it again to keep using the \
                     current version; your data is unchanged."
                ))
                .kind(MessageDialogKind::Error)
                .title(crate::TITLE)
                .blocking_show();
            app.exit(1);
            Err(e)
        }
    }
}

/// The native dialogs export and import open (fakes in tests). The page passes no path and
/// no confirmation: the user answers these dialogs, shown by the shell.
pub trait Dialogs {
    /// Save dialog (Windows asks itself before replacing an existing file); `None` if cancelled.
    fn save_backup(&self, default_name: &str) -> Option<PathBuf>;
    /// Open dialog; `None` if cancelled.
    fn open_backup(&self) -> Option<PathBuf>;
    /// OK / Cancel question; true on OK.
    fn confirm(&self, title: &str, message: &str) -> bool;
}

pub struct TauriDialogs<'a>(pub &'a AppHandle);

impl Dialogs for TauriDialogs<'_> {
    fn save_backup(&self, default_name: &str) -> Option<PathBuf> {
        self.0
            .dialog()
            .file()
            .set_title("Export Storm Codex data")
            .set_file_name(default_name)
            .add_filter("Storm Codex backup", &[backup::BACKUP_EXTENSION])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    }
    fn open_backup(&self) -> Option<PathBuf> {
        self.0
            .dialog()
            .file()
            .set_title("Import Storm Codex data")
            .add_filter("Storm Codex backup", &[backup::BACKUP_EXTENSION])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
    }
    fn confirm(&self, title: &str, message: &str) -> bool {
        self.0
            .dialog()
            .message(message)
            .kind(MessageDialogKind::Warning)
            .title(title)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Import".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    }
}

/// Where the export goes: the save dialog's answer (default name for `now_secs`), a local path.
pub fn choose_export_target(d: &dyn Dialogs, now_secs: u64) -> Result<PathBuf, String> {
    let out = d
        .save_backup(&backup::backup_file_name(now_secs))
        .ok_or_else(|| "export cancelled".to_string())?;
    backup::check_local_path(&out)?;
    Ok(out)
}

/// The backup to import: the open dialog's answer (a local path), then an explicit OK in a
/// confirmation dialog that says what is replaced.
pub fn choose_import_source(d: &dyn Dialogs, has_replays: bool) -> Result<PathBuf, String> {
    let file = d
        .open_backup()
        .ok_or_else(|| "import cancelled".to_string())?;
    backup::check_local_path(&file)?;
    let message = format!(
        "Import {}?\n\nThis replaces all Storm Codex data on this PC (database and replay \
         folder){}. The server is stopped during the import.",
        file.display(),
        if has_replays {
            ", including the replays already imported here"
        } else {
            ""
        }
    );
    if !d.confirm(crate::TITLE, &message) {
        return Err("import cancelled".into());
    }
    Ok(file)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn export_data(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        let shell = shell(&app);
        let out = choose_export_target(&TauriDialogs(&app), now_secs())?;
        shell.log(&format!("export to {}", out.display()));
        let result = shell
            .life
            .with_running(|stack| backup::export_stack(stack, &out))
            .ok_or_else(|| "Storm Codex is not running".to_string())?;
        match result {
            Ok(c) => {
                shell.log(&format!(
                    "export done: {} replays, {} bytes",
                    c.archive_files, c.total_bytes
                ));
                Ok(())
            }
            Err(e) => {
                shell.log(&format!("export failed: {e}"));
                Err(e.to_string())
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn import_data(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        let shell = shell(&app);
        // A short look (the dialogs are shown outside the lease: a Quit is not held up).
        let has_replays = shell
            .life
            .with_running(|stack| backup::has_files(&stack.data().archive()))
            .ok_or_else(|| "Storm Codex is not running".to_string())?;
        let file = choose_import_source(&TauriDialogs(&app), has_replays)?;
        shell.log(&format!("import from {}", file.display()));
        let result = shell
            .life
            .with_running(|stack| {
                backup::import_into(stack, &file, storm_codex_supervisor::free_space)
            })
            .ok_or_else(|| "Storm Codex is not running".to_string())?;
        match result {
            Ok(c) => {
                shell.log(&format!("import done: {} replays", c.archive_files));
                Ok(())
            }
            Err(e) => {
                shell.log(&format!("import failed: {e}"));
                Err(e)
            }
        }
    })
    .await
}

/// Identifier of the main window's runtime capability.
pub const MAIN_CAPABILITY: &str = "main-window-commands";

/// The one origin the IPC trusts: the local server, exact port.
pub fn app_origin(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// `allow-<command>` for each of the shell's commands (generated by `build.rs`).
pub fn app_permissions() -> Vec<String> {
    crate::command_names::APP_COMMANDS
        .iter()
        .map(|c| format!("allow-{}", c.replace('_', "-")))
        .collect()
}

/// The main window's capability, added at runtime once the server port is known: remote
/// origin [`app_origin`] only (no wildcard port: a page served on another local port gets no
/// IPC), not for local app assets, the shell's commands only.
pub fn main_capability(window: &str, port: u16) -> CapabilityBuilder {
    app_permissions().into_iter().fold(
        CapabilityBuilder::new(MAIN_CAPABILITY)
            .remote(app_origin(port))
            .local(false)
            .window(window),
        |cap, p| cap.permission(p),
    )
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    set_autostart_now(&app, enabled)?;
    shell(&app).log(&format!("autostart {}", if enabled { "on" } else { "off" }));
    Ok(())
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::command_names::APP_COMMANDS;
    use std::path::Path;

    #[test]
    fn autostart_default_is_off() {
        assert!(!std::hint::black_box(AUTOSTART_DEFAULT));
        let tmp = tempfile::tempdir().expect("tempdir");
        let marker = tmp.path().join(AUTOSTART_MARKER);
        let mut applied = Vec::new();
        assert_eq!(
            apply_autostart_default(&marker, |on| {
                applied.push(on);
                Ok(())
            }),
            Ok(true)
        );
        assert_eq!(applied, [false], "first launch turns autostart off");
        // Later launches keep the user's choice.
        assert_eq!(
            apply_autostart_default(&marker, |_| panic!("must not be called")),
            Ok(false)
        );
        // A failure is retried next launch (no marker written).
        let other = tmp.path().join("other-marker");
        assert!(apply_autostart_default(&other, |_| Err("registry".into())).is_err());
        assert!(!other.exists());
    }

    #[test]
    fn minimized_flag_is_parsed_from_the_command_line() {
        assert!(starts_minimized(["app.exe", "--minimized"]));
        assert!(starts_minimized(["app.exe", "--other", "--minimized"]));
        assert!(!starts_minimized(["app.exe"]));
        assert!(
            !starts_minimized(["--minimized"]),
            "the program name is not a flag"
        );
        assert!(!starts_minimized([
            "app.exe",
            "--minimized=1",
            "-minimized"
        ]));
    }

    fn url(s: &str) -> Url {
        s.parse().expect("url")
    }

    #[test]
    fn only_the_local_server_origin_stays_in_the_window() {
        assert!(is_app_url(&url("http://127.0.0.1:8088/"), 8088));
        assert!(is_app_url(
            &url("http://127.0.0.1:8088/matches/42?x=1#t"),
            8088
        ));
        for bad in [
            "http://127.0.0.1:8089/",
            "https://127.0.0.1:8088/",
            "http://localhost:8088/",
            "http://127.0.0.2:8088/",
            "http://user@127.0.0.1:8088/",
            "http://evil.example/",
            "file:///C:/Windows/win.ini",
            "tauri://localhost/",
        ] {
            assert!(!is_app_url(&url(bad), 8088), "{bad}");
        }
        assert!(opens_in_browser(&url("https://github.com/matella")));
        assert!(opens_in_browser(&url("http://example.com/")));
        for bad in [
            "file:///C:/x.exe",
            "ms-settings:privacy",
            "javascript:alert(1)",
        ] {
            assert!(!opens_in_browser(&url(bad)), "{bad}");
        }
    }

    /// The capability grants exactly the shell's own commands, from the exact app origin: no
    /// other port, host or scheme, and no capability file can widen it.
    #[test]
    fn capability_is_the_exact_origin_and_the_shell_commands_only() {
        use tauri::utils::acl::RemoteUrlPattern;
        let mut granted = app_permissions();
        let mut expected: Vec<String> = APP_COMMANDS
            .iter()
            .map(|c| format!("allow-{}", c.replace('_', "-")))
            .collect();
        granted.sort();
        expected.sort();
        assert_eq!(granted, expected);
        let pattern: RemoteUrlPattern = app_origin(8088).parse().expect("pattern");
        for ok in [
            "http://127.0.0.1:8088/",
            "http://127.0.0.1:8088/matches/42?x=1#t",
        ] {
            assert!(pattern.test(&url(ok)), "{ok}");
        }
        for bad in [
            "http://127.0.0.1:8089/",
            "http://127.0.0.1:5173/",
            "https://127.0.0.1:8088/",
            "http://localhost:8088/",
            "http://127.0.0.2:8088/",
        ] {
            assert!(!pattern.test(&url(bad)), "{bad}");
        }
        // No static capability file: the runtime one is the only grant.
        assert!(!Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities")).exists());
    }

    struct FakeDialogs {
        save: Option<PathBuf>,
        open: Option<PathBuf>,
        ok: bool,
        asked: std::cell::RefCell<Vec<String>>,
    }

    impl Dialogs for FakeDialogs {
        fn save_backup(&self, default_name: &str) -> Option<PathBuf> {
            self.asked.borrow_mut().push(format!("save {default_name}"));
            self.save.clone()
        }
        fn open_backup(&self) -> Option<PathBuf> {
            self.asked.borrow_mut().push("open".into());
            self.open.clone()
        }
        fn confirm(&self, _title: &str, message: &str) -> bool {
            self.asked.borrow_mut().push(format!("confirm {message}"));
            self.ok
        }
    }

    fn dialogs(save: Option<&Path>, open: Option<&Path>, ok: bool) -> FakeDialogs {
        FakeDialogs {
            save: save.map(Path::to_path_buf),
            open: open.map(Path::to_path_buf),
            ok,
            asked: Default::default(),
        }
    }

    #[test]
    fn export_target_comes_from_the_save_dialog_and_must_be_local() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("b.stormcodex");
        let d = dialogs(Some(&out), None, true);
        assert_eq!(choose_export_target(&d, 0), Ok(out));
        assert_eq!(
            *d.asked.borrow(),
            ["save storm-codex-backup-1970-01-01.stormcodex"]
        );
        assert_eq!(
            choose_export_target(&dialogs(None, None, true), 0),
            Err("export cancelled".into())
        );
        let unc = Path::new(r"\\server\share\b.stormcodex");
        assert!(choose_export_target(&dialogs(Some(unc), None, true), 0).is_err());
    }

    #[test]
    fn import_source_needs_the_open_dialog_and_an_explicit_ok() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("in.stormcodex");
        let d = dialogs(None, Some(&file), true);
        assert_eq!(choose_import_source(&d, true), Ok(file.clone()));
        let asked = d.asked.borrow();
        assert_eq!(asked[0], "open");
        assert!(asked[1].contains("replaces all"), "{}", asked[1]);
        assert!(
            asked[1].contains("replays already imported"),
            "{}",
            asked[1]
        );
        // Always confirmed, even without replays yet.
        let d = dialogs(None, Some(&file), true);
        choose_import_source(&d, false).expect("ok");
        assert_eq!(d.asked.borrow().len(), 2);
        // Cancel at either step.
        assert_eq!(
            choose_import_source(&dialogs(None, Some(&file), false), true),
            Err("import cancelled".into())
        );
        assert_eq!(
            choose_import_source(&dialogs(None, None, true), true),
            Err("import cancelled".into())
        );
        // A network path is refused before any question.
        let unc = Path::new(r"\\server\share\in.stormcodex");
        let d = dialogs(None, Some(unc), true);
        assert!(choose_import_source(&d, true).is_err());
        assert_eq!(*d.asked.borrow(), ["open"]);
    }
}
