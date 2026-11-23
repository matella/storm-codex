use crate::detector;
use crate::settings;
use crate::state::{
    AppEvent, EventReceiver, EventSender, SharedState, UploadStatus, WatcherStatus,
};
use crate::uploader;
use crate::watcher::{self, WatcherHandle};
use crate::win_utils;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

type ObsPlayersResult = (Vec<(String, String)>, Option<String>);

// --- Colors ---
const BG_DARK: egui::Color32 = egui::Color32::from_rgb(0x11, 0x18, 0x27);
const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(0x1f, 0x29, 0x37);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x37, 0x41, 0x51);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0xf9, 0xfa, 0xfb);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x9c, 0xa3, 0xaf);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x6b, 0x72, 0x80);
const TEXT_BODY: egui::Color32 = egui::Color32::from_rgb(0xd1, 0xd5, 0xdb);
const GREEN: egui::Color32 = egui::Color32::from_rgb(0x4a, 0xde, 0x80);
const RED: egui::Color32 = egui::Color32::from_rgb(0xf8, 0x71, 0x71);
const AMBER: egui::Color32 = egui::Color32::from_rgb(0xfb, 0xbf, 0x24);
const ORANGE: egui::Color32 = egui::Color32::from_rgb(0xfb, 0x92, 0x3c);
const BLUE: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
const ROW_ALT: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x23, 0x32);
const PROGRESS_TRACK: egui::Color32 = egui::Color32::from_rgb(0x37, 0x41, 0x51);
const SUCCESS_BG: egui::Color32 = egui::Color32::from_rgb(0x06, 0x4e, 0x3b);
const SUCCESS_TEXT: egui::Color32 = egui::Color32::from_rgb(0x6e, 0xe7, 0xb7);
const ERROR_BG: egui::Color32 = egui::Color32::from_rgb(0x7f, 0x1d, 0x1d);
const ERROR_TEXT: egui::Color32 = egui::Color32::from_rgb(0xfc, 0xa5, 0xa5);

#[derive(PartialEq)]
enum Panel {
    None,
    Settings,
    ObsSetup,
}

#[derive(PartialEq)]
enum AppScreen {
    Onboarding,
    Main,
}

pub struct ReplayApp {
    state: SharedState,
    tx: EventSender,
    rx: Arc<Mutex<EventReceiver>>,
    runtime: tokio::runtime::Handle,

    // Screen state
    screen: AppScreen,

    // GUI state
    active_panel: Panel,
    settings_replay_dirs: Vec<String>,
    save_message: Option<(String, bool, Instant)>,
    watcher_handles: Vec<WatcherHandle>,

    // Onboarding state
    onboarding_dirs: Vec<(String, bool)>, // (path, selected)

    // OBS Setup state
    obs_mode: usize,                        // 0=Storm League, 1=Custom/Scrims, 2=All
    obs_all_players: Vec<(String, String)>, // (toon_handle, player_name) from server
    obs_players_loaded: bool,
    obs_players_pending: Arc<Mutex<Option<ObsPlayersResult>>>,
    obs_selected: Vec<(String, String)>, // selected players (toon, name)
    obs_picker_open: bool,
    obs_search: String,
    obs_align: usize, // 0=Left, 1=Right

    // Config serveur éditable au runtime (plus une constante de build) — c'est ce qui rend l'.exe
    // utilisable contre n'importe quel serveur sans rien graver.
    settings_server_url: String,
    settings_auth_token: String,
    connectivity_started: bool, // le check tourne-t-il déjà ? (lancé au boot si URL non vide)
}

impl ReplayApp {
    pub fn new(
        state: SharedState,
        tx: EventSender,
        rx: EventReceiver,
        runtime: tokio::runtime::Handle,
        watcher_handles: Vec<WatcherHandle>,
    ) -> Self {
        let (replay_dirs, init_server_url, init_auth_token, conn_started) = {
            let s = state.lock().unwrap();
            (
                s.replay_dirs.clone(),
                s.server_url.clone(),
                s.auth_token.clone().unwrap_or_default(),
                !s.server_url.trim().is_empty(), // main.rs lance le check si URL non vide au boot
            )
        };

        // Show onboarding if no dirs configured
        let (screen, onboarding_dirs) = if replay_dirs.is_empty() {
            let detected = detector::detect_replay_dirs();
            let dirs: Vec<(String, bool)> = detected
                .into_iter()
                .map(|p| (p.to_string_lossy().to_string(), true))
                .collect();
            (AppScreen::Onboarding, dirs)
        } else {
            (AppScreen::Main, Vec::new())
        };

        Self {
            state,
            tx,
            rx: Arc::new(Mutex::new(rx)),
            runtime,
            screen,
            active_panel: Panel::None,
            settings_replay_dirs: replay_dirs,
            save_message: None,
            watcher_handles,
            onboarding_dirs,
            obs_mode: 0,
            obs_all_players: Vec::new(),
            obs_players_loaded: false,
            obs_players_pending: Arc::new(Mutex::new(None)),
            obs_selected: Vec::new(),
            obs_picker_open: false,
            obs_search: String::new(),
            obs_align: 0,
            settings_server_url: init_server_url,
            settings_auth_token: init_auth_token,
            connectivity_started: conn_started,
        }
    }

