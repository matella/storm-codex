use crate::state::SharedState;
use std::time::Duration;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
// storm-codex's GitHub Releases mix multiple components (desktop-v*, uploader-v*): list releases
// and pick the latest one tagged `uploader-v*`, rather than trusting `/releases/latest` (which
// would resolve to whichever component released most recently, usually the desktop app).
const RELEASES_URL: &str = "https://api.github.com/repos/matella/storm-codex/releases";
const TAG_PREFIX: &str = "uploader-v";

/// Parse a "major.minor.patch" version string into a tuple.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Returns true if `latest` is strictly newer than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Spawn a background task that checks GitHub Releases for a newer version.
pub fn check_for_update(state: SharedState, runtime: tokio::runtime::Handle) {
    runtime.spawn(async move {
        // Small delay so the UI loads first
        tokio::time::sleep(Duration::from_secs(3)).await;

        let result = tokio::task::spawn_blocking(|| -> Option<(String, String)> {
            let agent = ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .timeout_global(Some(Duration::from_secs(10)))
                    .build(),
            );

            let mut resp = agent
                .get(RELEASES_URL)
                .header("User-Agent", "hots-replay-client")
                .call()
                .ok()?;

            let body_str = resp.body_mut().read_to_string().ok()?;
            let releases: serde_json::Value = serde_json::from_str(&body_str).ok()?;
            let releases = releases.as_array()?;

            let release = releases.iter().find(|r| {
                r["tag_name"]
                    .as_str()
                    .is_some_and(|t| t.starts_with(TAG_PREFIX))
            })?;

            let tag = release["tag_name"]
                .as_str()?
                .trim_start_matches(TAG_PREFIX)
                .to_string();
            let url = release["html_url"].as_str()?.to_string();

            Some((tag, url))
        })
        .await
        .ok()?;

        if let Some((latest, url)) = result {
            if is_newer(&latest, CURRENT_VERSION) {
                let mut s = state.lock().unwrap();
                s.update_available = Some((latest, url));
                s.request_repaint();
            }
        }

        Some(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer("1.1.0", "1.0.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }
}
