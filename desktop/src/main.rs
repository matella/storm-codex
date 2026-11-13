//! Storm Codex desktop shell (Tauri v2): one instance per user, supervises the bundled
//! Postgres and `storm-codex-server` ([`supervise::Stack`]), shows the app in a window on
//! `http://127.0.0.1:<port>/` and lives in the tray. Closing the window hides it (the server
//! keeps running for OBS); Quit, app exit and Windows session end stop the stack.

// No console window in release builds (debug keeps it for logs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod job;
mod lifecycle;
mod session;
mod supervise;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use storm_codex_supervisor::DataDir;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::lifecycle::{Lifecycle, StopOutcome};
use crate::supervise::{Stack, StopBudget};

const MAIN: &str = "main";
const LOADING: &str = "loading";
const TITLE: &str = "Storm Codex";

/// State shared by the startup thread, the tray, the exit hooks and the session watcher.
pub struct Shell {
    /// Start/stop handshake: stop once, every caller waits for its end.
    life: Lifecycle<Stack>,
    logs: OnceLock<PathBuf>,
}

impl Shell {
    fn new() -> Self {
        Self {
            life: Lifecycle::new(),
            logs: OnceLock::new(),
        }
    }

    fn log(&self, msg: &str) {
        let Some(dir) = self.logs.get() else {
            eprintln!("{msg}");
            return;
        };
        log_to(dir, msg);
    }
}

fn log_to(dir: &Path, msg: &str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let line = format!("[{secs}] {msg}\n");
    eprint!("{line}");
    let _ = std::fs::create_dir_all(dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("desktop.log"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Stops the stack (idempotent). Returns only once the stack is fully stopped, whichever
/// caller ran the stop; an in-progress start is waited for, then stopped.
pub fn stop_stack(shell: &Shell, why: &str, budget: StopBudget) {
    match shell.life.stop(budget) {
        StopOutcome::Stopped(used, Ok(())) => {
            shell.log(&format!("stopped ({why}, {used:?} budget)"))
        }
        StopOutcome::Stopped(used, Err(e)) => {
            shell.log(&format!("stop failed ({why}, {used:?} budget): {e}"))
        }
        StopOutcome::NothingRunning => {}
    }
}

fn main() {
    let shell = Arc::new(Shell::new());

    let mut builder = tauri::Builder::default();
    // Registered first (plugin requirement). A second launch only focuses this instance: two
    // shells on one data folder would fight over Postgres (`Postgres::start` restarts a
    // running cluster).
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }));
    }
    let setup_shell = shell.clone();
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // The close button hides to the tray; the server keeps running (OBS sources).
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            create_loading_window(app.handle())?;
            create_tray(app.handle(), setup_shell.clone())?;
            #[cfg(windows)]
            session::watch_session_end(setup_shell.clone());
            let handle = app.handle().clone();
            let shell = setup_shell.clone();
            std::thread::spawn(move || startup(handle, shell));
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Storm Codex could not start: {e}");
            std::process::exit(1);
        }
    };
    app.run(move |_handle, event| match event {
        // No window left (none is ever closed, only hidden): stay in the tray.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        // Also reached at Windows session end (the event loop exits on WM_ENDSESSION): the
        // lifecycle then switches to the short budget and waits for the watcher's stop.
        RunEvent::ExitRequested { .. } => stop_stack(&shell, "exit requested", StopBudget::Full),
        RunEvent::Exit => stop_stack(&shell, "exit", StopBudget::Full),
        _ => {}
    });
}