    fn drain_events(&mut self) {
        let mut rx = self.rx.lock().unwrap();
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::ServerConnected => {
                    // server_connected already set by connectivity check — rescan all dirs
                    let replay_dirs = {
                        let s = self.state.lock().unwrap();
                        s.replay_dirs.clone()
                    };
                    let state = self.state.clone();
                    let tx = self.tx.clone();
                    self.runtime.spawn(async move {
                        for dir in replay_dirs {
                            uploader::scan_and_upload(&PathBuf::from(dir), &state, &tx).await;
                        }
                    });
                }
                AppEvent::ServerDisconnected => {
                    // server_connected already set by connectivity check — nothing else to do
                }
                AppEvent::WatcherStarted => {
                    self.state.lock().unwrap().watcher_status = WatcherStatus::Watching;
                }
                AppEvent::WatcherStopped => {
                    self.state.lock().unwrap().watcher_status = WatcherStatus::Stopped;
                }
                AppEvent::WatcherError(e) => {
                    self.state.lock().unwrap().watcher_status = WatcherStatus::Error(e);
                }
                AppEvent::UploadStart { filename } => {
                    // Use update (not add) — the uploader already added
                    // the Pending entry; this avoids creating a duplicate.
                    let mut s = self.state.lock().unwrap();
                    s.update_recent(&filename, UploadStatus::Pending);
                }
                AppEvent::UploadSuccess { filename } => {
                    let mut s = self.state.lock().unwrap();
                    s.update_recent(&filename, UploadStatus::Success);
                    s.uploaded_count = s.uploaded.len();
                }
                AppEvent::UploadFailed { filename, error } => {
                    self.state.lock().unwrap().update_recent_with_error(
                        &filename,
                        UploadStatus::Failed,
                        error,
                    );
                }
                AppEvent::UploadDuplicate { filename } => {
                    let mut s = self.state.lock().unwrap();
                    s.update_recent(&filename, UploadStatus::Duplicate);
                    s.uploaded_count = s.uploaded.len();
                }
                AppEvent::BulkProgress(bp) => {
                    self.state.lock().unwrap().bulk_progress = Some(bp);
                }
                AppEvent::BulkDone => {
                    self.state.lock().unwrap().bulk_progress = None;
                }
            }
        }
    }

    // ── Status bar: single compact row ──────────────────────────────────
    fn render_status_bar(&self, ui: &mut egui::Ui) {
        // Extract state up front so we don't hold the lock during rendering
        let (server_connected, watcher_status, uploaded_count, replay_dirs, is_scanning) = {
            let s = self.state.lock().unwrap();
            (
                s.server_connected,
                s.watcher_status.clone(),
                s.uploaded_count,
                s.replay_dirs.clone(),
                s.scanning > 0,
            )
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Server status
            let (server_color, server_text) = if server_connected {
                (GREEN, "Connected")
            } else {
                (RED, "Disconnected")
            };
            Self::status_dot(ui, server_color);
            ui.label(egui::RichText::new("Server:").size(12.0).color(TEXT_DIM));
            ui.label(egui::RichText::new(server_text).size(12.0).color(TEXT_BODY));

            ui.add_space(16.0);

            // Watcher status
            let (watcher_color, watcher_text) = match &watcher_status {
                WatcherStatus::Watching => (GREEN, "Watching".to_string()),
                WatcherStatus::Stopped => (AMBER, "Stopped".to_string()),
                WatcherStatus::Error(e) => (RED, format!("Error: {}", e)),
            };
            Self::status_dot(ui, watcher_color);
            ui.label(egui::RichText::new("Watcher:").size(12.0).color(TEXT_DIM));
            ui.label(
                egui::RichText::new(watcher_text)
                    .size(12.0)
                    .color(TEXT_BODY),
            );

            // Right side: Rescan button + upload count
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} uploaded", uploaded_count))
                        .size(12.0)
                        .color(TEXT_SECONDARY),
                );
                Self::status_dot(ui, GREEN);

                ui.add_space(12.0);

                // Rescan button — disabled if disconnected, no replay dirs, or already scanning
                let can_rescan = !replay_dirs.is_empty() && server_connected && !is_scanning;
                let btn = ui.add_enabled(
                    can_rescan,
                    egui::Button::new(egui::RichText::new("Rescan").size(11.0).color(TEXT_BODY))
                        .fill(BORDER)
                        .min_size(egui::vec2(50.0, 20.0)),
                );
                if btn.clicked() {
                    let dirs = replay_dirs.clone();
                    let state = self.state.clone();
                    let tx = self.tx.clone();
                    self.runtime.spawn(async move {
                        for dir in dirs {
                            uploader::scan_and_upload(&PathBuf::from(dir), &state, &tx).await;
                        }
                    });
                }
            });
        });
    }

    fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, color);
    }

    // ── Bulk progress bar ───────────────────────────────────────────────
    fn render_progress(&self, ui: &mut egui::Ui) {
        let bp = {
            let s = self.state.lock().unwrap();
            s.bulk_progress.clone()
        };

        if let Some(ref bp) = bp {
            let pct = if bp.total > 0 {
                bp.done as f32 / bp.total as f32
            } else {
                0.0
            };

            ui.add_space(4.0);

            // Label
            let mut label = format!("Uploading {}/{}", bp.done, bp.total);
            if bp.duplicates > 0 {
                label.push_str(&format!(" — {} duplicates", bp.duplicates));
            }
            if bp.failed > 0 {
                label.push_str(&format!(" — {} failed", bp.failed));
            }
            ui.label(egui::RichText::new(label).size(11.0).color(TEXT_SECONDARY));

            ui.add_space(3.0);

            // Custom progress bar
            let desired_size = egui::vec2(ui.available_width(), 6.0);
            let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
            let painter = ui.painter();

            // Track
            painter.rect_filled(rect, 3.0, PROGRESS_TRACK);

            // Fill
            let mut fill_rect = rect;
            fill_rect.set_right(rect.left() + rect.width() * pct);
            painter.rect_filled(fill_rect, 3.0, GREEN);

            ui.add_space(4.0);
        }
    }

    // ── Activity log ────────────────────────────────────────────────────
    fn render_activity_log(&self, ui: &mut egui::Ui) {
        let s = self.state.lock().unwrap();
        let recent = s.recent_uploads.clone();
        drop(s);

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("RECENT ACTIVITY")
                .size(10.0)
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);

        egui::Frame::new()
            .fill(BG_PANEL)
            .stroke(egui::Stroke::new(1.0_f32, BORDER))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if recent.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("No uploads yet")
                                        .size(13.0)
                                        .color(TEXT_DIM),
                                );
                            });
                            ui.add_space(40.0);
                        } else {
                            for (i, entry) in recent.iter().enumerate() {
                                let row_bg = if i % 2 == 0 { BG_PANEL } else { ROW_ALT };

                                egui::Frame::new()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(12, 6))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;

                                            // Status dot
                                            let dot_color = match entry.status {
                                                UploadStatus::Success => GREEN,
                                                UploadStatus::Failed => RED,
                                                UploadStatus::Pending => AMBER,
                                                UploadStatus::Duplicate => ORANGE,
                                                UploadStatus::Skipped => TEXT_DIM,
                                            };
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::vec2(8.0, 8.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                rect.center(),
                                                4.0,
                                                dot_color,
                                            );

                                            // Filename (fills space)
                                            let label = egui::Label::new(
                                                egui::RichText::new(&entry.filename)
                                                    .size(12.0)
                                                    .color(TEXT_BODY),
                                            )
                                            .truncate()
                                            .wrap_mode(egui::TextWrapMode::Truncate);

                                            let hover_text = if let Some(ref err) = entry.error {
                                                format!("{}\nError: {}", entry.filename, err)
                                            } else {
                                                entry.filename.clone()
                                            };
                                            ui.add(label).on_hover_text(&hover_text);

                                            // Timestamp (right-aligned)
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let time =
                                                        entry.timestamp.format("%H:%M").to_string();
                                                    ui.label(
                                                        egui::RichText::new(time)
                                                            .size(11.0)
                                                            .color(TEXT_DIM),
                                                    );
                                                },
                                            );
                                        });
                                    });
                            }
                        }
                    });
            });
    }

    // ── Settings panel ──────────────────────────────────────────────────
    fn render_settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);

        // Server (URL + upload token) — éditable, persisté ; rien n'est gravé dans le binaire.
        ui.label(egui::RichText::new("SERVER").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("URL").size(11.0).color(TEXT_DIM));
            ui.add(
                egui::TextEdit::singleline(&mut self.settings_server_url)
                    .hint_text("http://192.168.x.x:5102")
                    .desired_width(280.0)
                    .font(egui::TextStyle::Monospace),
            );
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Token").size(11.0).color(TEXT_DIM));
            ui.add(
                egui::TextEdit::singleline(&mut self.settings_auth_token)
                    .hint_text("upload token (Admin → Upload tokens)")
                    .desired_width(280.0)
                    .password(true)
                    .font(egui::TextStyle::Monospace),
            );
        });
        ui.add_space(10.0);

        // Replay directories
        ui.label(
            egui::RichText::new("REPLAY DIRECTORIES")
                .size(10.0)
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);

        // List current directories with remove buttons
        let mut to_remove = None;
        for (i, dir) in self.settings_replay_dirs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(dir)
                        .size(12.0)
                        .color(TEXT_BODY)
                        .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("✕").size(12.0).color(RED))
                                .fill(egui::Color32::TRANSPARENT)
                                .min_size(egui::vec2(22.0, 22.0)),
                        )
                        .clicked()
                    {
                        to_remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = to_remove {
            self.settings_replay_dirs.remove(i);
        }

        if self.settings_replay_dirs.is_empty() {
            ui.label(
                egui::RichText::new("No directories configured")
                    .size(11.0)
                    .color(TEXT_DIM),
            );
        }

        ui.add_space(4.0);

        // Add + Save buttons
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("+ Add Directory")
                            .size(12.0)
                            .color(TEXT_BODY),
                    )
                    .fill(BORDER)
                    .min_size(egui::vec2(120.0, 26.0)),
                )
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select Replay Directory")
                    .pick_folder()
                {
                    let dir = path.to_string_lossy().to_string();
                    if !self.settings_replay_dirs.contains(&dir) {
                        self.settings_replay_dirs.push(dir);
                    }
                }
            }

            // Détection sous un dossier parent : trouve tous les dossiers de replays dessous
            // (tous les comptes/toons), pas besoin de les ajouter un par un.
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("⌕ Auto-detect under folder")
                            .size(12.0)
                            .color(TEXT_BODY),
                    )
                    .fill(BORDER)
                    .min_size(egui::vec2(170.0, 26.0)),
                )
                .clicked()
            {
                if let Some(parent) = rfd::FileDialog::new()
                    .set_title("Select a PARENT folder (all accounts/replays under it)")
                    .pick_folder()
                {
                    for found in detector::find_replay_dirs_under(&parent) {
                        let dir = found.to_string_lossy().to_string();
                        if !self.settings_replay_dirs.contains(&dir) {
                            self.settings_replay_dirs.push(dir);
                        }
                    }
                }
            }

            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Save")
                            .size(12.0)
                            .color(egui::Color32::WHITE),
                    )
                    .fill(BLUE)
                    .min_size(egui::vec2(60.0, 26.0)),
                )
                .clicked()
            {
                self.save_settings();
            }
        });

        // Status message
        if let Some((ref msg, is_success, time)) = self.save_message {
            if time.elapsed().as_secs() < 5 {
                ui.add_space(6.0);
                let (bg, tc) = if is_success {
                    (SUCCESS_BG, SUCCESS_TEXT)
                } else {
                    (ERROR_BG, ERROR_TEXT)
                };
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(msg).size(11.0).color(tc));
                    });
            }
        }

        ui.add_space(4.0);
    }

    // ── OBS Setup panel ────────────────────────────────────────────────
    fn fetch_obs_players(&mut self) {
        if self.obs_players_loaded {
            return;
        }
        self.obs_players_loaded = true;

        let server_url = {
            let s = self.state.lock().unwrap();
            s.server_url.trim_end_matches('/').to_string()
        };
        let pending = self.obs_players_pending.clone();
        let ctx = self.state.lock().unwrap().ctx.clone();

        self.runtime.spawn(async move {
            let url = format!("{}/api/players", server_url);
            let result = tokio::task::spawn_blocking(move || -> Option<ObsPlayersResult> {
                let agent = ureq::Agent::new_with_config(
                    ureq::config::Config::builder()
                        .timeout_global(Some(std::time::Duration::from_secs(10)))
                        .build(),
                );
                let mut resp = agent.get(&url).call().ok()?;
                let body_str = resp.body_mut().read_to_string().ok()?;
                let body: serde_json::Value = serde_json::from_str(&body_str).ok()?;
                let default = body["default"].as_str().map(|s| s.to_string());
                let players: Vec<(String, String)> = body["players"]
                    .as_array()?
                    .iter()
                    .filter_map(|p| {
                        let toon = p["toonHandle"].as_str()?.to_string();
                        let name = p["playerName"].as_str().unwrap_or("").to_string();
                        Some((toon, name))
                    })
                    .collect();
                Some((players, default))
            })
            .await
            .ok()?;

            if let Some(data) = result {
                *pending.lock().unwrap() = Some(data);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            }

            Some(())
        });
    }

    fn check_obs_players_pending(&mut self) {
        let data = self.obs_players_pending.lock().unwrap().take();
        if let Some((players, default_toon)) = data {
            // Pre-select the default player if none selected yet
            if self.obs_selected.is_empty() {
                let mut found = false;

                // Try server-reported default first
                if let Some(ref default) = default_toon {
                    if let Some(p) = players.iter().find(|(t, _)| t == default) {
                        self.obs_selected.push(p.clone());
                        found = true;
                    }
                }

                // Fall back to extracting toon handles from replay directory paths
                // Typical path: .../Accounts/<id>/<toon_handle>/Replays/Multiplayer
                if !found {
                    for replay_dir in &self.settings_replay_dirs {
                        for component in std::path::Path::new(replay_dir).components() {
                            let part = component.as_os_str().to_string_lossy();
                            // Toon handles look like "2-Hero-1-12345" (digit-text-digit-digits)
                            if part.len() > 3
                                && part.as_bytes()[0].is_ascii_digit()
                                && part.contains('-')
                            {
                                if let Some(p) = players.iter().find(|(t, _)| t == part.as_ref()) {
                                    if !self.obs_selected.iter().any(|(t, _)| t == &p.0) {
                                        self.obs_selected.push(p.clone());
                                    }
                                    break; // Move to next dir
                                }
                            }
                        }
                    }
                }
            }
            self.obs_all_players = players;
        }
    }

    fn render_obs_setup_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);

        let server_url = self
            .state
            .lock()
            .unwrap()
            .server_url
            .trim_end_matches('/')
            .to_string();
        let server_url = server_url.as_str();

        // Mode selector
        ui.label(
            egui::RichText::new("OVERLAY MODE")
                .size(10.0)
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let modes = ["Storm League", "Custom", "All"];
            for (i, label) in modes.iter().enumerate() {
                let selected = self.obs_mode == i;
                let btn = ui.add(
                    egui::Button::new(egui::RichText::new(*label).size(12.0).color(if selected {
                        egui::Color32::WHITE
                    } else {
                        TEXT_SECONDARY
                    }))
                    .fill(if selected { BLUE } else { BORDER })
                    .min_size(egui::vec2(90.0, 26.0)),
                );
                if btn.clicked() {
                    self.obs_mode = i;
                }
            }
        });

        ui.add_space(8.0);

        // Player selector — selected chips + Add button
        ui.label(egui::RichText::new("PLAYERS").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            // Show selected players as removable chips
            let mut to_remove = None;
            for (i, (_, name)) in self.obs_selected.iter().enumerate() {
                let chip_text = if name.is_empty() {
                    &self.obs_selected[i].0
                } else {
                    name
                };
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("{} x", chip_text))
                            .size(11.0)
                            .color(egui::Color32::WHITE),
                    )
                    .fill(BLUE)
                    .corner_radius(12.0)
                    .min_size(egui::vec2(0.0, 22.0)),
                );
                if btn.clicked() {
                    to_remove = Some(i);
                }
            }
            if let Some(i) = to_remove {
                self.obs_selected.remove(i);
            }

            // Add Player button
            if !self.obs_all_players.is_empty() {
                let add_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("+ Add Player")
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                    )
                    .fill(BORDER)
                    .corner_radius(12.0)
                    .min_size(egui::vec2(0.0, 22.0)),
                );
                if add_btn.clicked() {
                    self.obs_picker_open = true;
                    self.obs_search.clear();
                }
            } else if !self.obs_players_loaded {
                ui.label(
                    egui::RichText::new("Loading...")
                        .size(11.0)
                        .color(TEXT_SECONDARY),
                );
            }
        });

        ui.add_space(8.0);

        // Alignment selector
        ui.label(egui::RichText::new("ALIGNMENT").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let aligns = ["Left", "Right"];
            for (i, label) in aligns.iter().enumerate() {
                let selected = self.obs_align == i;
                let btn = ui.add(
                    egui::Button::new(egui::RichText::new(*label).size(12.0).color(if selected {
                        egui::Color32::WHITE
                    } else {
                        TEXT_SECONDARY
                    }))
                    .fill(if selected { BLUE } else { BORDER })
                    .min_size(egui::vec2(90.0, 26.0)),
                );
                if btn.clicked() {
                    self.obs_align = i;
                }
            }
        });

        ui.add_space(8.0);

        // Build OBS URL
        let mode_param = match self.obs_mode {
            0 => "storm+league",
            1 => "custom",
            _ => "all",
        };

        let selected_toons: Vec<String> = self
            .obs_selected
            .iter()
            .map(|(toon, _)| toon.clone())
            .collect();

        let mut obs_url = format!("{}?mode={}", server_url, mode_param);
        if !selected_toons.is_empty() {
            obs_url.push_str(&format!("&player={}", selected_toons.join(",")));
        }
        if self.obs_align == 1 {
            obs_url.push_str("&align=right");
        }

        // OBS URL
        ui.label(
            egui::RichText::new("OBS BROWSER SOURCE URL")
                .size(10.0)
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut obs_url.clone())
                    .desired_width(ui.available_width() - 80.0)
                    .font(egui::TextStyle::Monospace),
            );
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("Copy").size(12.0).color(TEXT_BODY))
                        .fill(BORDER)
                        .min_size(egui::vec2(60.0, 26.0)),
                )
                .clicked()
            {
                ui.ctx().copy_text(obs_url.clone());
            }
        });

        ui.add_space(8.0);

        // Instructions
        ui.label(
            egui::RichText::new("SETUP INSTRUCTIONS")
                .size(10.0)
                .color(TEXT_DIM),
        );
        ui.add_space(4.0);
        let steps = [
            "1. Select mode and player(s) above",
            "2. Copy the URL",
            "3. In OBS, add a Browser Source (800 x 120)",
            "4. Paste the URL — done!",
        ];
        for step in &steps {
            ui.label(egui::RichText::new(*step).size(11.0).color(TEXT_BODY));
        }

        ui.add_space(4.0);
    }

    /// Applique l'URL + le token saisis à l'état partagé (le check de connectivité + les uploads les
    /// relisent), et démarre le check s'il ne tourne pas encore (cas du 1er lancement sans URL au boot).
    fn apply_server_config(&mut self) {
        let url = self.settings_server_url.trim().to_string();
        let token = self.settings_auth_token.trim().to_string();
        {
            let mut s = self.state.lock().unwrap();
            s.server_url = url.clone();
            s.auth_token = if token.is_empty() { None } else { Some(token) };
            s.server_connected = false; // force un re-check au prochain tour
        }
        if !url.is_empty() && !self.connectivity_started {
            uploader::start_connectivity_check(
                self.state.clone(),
                self.tx.clone(),
                self.runtime.clone(),
            );
            self.connectivity_started = true;
        }
    }

    fn save_settings(&mut self) {
        // Validate all directories
        let valid_dirs: Vec<String> = self
            .settings_replay_dirs
            .iter()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect();

        let mut invalid = Vec::new();
        for dir in &valid_dirs {
            if !PathBuf::from(dir).is_dir() {
                invalid.push(dir.clone());
            }
        }

        if !invalid.is_empty() {
            self.save_message = Some((
                format!("Directory not found: {}", invalid[0]),
                false,
                Instant::now(),
            ));
            return;
        }

        if let Err(e) = settings::save_config(
            &self.settings_server_url,
            &self.settings_auth_token,
            &valid_dirs,
        ) {
            self.save_message = Some((format!("Failed to save: {}", e), false, Instant::now()));
            return;
        }

        // Update shared state (dirs) + serveur/token avec reconnexion à chaud
        {
            let mut s = self.state.lock().unwrap();
            s.replay_dirs = valid_dirs.clone();
        }
        self.apply_server_config();

        // Stop all old watchers
        for handle in self.watcher_handles.drain(..) {
            handle.stop();
        }
        self.state.lock().unwrap().watcher_status = WatcherStatus::Stopped;

        // Start new watcher + scan for each dir
        let mut any_started = false;
        let mut watcher_errors = Vec::new();
        for dir in &valid_dirs {
            match watcher::start_watcher(
                dir,
                self.state.clone(),
                self.tx.clone(),
                self.runtime.clone(),
            ) {
                Ok(handle) => {
                    self.watcher_handles.push(handle);
                    any_started = true;
                }
                Err(e) => {
                    watcher_errors.push(e);
                }
            }
        }

        // Scan all dirs sequentially in one background task
        {
            let scan_dirs = valid_dirs.clone();
            let state = self.state.clone();
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                for dir in scan_dirs {
                    uploader::scan_and_upload(&PathBuf::from(dir), &state, &tx).await;
                }
            });
        }

        if any_started {
            self.state.lock().unwrap().watcher_status = WatcherStatus::Watching;
        }

        self.settings_replay_dirs = valid_dirs;
        if watcher_errors.is_empty() {
            self.save_message = Some(("Settings saved.".to_string(), true, Instant::now()));
        } else {
            self.save_message = Some((
                format!("Saved, but watcher failed: {}", watcher_errors.join("; ")),
                false,
                Instant::now(),
            ));
        }
    }

    // ── Onboarding screen ─────────────────────────────────────────────
    fn render_onboarding(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG_DARK)
                    .inner_margin(egui::Margin::same(0)),
            )
            .show(ctx, |ui| {
                // Center content vertically
                let avail = ui.available_size();
                ui.add_space((avail.y * 0.15).max(20.0));

                ui.vertical_centered(|ui| {
                    ui.set_max_width(500.0);

                    ui.label(
                        egui::RichText::new("Welcome to HotS Replay Client")
                            .size(22.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Select your replay folders to get started")
                            .size(14.0)
                            .color(TEXT_SECONDARY),
                    );
                    ui.add_space(20.0);

                    if self.onboarding_dirs.is_empty() {
                        // No dirs detected
                        egui::Frame::new()
                            .fill(BG_PANEL)
                            .stroke(egui::Stroke::new(1.0_f32, BORDER))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("No Heroes of the Storm installation detected.")
                                        .size(13.0)
                                        .color(AMBER),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("Use the Browse button below to add your replay folder manually.")
                                        .size(12.0)
                                        .color(TEXT_SECONDARY),
                                );
                            });
                    } else {
                        // Show detected dirs with checkboxes
                        ui.label(
                            egui::RichText::new("DETECTED REPLAY FOLDERS")
                                .size(10.0)
                                .color(TEXT_DIM),
                        );
                        ui.add_space(6.0);

                        egui::Frame::new()
                            .fill(BG_PANEL)
                            .stroke(egui::Stroke::new(1.0_f32, BORDER))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                for (path, selected) in &mut self.onboarding_dirs {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(selected, "");
                                        ui.label(
                                            egui::RichText::new(path.as_str())
                                                .size(12.0)
                                                .color(TEXT_BODY)
                                                .family(egui::FontFamily::Monospace),
                                        );
                                    });
                                }
                            });
                    }

                    ui.add_space(12.0);

                    // Browse + Start buttons
                    ui.horizontal(|ui| {
                        // Center the buttons
                        let total_width = 140.0 + 8.0 + 100.0;
                        ui.add_space((ui.available_width() - total_width) / 2.0);

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("+ Browse").size(13.0).color(TEXT_BODY),
                                )
                                .fill(BORDER)
                                .min_size(egui::vec2(140.0, 32.0)),
                            )
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Select Replay Directory")
                                .pick_folder()
                            {
                                let dir = path.to_string_lossy().to_string();
                                if !self.onboarding_dirs.iter().any(|(p, _)| p == &dir) {
                                    self.onboarding_dirs.push((dir, true));
                                }
                            }
                        }

                        let has_selected = self.onboarding_dirs.iter().any(|(_, sel)| *sel);
                        let start_btn = ui.add_enabled(
                            has_selected,
                            egui::Button::new(
                                egui::RichText::new("Start")
                                    .size(13.0)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(if has_selected { BLUE } else { BORDER })
                            .min_size(egui::vec2(100.0, 32.0)),
                        );

                        if start_btn.clicked() {
                            self.finish_onboarding();
                        }
                    });

                    ui.add_space(8.0);

                    // Skip link for users who want to configure later
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Skip for now").size(11.0).color(TEXT_DIM),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        self.screen = AppScreen::Main;
                    }
                });
            });
    }

    fn finish_onboarding(&mut self) {
        let selected_dirs: Vec<String> = self
            .onboarding_dirs
            .iter()
            .filter(|(_, sel)| *sel)
            .map(|(path, _)| path.clone())
            .filter(|d| PathBuf::from(d).is_dir())
            .collect();

        // Save to settings (serveur/token éventuellement déjà saisis + dossiers)
        if let Err(e) = settings::save_config(
            &self.settings_server_url,
            &self.settings_auth_token,
            &selected_dirs,
        ) {
            eprintln!("Failed to save settings: {}", e);
        }

        // Update shared state
        {
            let mut s = self.state.lock().unwrap();
            s.replay_dirs = selected_dirs.clone();
        }
        self.apply_server_config();

        // Start watchers
        for dir in &selected_dirs {
            match watcher::start_watcher(
                dir,
                self.state.clone(),
                self.tx.clone(),
                self.runtime.clone(),
            ) {
                Ok(handle) => {
                    self.watcher_handles.push(handle);
                }
                Err(e) => {
                    eprintln!("Watcher error for {}: {}", dir, e);
                }
            }
        }

        if !self.watcher_handles.is_empty() {
            self.state.lock().unwrap().watcher_status = WatcherStatus::Watching;
        }

        // Scan all dirs sequentially in one background task
        {
            let scan_dirs = selected_dirs.clone();
            let state = self.state.clone();
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                for dir in scan_dirs {
                    uploader::scan_and_upload(&PathBuf::from(dir), &state, &tx).await;
                }
            });
        }

        self.settings_replay_dirs = selected_dirs;
        self.screen = AppScreen::Main;
    }
}

