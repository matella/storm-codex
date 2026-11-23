use crate::state::SharedState;
use std::time::Duration;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
// storm-codex's GitHub Releases mix multiple components (desktop-v*, uploader-v*): list releases
// and pick the latest one tagged `uploader-v*`, rather than trusting `/releases/latest` (which
// would resolve to whichever component released most recently, usually the desktop app).
// per_page=100 is a pragmatic bound, not full pagination: this is a best-effort "update available"
// notice, not a critical path, and 100 releases comfortably covers this project's realistic history.
const RELEASES_URL: &str = "https://api.github.com/repos/matella/storm-codex/releases?per_page=100";
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

/// Pick the first release tagged `uploader-v*` out of a GitHub releases list, returning
/// (version without the prefix, html_url). Pure and testable independently of the network call.
fn find_uploader_release(releases: &serde_json::Value) -> Option<(String, String)> {
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
            find_uploader_release(&releases)
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

    #[test]
    fn find_uploader_release_skips_desktop_releases() {
        let releases = serde_json::json!([
            { "tag_name": "desktop-v0.1.7", "html_url": "https://example.com/desktop-v0.1.7" },
            { "tag_name": "uploader-v1.1.2", "html_url": "https://example.com/uploader-v1.1.2" },
            { "tag_name": "desktop-v0.1.6", "html_url": "https://example.com/desktop-v0.1.6" },
        ]);
        let (tag, url) = find_uploader_release(&releases).expect("an uploader-v* release exists");
        assert_eq!(tag, "1.1.2");
        assert_eq!(url, "https://example.com/uploader-v1.1.2");
    }

    #[test]
    fn find_uploader_release_none_when_absent() {
        let releases = serde_json::json!([
            { "tag_name": "desktop-v0.1.7", "html_url": "https://example.com/desktop-v0.1.7" },
            { "tag_name": "desktop-v0.1.6", "html_url": "https://example.com/desktop-v0.1.6" },
        ]);
        assert!(find_uploader_release(&releases).is_none());
    }

    #[test]
    fn find_uploader_release_handles_empty_list() {
        assert!(find_uploader_release(&serde_json::json!([])).is_none());
    }
}
