#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod detector;
mod live;
mod settings;
mod state;
mod tray;
mod updater;
mod uploader;
mod watcher;
mod win_utils;

use state::{AppState, SharedState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn main() {
    // Load settings (layered: env > settings.json > build defaults)
    let cfg = settings::load();
    let uploaded = settings::load_uploaded();

    println!("Server URL: {}", cfg.server_url);
    println!("Replay dirs: {:?}", cfg.replay_dirs);
    println!("Uploaded count: {}", uploaded.len());

    // Create shared state
    let state: SharedState = Arc::new(Mutex::new(AppState::new(
        cfg.server_url.clone(),
        cfg.auth_token.clone(),
        cfg.replay_dirs.clone(),
        uploaded,
    )));

    // Create event channel
    let (tx, rx) = state::event_channel();

    // Build tokio runtime for async tasks
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let rt_handle = runtime.handle().clone();

    // Start connectivity check
    if !cfg.server_url.is_empty() {
        uploader::start_connectivity_check(state.clone(), tx.clone(), rt_handle.clone());
    }

    // Check for updates
    updater::check_for_update(state.clone(), rt_handle.clone());

    // Feed live (storm-codex `/live`) : garder le handle aussi en mode headless.
    let _live_watcher = live::start_watcher(state.clone(), rt_handle.clone());

    // Start watcher for each replay directory
    let mut watcher_handles = Vec::new();
    let mut scan_dirs = Vec::new();
    for dir in &cfg.replay_dirs {
        let dir_path = PathBuf::from(dir);
        if dir_path.is_dir() {
            match watcher::start_watcher(dir, state.clone(), tx.clone(), rt_handle.clone()) {
                Ok(handle) => {
                    scan_dirs.push(dir.clone());
                    watcher_handles.push(handle);
                }
                Err(e) => {
                    eprintln!("Failed to start watcher for {}: {}", dir, e);
                }
            }
        } else {
            eprintln!("Replay directory does not exist: {}", dir);
        }
    }

    // Scan all dirs sequentially in one background task
    if !scan_dirs.is_empty() {
        let state_bg = state.clone();
        let tx_bg = tx.clone();
        rt_handle.spawn(async move {
            for dir in scan_dirs {
                uploader::scan_and_upload(&PathBuf::from(dir), &state_bg, &tx_bg).await;
            }
        });
    }

    // Re-scan périodique : les watchers (notify, NonRecursive) ne couvrent que les dossiers présents
    // au démarrage et peuvent manquer un event. Toutes les 90 s on remonte jusqu'au dossier
    // `Accounts` (structure HotS) et on re-découvre TOUS les dossiers de replays courants — captant
    // ainsi un nouveau compte joué après le lancement (ex. multi-comptes) ET tout fichier manqué.
    // Dédup local + serveur → idempotent et sûr.
    if !cfg.replay_dirs.is_empty() {
        let state_rs = state.clone();
        let tx_rs = tx.clone();
        let roots = rescan_roots(&cfg.replay_dirs);
        rt_handle.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(90)).await;
                if !state_rs.lock().unwrap().server_connected {
                    continue;
                }
                let mut seen: Vec<PathBuf> = Vec::new();
                for root in &roots {
                    for d in detector::find_replay_dirs_under(root) {
                        if !seen.contains(&d) {
                            seen.push(d.clone());
                            uploader::scan_and_upload(&d, &state_rs, &tx_rs).await;
                        }
                    }
                }
            }
        });
    }

    // Set aggregate watcher status
    if !watcher_handles.is_empty() {
        let mut s = state.lock().unwrap();
        s.watcher_status = state::WatcherStatus::Watching;
    }

    // Mode headless (Docker, HEADLESS=1) : pas d'UI. Les watchers + le re-scan périodique tournent
    // déjà ; on draine les events (sinon pas de consommateur) et on bloque pour garder le runtime et
    // les watchers vivants. Config via env (SERVER_URL / AUTH_TOKEN / REPLAY_DIR).
    if std::env::var("HEADLESS").is_ok() {
        println!(
            "Headless: watching {} dir(s), uploading to {}",
            watcher_handles.len(),
            cfg.server_url
        );
        std::thread::spawn(move || {
            let mut rx = rx;
            while rx.blocking_recv().is_some() {}
        });
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    // Create system tray
    let _tray_icon = tray::create_tray();

    // Launch eframe GUI
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HotS Replay Client")
            .with_inner_size([700.0, 520.0])
            .with_min_inner_size([500.0, 400.0])
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    let state_gui = state.clone();
    let tx_gui = tx.clone();
    let rt_gui = rt_handle.clone();

    eframe::run_native(
        "HotS Replay Client",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(app::ReplayApp::new(
                state_gui,
                tx_gui,
                rx,
                rt_gui,
                watcher_handles,
            )))
        }),
    )
    .expect("Failed to run eframe");
}

/// Racines de re-scan : pour chaque dossier configuré, on remonte jusqu'au dossier `Accounts`
/// (structure HotS) inclus afin que `find_replay_dirs_under` re-découvre tous les comptes — y
/// compris ceux créés/joués après le démarrage. Si `Accounts` est absent du chemin (dossier
/// personnalisé), on garde le dossier tel quel. Dédupliqué.
fn rescan_roots(dirs: &[String]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for d in dirs {
        let p = PathBuf::from(d);
        let root = p
            .ancestors()
            .find(|a| {
                a.file_name()
                    .map(|n| n.eq_ignore_ascii_case("Accounts"))
                    .unwrap_or(false)
            })
            .map(|a| a.to_path_buf())
            .unwrap_or_else(|| p.clone());
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

fn load_app_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .expect("Failed to load app icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}