impl eframe::App for ReplayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Store ctx for background repaint requests
        {
            let mut s = self.state.lock().unwrap();
            if s.ctx.is_none() {
                s.ctx = Some(ctx.clone());
            }
        }

        // Minimize → hide to tray via Windows API (removes from taskbar)
        if ctx.input(|i| i.viewport().minimized == Some(true)) {
            win_utils::hide_window(win_utils::WINDOW_TITLE);
        }

        self.drain_events();
        self.check_obs_players_pending();

        // Expire save message
        if let Some((_, _, time)) = &self.save_message {
            if time.elapsed().as_secs() >= 5 {
                self.save_message = None;
            }
        }

        // ── Theme ───────────────────────────────────────────────────────
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_DARK;
        visuals.window_fill = BG_DARK;
        visuals.extreme_bg_color = BG_DARK;
        visuals.widgets.noninteractive.bg_fill = BG_PANEL;
        visuals.widgets.inactive.bg_fill = BG_PANEL;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_SECONDARY);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_SECONDARY);
        visuals.selection.bg_fill = BLUE;
        ctx.set_visuals(visuals);

        // ── Screen branching ────────────────────────────────────────────
        if self.screen == AppScreen::Onboarding {
            self.render_onboarding(ctx);
            return;
        }

        // ── Header ──────────────────────────────────────────────────────
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("HotS Replay Client")
                            .size(15.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );

                    // Update banner
                    let update_info = self.state.lock().unwrap().update_available.clone();
                    if let Some((version, url)) = update_info {
                        ui.add_space(8.0);
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(format!("v{} available", version))
                                    .size(11.0)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(GREEN)
                            .min_size(egui::vec2(0.0, 20.0)),
                        );
                        if btn.clicked() {
                            let _ = std::process::Command::new("cmd")
                                .args(["/C", "start", &url])
                                .spawn();
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Settings button
                        let settings_text = if self.active_panel == Panel::Settings {
                            "Close"
                        } else {
                            "Settings"
                        };
                        let settings_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(settings_text)
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            )
                            .frame(false),
                        );
                        if settings_btn.clicked() {
                            if self.active_panel == Panel::Settings {
                                self.active_panel = Panel::None;
                            } else {
                                self.active_panel = Panel::Settings;
                                let s = self.state.lock().unwrap();
                                self.settings_replay_dirs = s.replay_dirs.clone();
                            }
                        }

                        ui.add_space(8.0);

                        // OBS Setup button
                        let obs_text = if self.active_panel == Panel::ObsSetup {
                            "Close"
                        } else {
                            "OBS Setup"
                        };
                        let obs_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(obs_text)
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            )
                            .frame(false),
                        );
                        if obs_btn.clicked() {
                            if self.active_panel == Panel::ObsSetup {
                                self.active_panel = Panel::None;
                            } else {
                                self.active_panel = Panel::ObsSetup;
                                // Reset so we re-fetch (handles URL changes + failed fetches)
                                self.obs_players_loaded = false;
                                self.fetch_obs_players();
                            }
                        }
                    });
                });
            });

        // ── Bottom panel (Settings or OBS Setup) ────────────────────────
        if self.active_panel != Panel::None {
            egui::TopBottomPanel::bottom("bottom_panel")
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .inner_margin(egui::Margin::symmetric(16, 8))
                        .stroke(egui::Stroke::new(1.0_f32, BORDER)),
                )
                .show(ctx, |ui| match self.active_panel {
                    Panel::Settings => self.render_settings_panel(ui),
                    Panel::ObsSetup => self.render_obs_setup_panel(ui),
                    Panel::None => {}
                });
        }

        // ── Player picker popup ─────────────────────────────────────────
        if self.obs_picker_open {
            let mut open = true;
            egui::Window::new("Add Player")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(350.0, 300.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .frame(
                    egui::Frame::new()
                        .fill(BG_PANEL)
                        .stroke(egui::Stroke::new(1.0_f32, BORDER))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ctx, |ui| {
                    // Search input
                    ui.add(
                        egui::TextEdit::singleline(&mut self.obs_search)
                            .hint_text("Search players...")
                            .desired_width(ui.available_width())
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(6.0);

                    // Filter and display matching players
                    let search_lower = self.obs_search.to_lowercase();
                    let selected_toons: Vec<String> =
                        self.obs_selected.iter().map(|(t, _)| t.clone()).collect();

                    let filtered: Vec<(String, String)> = self
                        .obs_all_players
                        .iter()
                        .filter(|(toon, name)| {
                            // Exclude already selected
                            if selected_toons.contains(toon) {
                                return false;
                            }
                            if search_lower.is_empty() {
                                return true;
                            }
                            name.to_lowercase().contains(&search_lower)
                                || toon.to_lowercase().contains(&search_lower)
                        })
                        .cloned()
                        .collect();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.label(
                                    egui::RichText::new("No matching players")
                                        .size(12.0)
                                        .color(TEXT_DIM),
                                );
                            }
                            let mut picked = None;
                            for (i, (toon, name)) in filtered.iter().enumerate() {
                                let display = if name.is_empty() {
                                    toon.clone()
                                } else {
                                    format!("{} ({})", name, toon)
                                };
                                let row_bg = if i % 2 == 0 { BG_PANEL } else { ROW_ALT };
                                egui::Frame::new()
                                    .fill(row_bg)
                                    .inner_margin(egui::Margin::symmetric(8, 4))
                                    .show(ui, |ui| {
                                        let resp = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(&display)
                                                    .size(12.0)
                                                    .color(TEXT_BODY),
                                            )
                                            .fill(row_bg)
                                            .frame(false)
                                            .min_size(egui::vec2(ui.available_width(), 0.0)),
                                        );
                                        if resp.clicked() {
                                            picked = Some((toon.clone(), name.clone()));
                                        }
                                    });
                            }
                            if let Some(player) = picked {
                                self.obs_selected.push(player);
                                self.obs_picker_open = false;
                            }
                        });
                });
            if !open {
                self.obs_picker_open = false;
            }
        }

        // ── Central area ────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG_DARK)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                self.render_status_bar(ui);
                ui.add_space(2.0);
                ui.separator();
                self.render_progress(ui);
                self.render_activity_log(ui);
            });
    }
}
