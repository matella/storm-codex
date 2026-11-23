use chrono::{DateTime, Local};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const MAX_RECENT: usize = 50;

// --- Upload status ---
#[derive(Debug, Clone, PartialEq)]
pub enum UploadStatus {
    Pending,
    Success,
    Failed,
    Duplicate,
    Skipped, // Server disconnected — will retry on reconnect
}

// --- Watcher status ---
#[derive(Debug, Clone, PartialEq)]
pub enum WatcherStatus {
    Stopped,
    Watching,
    Error(String),
}

// --- Log entry ---
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub filename: String,
    pub status: UploadStatus,
    pub timestamp: DateTime<Local>,
    pub error: Option<String>,
}

// --- Bulk progress ---
#[derive(Debug, Clone)]
pub struct BulkProgress {
    pub done: usize,
    pub failed: usize,
    pub duplicates: usize,
    pub total: usize,
}

// --- Events sent from background tasks to GUI ---
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AppEvent {
    ServerConnected,
    ServerDisconnected,
    WatcherStarted,
    WatcherStopped,
    WatcherError(String),
    UploadStart { filename: String },
    UploadSuccess { filename: String },
    UploadFailed { filename: String, error: String },
    UploadDuplicate { filename: String },
    BulkProgress(BulkProgress),
    BulkDone,
}

// --- Shared application state ---
pub struct AppState {
    // Server
    pub server_url: String,
    pub auth_token: Option<String>,
    pub server_connected: bool,

    // Watcher
    pub watcher_status: WatcherStatus,
    pub replay_dirs: Vec<String>,

    // Uploads
    pub uploaded: HashSet<String>,
    pub uploaded_count: usize,
    pub recent_uploads: VecDeque<LogEntry>,
    pub bulk_progress: Option<BulkProgress>,
    pub scanning: usize,

    // Update check
    pub update_available: Option<(String, String)>, // (version, release_url)

    // GUI repaint handle
    pub ctx: Option<egui::Context>,
}

impl AppState {
    pub fn new(
        server_url: String,
        auth_token: Option<String>,
        replay_dirs: Vec<String>,
        uploaded: HashSet<String>,
    ) -> Self {
        let uploaded_count = uploaded.len();
        Self {
            server_url,
            auth_token,
            server_connected: false,
            watcher_status: WatcherStatus::Stopped,
            replay_dirs,
            uploaded,
            uploaded_count,
            recent_uploads: VecDeque::new(),
            bulk_progress: None,
            scanning: 0,
            update_available: None,
            ctx: None,
        }
    }

    /// Add a recent upload log entry (max 50, newest first)
    pub fn add_recent(&mut self, filename: String, status: UploadStatus) {
        self.recent_uploads.push_front(LogEntry {
            filename,
            status,
            timestamp: Local::now(),
            error: None,
        });
        if self.recent_uploads.len() > MAX_RECENT {
            self.recent_uploads.pop_back();
        }
    }

    /// Update the status of an existing log entry, or add new
    pub fn update_recent(&mut self, filename: &str, status: UploadStatus) {
        if let Some(entry) = self
            .recent_uploads
            .iter_mut()
            .find(|e| e.filename == filename)
        {
            entry.status = status;
        } else {
            self.add_recent(filename.to_string(), status);
        }
    }

    /// Update the status with an error message
    pub fn update_recent_with_error(
        &mut self,
        filename: &str,
        status: UploadStatus,
        error: String,
    ) {
        if let Some(entry) = self
            .recent_uploads
            .iter_mut()
            .find(|e| e.filename == filename)
        {
            entry.status = status;
            entry.error = Some(error);
        } else {
            self.recent_uploads.push_front(LogEntry {
                filename: filename.to_string(),
                status,
                timestamp: Local::now(),
                error: Some(error),
            });
            if self.recent_uploads.len() > MAX_RECENT {
                self.recent_uploads.pop_back();
            }
        }
    }

    /// Request GUI repaint
    pub fn request_repaint(&self) {
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }
}

pub type SharedState = Arc<Mutex<AppState>>;
pub type EventSender = mpsc::UnboundedSender<AppEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<AppEvent>;

/// Create a new event channel
pub fn event_channel() -> (EventSender, EventReceiver) {
    mpsc::unbounded_channel()
}
