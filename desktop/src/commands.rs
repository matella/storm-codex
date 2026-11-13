//! IPC commands of the shell, callable from the main window only (capability
//! `capabilities/default.json`: origin `http://127.0.0.1:*`, these commands and nothing else),
//! plus the pure helpers behind the navigation guard and `--minimized`.
//!
//! Long commands are `async` and run their work on a blocking task: a synchronous command
//! would run on the main (UI) thread.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Url};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

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

#[tauri::command]
pub async fn export_data(app: AppHandle, path: String) -> Result<(), String> {
    blocking(move || {
        let shell = shell(&app);
        let out = if path.trim().is_empty() {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let picked = app
                .dialog()
                .file()
                .set_title("Export Storm Codex data")
                .set_file_name(backup::backup_file_name(secs))
                .add_filter("Storm Codex backup", &[backup::BACKUP_EXTENSION])
                .blocking_save_file();
            match picked {
                Some(p) => p.into_path().map_err(|e| e.to_string())?,
                None => return Err("export cancelled".into()),
            }
        } else {
            PathBuf::from(path)
        };
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
pub async fn import_data(
    app: AppHandle,
    path: String,
    confirm: Option<bool>,
) -> Result<(), String> {
    blocking(move || {
        let shell = shell(&app);
        if path.trim().is_empty() {
            return Err("no backup file given".into());
        }
        let file = PathBuf::from(path);
        shell.log(&format!("import from {}", file.display()));
        let result = shell
            .life
            .with_running(|stack| {
                backup::import_into(
                    stack,
                    &file,
                    confirm.unwrap_or(false),
                    storm_codex_supervisor::free_space,
                )
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

    /// The remote capability grants exactly the shell's own commands, to the main window,
    /// from the loopback origin: no core, plugin or unrelated permission.
    #[test]
    fn capability_grants_only_the_shell_commands() {
        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).expect("json");
        assert_eq!(cap["windows"], serde_json::json!(["main"]));
        assert_eq!(
            cap["remote"]["urls"],
            serde_json::json!(["http://127.0.0.1:*"])
        );
        let mut granted: Vec<String> = cap["permissions"]
            .as_array()
            .expect("permissions")
            .iter()
            .map(|p| p.as_str().expect("plain identifier").to_string())
            .collect();
        let mut expected: Vec<String> = APP_COMMANDS
            .iter()
            .map(|c| format!("allow-{}", c.replace('_', "-")))
            .collect();
        granted.sort();
        expected.sort();
        assert_eq!(granted, expected);
        // A single capability file: nothing else can widen the grant.
        let files: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities"))
            .expect("dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(files, ["default.json"]);
    }
}
