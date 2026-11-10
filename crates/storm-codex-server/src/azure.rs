//! Post-game box→Azure push (milestone 5): authenticated outgoing POST of the game summary to
//! the Twitch Azure EBS (same mechanism as the HotsPatchNotes patch digest). Optional
//! (`AZURE_PUSH_URL`/`AZURE_PUSH_TOKEN`). **Not tested against the real EBS**: code ready,
//! integration to validate in the evening with the box. Best-effort: never affects the parse.

use serde_json::Value as J;

/// POST `event` to Azure in the background (spawn_blocking: ureq is synchronous).
pub fn push(url: String, token: Option<String>, event: J) {
    tokio::task::spawn_blocking(move || {
        let mut req = ureq::post(&url);
        if let Some(t) = &token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        match req.send_json(&event) {
            Ok(_) => tracing::debug!("push Azure ok"),
            Err(e) => tracing::warn!("Azure push failed: {e}"),
        }
    });
}
