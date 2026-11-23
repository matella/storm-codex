use crate::state::{AppEvent, EventSender, SharedState};
use crate::uploader;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;

// storm-codex (jalon 3) : latence live abaissée (~5 s → ~1 s) ; le serveur dédoublonne de
// toute façon, donc une stabilisation courte est sûre.
pub(crate) const STABILITY_THRESHOLD: Duration = Duration::from_secs(1);
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
    stop_tx: tokio_mpsc::Sender<()>,
}

impl WatcherHandle {
    pub fn stop(self) {
        let _ = self.stop_tx.try_send(());
    }
}

pub fn start_watcher(
    replay_dir: &str,
    state: SharedState,
    tx: EventSender,
    runtime: tokio::runtime::Handle,
) -> Result<WatcherHandle, String> {
    let dir = PathBuf::from(replay_dir);
    if !dir.is_dir() {
        return Err(format!("Not a directory: {}", replay_dir));
    }

    let (fs_tx, mut fs_rx) = tokio_mpsc::unbounded_channel::<PathBuf>();
    let (stop_tx, mut stop_rx) = tokio_mpsc::channel::<()>(1);

    let state_clone = state.clone();
    let tx_clone = tx.clone();
    runtime.spawn(async move {
        let pending: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
        let agent = Arc::new(uploader::make_agent());

        loop {
            tokio::select! {
                Some(path) = fs_rx.recv() => {
                    let mut p = pending.lock().unwrap();
                    p.insert(path, Instant::now());
                }
                _ = tokio::time::sleep(POLL_INTERVAL) => {
                    let mut ready = Vec::new();
                    {
                        let mut p = pending.lock().unwrap();
                        let now = Instant::now();
                        p.retain(|path, last_seen| {
                            if now.duration_since(*last_seen) >= STABILITY_THRESHOLD {
                                ready.push(path.clone());
                                false
                            } else {
                                true
                            }
                        });
                    }
                    for path in ready {
                        let connected = state_clone.lock().unwrap().server_connected;
                        if connected {
                            uploader::upload_file(&path, &state_clone, &tx_clone, &agent).await;
                        } else {
                            let mut p = pending.lock().unwrap();
                            p.insert(path, Instant::now());
                        }
                    }
                }
                _ = stop_rx.recv() => {
                    break;
                }
            }
        }
    });

    let mut watcher = notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
        if let Ok(event) = result {
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    for path in event.paths {
                        if is_storm_replay(&path) {
                            let _ = fs_tx.send(path);
                        }
                    }
                }
                _ => {}
            }
        }
    })
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch directory: {}", e))?;

    let _ = tx.send(AppEvent::WatcherStarted);
    println!("Watching: {}", replay_dir);

    Ok(WatcherHandle {
        _watcher: watcher,
        stop_tx,
    })
}

fn is_storm_replay(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.eq_ignore_ascii_case("StormReplay"))
        .unwrap_or(false)
}
