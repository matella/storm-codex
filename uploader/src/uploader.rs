use crate::settings;
use crate::state::{AppEvent, BulkProgress, EventSender, SharedState, UploadStatus};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Append a line to the log file in the app data directory
pub fn log(msg: &str) {
    let log_path = settings::data_dir().join("client.log");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

const MAX_RETRIES: u32 = 5;
const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(30);
const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Create a shared ureq Agent for connection pooling (reuses TCP connections)
pub fn make_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .build(),
    )
}

/// Upload a single replay file. Returns the upload result status.
pub async fn upload_file(
    file_path: &Path,
    state: &SharedState,
    tx: &EventSender,
    agent: &Arc<ureq::Agent>,
) -> UploadStatus {
    let filename = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Skip if already uploaded or server is disconnected
    {
        let s = state.lock().unwrap();
        if s.uploaded.contains(&filename) {
            return UploadStatus::Duplicate;
        }
        if !s.server_connected {
            return UploadStatus::Skipped;
        }
    }

    let (server_url, auth_token) = {
        let s = state.lock().unwrap();
        (s.server_url.clone(), s.auth_token.clone())
    };

    // Notify upload start
    {
        let mut s = state.lock().unwrap();
        s.add_recent(filename.clone(), UploadStatus::Pending);
        s.request_repaint();
    }
    let _ = tx.send(AppEvent::UploadStart {
        filename: filename.clone(),
    });

    // storm-codex-server (jalon 3) : API neuve `/api/upload` (remplace l'overlay Node /api/upload-raw)
    let url = format!("{}/api/upload", server_url.trim_end_matches('/'));
    let file_path_owned = file_path.to_path_buf();
    let mut last_err = String::new();

    for attempt in 1..=MAX_RETRIES {
        let url_c = url.clone();
        let filename_c = filename.clone();
        let auth_c = auth_token.clone();
        let path_c = file_path_owned.clone();
        let agent_c = agent.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<(u16, String), String> {
            let file_bytes = std::fs::read(&path_c).map_err(|e| format!("Read error: {}", e))?;

            // Percent-encode filename for safe HTTP header transport (non-ASCII chars)
            let encoded_filename: String = filename_c
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b".-_ ()".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{:02X}", b)
                    }
                })
                .collect();

            let mut req = agent_c
                .post(&url_c)
                .header("Content-Type", "application/octet-stream")
                .header("X-Filename", &encoded_filename);

            if let Some(ref token) = auth_c {
                req = req.header("Authorization", &format!("Bearer {}", token));
            }

            match req.send(&file_bytes) {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.body_mut().read_to_string().unwrap_or_default();
                    Ok((status, body))
                }
                Err(e) => Err(format!("Network error: {}", e)),
            }
        })
        .await;

        match result {
            Ok(Ok((status_code, body))) => {
                if status_code == 409 {
                    let mut s = state.lock().unwrap();
                    s.uploaded.insert(filename.clone());
                    s.uploaded_count = s.uploaded.len();
                    s.update_recent(&filename, UploadStatus::Duplicate);
                    s.request_repaint();
                    settings::save_uploaded(&s.uploaded);
                    let _ = tx.send(AppEvent::UploadDuplicate {
                        filename: filename.clone(),
                    });
                    return UploadStatus::Duplicate;
                }

                if (200..300).contains(&status_code) {
                    let snippet: String = body.chars().take(200).collect();
                    log(&format!("Uploaded: {} -> {}", filename, snippet));
                    let mut s = state.lock().unwrap();
                    s.uploaded.insert(filename.clone());
                    s.uploaded_count = s.uploaded.len();
                    s.update_recent(&filename, UploadStatus::Success);
                    s.request_repaint();
                    settings::save_uploaded(&s.uploaded);
                    let _ = tx.send(AppEvent::UploadSuccess {
                        filename: filename.clone(),
                    });
                    return UploadStatus::Success;
                }

                last_err = format!("HTTP {}: {}", status_code, body);
                log(&format!(
                    "Upload {}/{} for {} failed: {}",
                    attempt, MAX_RETRIES, filename, last_err
                ));
            }
            Ok(Err(e)) => {
                last_err = e.clone();
                log(&format!(
                    "Upload {}/{} for {} error: {}",
                    attempt, MAX_RETRIES, filename, e
                ));
            }
            Err(e) => {
                last_err = format!("Task error: {}", e);
                log(&format!(
                    "Upload {}/{} for {} error: {}",
                    attempt, MAX_RETRIES, filename, last_err
                ));
            }
        }

        if attempt < MAX_RETRIES {
            let delay = Duration::from_secs(2u64.pow(attempt.min(6)));
            sleep(delay).await;
        }
    }

    // All retries exhausted
    let err_msg = format!("After {} attempts: {}", MAX_RETRIES, last_err);
    let mut s = state.lock().unwrap();
    s.update_recent_with_error(&filename, UploadStatus::Failed, err_msg.clone());
    s.request_repaint();
    let _ = tx.send(AppEvent::UploadFailed {
        filename: filename.clone(),
        error: err_msg,
    });
    UploadStatus::Failed
}