/// Resolves the folders, starts the stack, then swaps the loading window for the app window.
/// Any failure is shown in a native dialog naming the log folder, and the app exits.
fn startup(app: AppHandle, shell: Arc<Shell>) {
    let result = (|| -> Result<Stack, String> {
        let data_root = data_root(&app)?;
        let logs = DataDir::new(data_root.clone()).logs();
        let _ = shell.logs.set(logs);
        let resources = resource_dir(&app)?;
        shell.log(&format!(
            "starting: resources {}, data {}",
            resources.display(),
            data_root.display()
        ));
        Stack::start(&resources, &data_root).map_err(|e| e.to_string())
    })();
    let (stack, failure) = match result {
        Ok(stack) => (Some(stack), None),
        Err(e) => (None, Some(e)),
    };
    let port = stack.as_ref().map(Stack::port);
    if !shell.life.finish_start(stack) {
        return; // Quit or session end meanwhile: that caller stops what was started.
    }
    let opened = match (port, failure) {
        (Some(port), _) => {
            shell.log(&format!("server healthy on 127.0.0.1:{port}"));
            open_main_window(&app, port).map_err(|e| format!("could not open the window: {e}"))
        }
        (None, failure) => Err(failure.unwrap_or_else(|| "unknown error".into())),
    };
    if let Err(e) = opened {
        shell.log(&format!("startup failed: {e}"));
        stop_stack(&shell, "startup failed", StopBudget::Full);
        let logs = shell
            .logs
            .get()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".into());
        app.dialog()
            .message(format!(
                "Storm Codex could not start.\n\n{e}\n\nLog files: {logs}"
            ))
            .kind(MessageDialogKind::Error)
            .title(TITLE)
            .blocking_show();
        app.exit(1);
    }
}

/// `app_local_data_dir()` (covered by the uninstaller's "delete app data" option). Debug
/// builds accept `STORM_CODEX_DATA_ROOT` to keep dev runs out of the real profile.
fn data_root(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("STORM_CODEX_DATA_ROOT") {
        return Ok(PathBuf::from(dir));
    }
    app.path()
        .app_local_data_dir()
        .map_err(|e| format!("no local app data folder: {e}"))
}

/// The bundle's resource folder. Debug builds accept `STORM_CODEX_RESOURCE_DIR` (a folder with
/// the server binary, `postgres/`, `web/`, `minimaps/`).
fn resource_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("STORM_CODEX_RESOURCE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    app.path()
        .resource_dir()
        .map_err(|e| format!("no resource folder: {e}"))
}

fn create_loading_window(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LOADING, WebviewUrl::App("loading.html".into()))
        .title(TITLE)
        .inner_size(420.0, 260.0)
        .resizable(false)
        .center()
        .build()?;
    Ok(())
}

/// Opens the app window on the local server and closes the loading window. The new window
/// stays hidden if the user had hidden the loading window to the tray.
fn open_main_window(app: &AppHandle, port: u16) -> Result<(), String> {
    let url: tauri::Url = format!("http://127.0.0.1:{port}/")
        .parse()
        .map_err(|e| format!("{e}"))?;
    let loading = app.get_webview_window(LOADING);
    let visible = loading
        .as_ref()
        .is_none_or(|w| w.is_visible().unwrap_or(true));
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url))
        .title(TITLE)
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 560.0)
        .center()
        .visible(visible)
        .build()
        .map_err(|e| e.to_string())?;
    if let Some(loading) = loading {
        // `destroy`, not `close`: close is turned into hide by the window event handler.
        let _ = loading.destroy();
    }
    Ok(())
}

/// Shows and focuses the app window (or the loading window while starting).
fn show_window(app: &AppHandle) {
    let window = app
        .get_webview_window(MAIN)
        .or_else(|| app.get_webview_window(LOADING));
    if let Some(window) = window {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create_tray(app: &AppHandle, shell: Arc<Shell>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Storm Codex", true, None::<&str>)?;
    // Enabled by the updater once an update is downloaded and ready.
    let restart = MenuItem::with_id(
        app,
        "restart_update",
        "Restart to update",
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &restart, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(TITLE)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "quit" => quit_app(app, shell.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Hides the windows, stops the stack off the UI thread, then exits.
fn quit_app(app: &AppHandle, shell: Arc<Shell>) {
    if !shell.life.request_stop() {
        return;
    }
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        stop_stack(&shell, "quit", StopBudget::Full);
        app.exit(0);
    });
}
