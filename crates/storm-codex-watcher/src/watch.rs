//! Live watch: emits a new or changed `.StormReplay` once it is completely written.
//!
//! A replay still being written must never be uploaded half-written. A path is emitted only
//! after 1 s without any filesystem event for it, then a size sample (baseline) and two more
//! checks 500 ms apart that all agree: the earliest emission is ~2 s after the last write.
//! Any event or size change in between restarts the wait.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::detector::is_storm_replay;

/// Quiet period without filesystem events before the size checks start.
const QUIET: Duration = Duration::from_secs(1);
/// Gap between two size checks.
const CHECK_GAP: Duration = Duration::from_millis(500);
/// Size samples that must agree: the baseline plus two checks.
const STABLE_SAMPLES: usize = 3;
const TICK: Duration = Duration::from_millis(100);

/// Keeps the watch alive; dropping it (or [`WatchHandle::stop`]) stops watching.
pub struct WatchHandle {
    _watcher: RecommendedWatcher,
    task: JoinHandle<()>,
}

impl WatchHandle {
    pub fn stop(self) {}
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Watches `dirs` recursively and sends each completed replay path on `tx`.
/// Must be called from within a tokio runtime.
pub fn start(dirs: &[PathBuf], tx: UnboundedSender<PathBuf>) -> Result<WatchHandle, String> {
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|e| format!("watch::start needs a tokio runtime: {e}"))?;
    if let Some(missing) = dirs.iter().find(|d| !d.is_dir()) {
        return Err(format!("not a directory: {}", missing.display()));
    }

    let (fs_tx, fs_rx) = mpsc::unbounded_channel::<PathBuf>();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) => {
                for path in event.paths.into_iter().filter(|p| is_storm_replay(p)) {
                    let _ = fs_tx.send(path);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "replay watch error"),
        })
        .map_err(|e| format!("failed to create the replay watcher: {e}"))?;

    for dir in dirs {
        watcher
            .watch(dir, RecursiveMode::Recursive)
            .map_err(|e| format!("failed to watch {}: {e}", dir.display()))?;
        tracing::info!(dir = %dir.display(), "watching replays");
    }

    let task = runtime.spawn(debounce(fs_rx, tx));
    Ok(WatchHandle {
        _watcher: watcher,
        task,
    })
}

/// Per-path state between the last event and the emission.
struct Pending {
    last_event: Instant,
    last_sample: Option<Instant>,
    samples: Vec<u64>,
}

impl Pending {
    fn new(now: Instant) -> Self {
        Pending {
            last_event: now,
            last_sample: None,
            samples: Vec::new(),
        }
    }
}

enum Verdict {
    Wait,
    Emit,
    Drop,
}

async fn debounce(mut fs_rx: mpsc::UnboundedReceiver<PathBuf>, tx: UnboundedSender<PathBuf>) {
    let mut pending: HashMap<PathBuf, Pending> = HashMap::new();
    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            event = fs_rx.recv() => match event {
                // Any new event restarts the wait from scratch.
                Some(path) => { pending.insert(path, Pending::new(Instant::now())); }
                None => break, // watcher dropped
            },
            _ = ticker.tick() => {
                let now = Instant::now();
                let mut emit = Vec::new();
                pending.retain(|path, state| match evaluate(path, state, now) {
                    Verdict::Wait => true,
                    Verdict::Emit => { emit.push(path.clone()); false }
                    Verdict::Drop => false,
                });
                for path in emit {
                    tracing::info!(file = %path.display(), "replay ready");
                    if tx.send(path).is_err() {
                        return; // nobody listens any more
                    }
                }
            }
        }
    }
}

fn evaluate(path: &Path, state: &mut Pending, now: Instant) -> Verdict {
    if now.duration_since(state.last_event) < QUIET {
        return Verdict::Wait;
    }
    if state
        .last_sample
        .is_some_and(|t| now.duration_since(t) < CHECK_GAP)
    {
        return Verdict::Wait;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return Verdict::Drop; // deleted or renamed away; a new event re-adds it
    };
    let size = meta.len();
    if state.samples.last().is_some_and(|&prev| prev != size) {
        *state = Pending::new(now); // still growing
        return Verdict::Wait;
    }
    state.samples.push(size);
    state.last_sample = Some(now);
    if state.samples.len() < STABLE_SAMPLES {
        Verdict::Wait
    } else if size == 0 {
        Verdict::Drop // empty file: the write that fills it raises a new event
    } else {
        Verdict::Emit
    }
}