/// Scan a directory for .StormReplay files and upload **every** replay not yet uploaded
/// (full backfill, oldest first so history fills chronologically). The persisted `uploaded`
/// set + the server's content-hash dedup (409) make this idempotent: the first run uploads the
/// whole archive, later runs only the new files; the watcher handles replays during a session.
pub async fn scan_and_upload(replay_dir: &Path, state: &SharedState, tx: &EventSender) {
    {
        let mut s = state.lock().unwrap();
        if !s.server_connected {
            return;
        }
        s.scanning += 1;
    }

    let entries = match std::fs::read_dir(replay_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to read replay dir: {}", e);
            {
                let mut s = state.lock().unwrap();
                s.scanning = s.scanning.saturating_sub(1);
            }
            return;
        }
    };

    let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    let already_uploaded = {
        let s = state.lock().unwrap();
        s.uploaded.clone()
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext.eq_ignore_ascii_case("StormReplay") {
                let fname = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !already_uploaded.contains(&fname) {
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    candidates.push((path, mtime));
                }
            }
        }
    }

    if candidates.is_empty() {
        {
            let mut s = state.lock().unwrap();
            s.scanning = s.scanning.saturating_sub(1);
        }
        return;
    }

    // Oldest first → l'historique se remplit chronologiquement. Pas de troncature : on envoie
    // TOUT le non-uploadé (backfill complet). Dédup garantie par le set persisté + le 409 serveur.
    candidates.sort_by_key(|candidate| candidate.1);
    let to_upload: Vec<PathBuf> = candidates.into_iter().map(|(p, _)| p).collect();

    println!(
        "Backfill : envoi de {} replays ({} déjà uploadés ignorés)...",
        to_upload.len(),
        already_uploaded.len()
    );

    let total = to_upload.len();
    let agent = Arc::new(make_agent());

    // Send initial bulk progress
    {
        let bp = BulkProgress {
            done: 0,
            failed: 0,
            duplicates: 0,
            total,
        };
        let mut s = state.lock().unwrap();
        s.bulk_progress = Some(bp.clone());
        s.request_repaint();
        let _ = tx.send(AppEvent::BulkProgress(bp));
    }

    let mut done = 0usize;
    let mut fail_count = 0usize;
    let mut dupe_count = 0usize;

    for file_path in &to_upload {
        {
            let s = state.lock().unwrap();
            if !s.server_connected {
                break;
            }
        }

        let status = upload_file(file_path, state, tx, &Arc::clone(&agent)).await;
        done += 1;

        match status {
            UploadStatus::Duplicate => {
                dupe_count += 1;
            }
            UploadStatus::Failed => {
                fail_count += 1;
            }
            _ => {}
        }

        let bp = BulkProgress {
            done,
            failed: fail_count,
            duplicates: dupe_count,
            total,
        };
        {
            let mut s = state.lock().unwrap();
            s.bulk_progress = Some(bp.clone());
            s.request_repaint();
        }
        let _ = tx.send(AppEvent::BulkProgress(bp));
    }

    println!(
        "  Initial upload done: {}/{} ({} duplicates, {} failed)",
        done, total, dupe_count, fail_count
    );

    // Clear bulk progress and scanning flag
    {
        let mut s = state.lock().unwrap();
        s.bulk_progress = None;
        s.scanning = s.scanning.saturating_sub(1);
        s.request_repaint();
    }
    let _ = tx.send(AppEvent::BulkDone);
}

/// Spawn a background task that periodically checks server connectivity.
pub fn start_connectivity_check(
    state: SharedState,
    tx: EventSender,
    runtime: tokio::runtime::Handle,
) {
    runtime.spawn(async move {
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .timeout_global(Some(CONNECTIVITY_TIMEOUT))
                .build(),
        );

        loop {
            let (url, was_connected) = {
                let s = state.lock().unwrap();
                (s.server_url.clone(), s.server_connected)
            };

            let check_url = format!("{}/api/health", url.trim_end_matches('/'));
            let agent_c = agent.clone();
            let is_connected =
                tokio::task::spawn_blocking(move || agent_c.get(&check_url).call().is_ok())
                    .await
                    .unwrap_or(false);

            if is_connected != was_connected {
                let mut s = state.lock().unwrap();
                s.server_connected = is_connected;
                s.request_repaint();
                if is_connected {
                    let _ = tx.send(AppEvent::ServerConnected);
                } else {
                    let _ = tx.send(AppEvent::ServerDisconnected);
                }
            }

            sleep(CONNECTIVITY_INTERVAL).await;
        }
    });
}
