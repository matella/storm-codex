//! Patch-note image proxy/cache (spec `docs/specs/2026-11-19-patch-image-proxy.md`).
//!
//! Patch-notes HTML embeds `<img src="https://…">` pointing at Blizzard's CDN (observed host:
//! `bnetcmsus-a.akamaihd.net`). [`rewrite`] is a pure, offline string transform run at dataset
//! build time ([`super::build_patch`]) that replaces an allowlisted `src` with a local
//! `patch-images/<hash>.<ext>` path, so the HTML stored in Postgres never embeds a live remote
//! URL — in either box or standalone mode. [`fetch_missing`] (box only, see `load::refresh`) then
//! fetches each referenced image once and writes it under `IMAGES_DIR/patch-images/`, already
//! served by the existing `/images` `ServeDir` (no new route).

use std::collections::BTreeSet;
use std::path::Path;

/// Hosts this proxy is allowed to fetch from. Extend if the operator finds another Blizzard CDN
/// host in practice; any other host is left untouched by [`rewrite`] (the image keeps loading
/// directly from its original third party, same as before this feature).
const ALLOWED_HOSTS: &[&str] = &["bnetcmsus-a.akamaihd.net"];

/// Image extensions recognised in a source URL; anything else falls back to `jpg`.
const KNOWN_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "avif"];

/// Content types accepted when actually fetching an image.
const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
];

#[derive(Debug, thiserror::Error)]
pub enum PatchImageError {
    #[error("{0}: host not allowlisted for the patch-image proxy")]
    DisallowedHost(String),
    #[error("{url}: fetch failed ({detail})")]
    Fetch { url: String, detail: String },
    #[error("{url}: {bytes} bytes exceeds the {limit}-byte budget")]
    TooLarge { url: String, bytes: u64, limit: u64 },
    #[error("{url}: unsupported content type {content_type}")]
    UnsupportedContentType { url: String, content_type: String },
    #[error("{0}")]
    Io(String),
}

/// One image referenced by rewritten HTML: the original remote URL and where it will live under
/// `images_dir` (`patch-images/<sha256(url)>.<ext>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageRef {
    pub url: String,
    pub dest: String,
}

/// Matches an `<img …>` tag's `src="…"` attribute (double or single quoted); case-insensitive on
/// the tag name, order-independent on attributes (a bounded, non-greedy scan of the tag body).
fn img_src_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r#"(?is)<img\b[^>]*?\bsrc\s*=\s*("([^"]*)"|'([^']*)')"#)
            .unwrap_or_else(|e| panic!("static regex: {e}"))
    })
}

/// Extension inferred from a URL's path (last dot-segment, lowercased), else `jpg`.
fn extension_of(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    KNOWN_EXTENSIONS
        .iter()
        .find(|k| **k == ext)
        .copied()
        .unwrap_or("jpg")
}

/// `true` when `url` is an absolute `http(s)` URL whose host is in [`ALLOWED_HOSTS`]
/// (case-insensitive).
fn allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    ALLOWED_HOSTS.contains(&host)
}

/// `patch-images/<sha256(url)>.<ext>`, relative to `images_dir`.
fn dest_for(url: &str) -> String {
    format!(
        "patch-images/{}.{}",
        super::sha256_hex(url.as_bytes()),
        extension_of(url)
    )
}

/// Rewrites every allowlisted `<img src="…">` in `html` to its local `patch-images/…` path and
/// returns the (deduplicated, sorted) list of images to fetch. Anything not allowlisted (another
/// host, a relative path, `data:`, malformed markup) is left exactly as it was. Pure: no I/O, no
/// network — safe to run for every patch/section at dataset build time, box or standalone.
pub fn rewrite(html: &str) -> (String, Vec<ImageRef>) {
    let mut refs = BTreeSet::new();
    let out = img_src_regex().replace_all(html, |caps: &regex::Captures| {
        let Some(whole) = caps.get(0) else {
            return String::new();
        };
        let whole = whole.as_str();
        let url = caps
            .get(2)
            .or_else(|| caps.get(3))
            .map(|m| m.as_str())
            .unwrap_or_default();
        if !allowed(url) {
            return whole.to_string();
        }
        let dest = dest_for(url);
        refs.insert(ImageRef {
            url: url.to_string(),
            dest: dest.clone(),
        });
        let quote = if caps.get(2).is_some() { '"' } else { '\'' };
        whole.replacen(
            &format!("{quote}{url}{quote}"),
            &format!("{quote}/images/{dest}{quote}"),
            1,
        )
    });
    (out.into_owned(), refs.into_iter().collect())
}

/// Fetches an allowlisted image (prod: [`Ureq`]; tests: an in-memory fake).
pub trait ImageFetcher: Send + Sync {
    /// Returns the body and the `Content-Type` header (lowercased, parameters stripped) of `url`.
    fn fetch(&self, url: &str, max_bytes: u64) -> Result<(Vec<u8>, String), PatchImageError>;
}

/// Production fetcher: plain `ureq` GET (same HTTP crate as `refdata::load::Http`, no new
/// dependency), capped by `Content-Length` when present and by bytes actually read either way.
pub struct Ureq;

impl ImageFetcher for Ureq {
    fn fetch(&self, url: &str, max_bytes: u64) -> Result<(Vec<u8>, String), PatchImageError> {
        let res = ureq::get(url).call().map_err(|e| PatchImageError::Fetch {
            url: url.to_string(),
            detail: e.to_string(),
        })?;
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if let Some(len) = res
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            if len > max_bytes {
                return Err(PatchImageError::TooLarge {
                    url: url.to_string(),
                    bytes: len,
                    limit: max_bytes,
                });
            }
        }
        let bytes = res
            .into_body()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|e| PatchImageError::Fetch {
                url: url.to_string(),
                detail: e.to_string(),
            })?;
        Ok((bytes, content_type))
    }
}

/// Outcome of one [`fetch_missing`] pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FetchReport {
    pub fetched: u32,
    pub already_present: u32,
    pub failed: u32,
}

/// Fetches every `ImageRef` not already present under `images_dir`, writing each atomically
/// (temp file + rename, same pattern as `raw.rs`/`replay2d.rs`). One failing image is logged by
/// the caller and does not stop the others — the write already committed the patch data, this is
/// best-effort enrichment, same policy as the existing hero/map art fetch in `load.rs`.
pub fn fetch_missing(
    images_dir: &Path,
    fetcher: &dyn ImageFetcher,
    refs: &[ImageRef],
    max_file_bytes: u64,
) -> FetchReport {
    let mut report = FetchReport::default();
    for r in refs {
        let dest = images_dir.join(&r.dest);
        if dest.exists() {
            report.already_present += 1;
            continue;
        }
        match fetch_one(&dest, fetcher, &r.url, max_file_bytes) {
            Ok(()) => report.fetched += 1,
            Err(e) => {
                tracing::warn!("patch image not fetched: {e}");
                report.failed += 1;
            }
        }
    }
    report
}

fn fetch_one(
    dest: &Path,
    fetcher: &dyn ImageFetcher,
    url: &str,
    max_file_bytes: u64,
) -> Result<(), PatchImageError> {
    if !allowed(url) {
        return Err(PatchImageError::DisallowedHost(url.to_string()));
    }
    let (bytes, content_type) = fetcher.fetch(url, max_file_bytes)?;
    if bytes.len() as u64 > max_file_bytes {
        return Err(PatchImageError::TooLarge {
            url: url.to_string(),
            bytes: bytes.len() as u64,
            limit: max_file_bytes,
        });
    }
    if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
        return Err(PatchImageError::UnsupportedContentType {
            url: url.to_string(),
            content_type,
        });
    }
    let parent = dest
        .parent()
        .ok_or_else(|| PatchImageError::Io(format!("{}: no parent directory", dest.display())))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| PatchImageError::Io(format!("{}: {e}", parent.display())))?;
    let tmp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, &bytes)
        .map_err(|e| PatchImageError::Io(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        PatchImageError::Io(format!("{}: {e}", dest.display()))
    })?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn allowlisted_host_is_rewritten_and_collected() {
        let html =
            r#"<p><img src="https://bnetcmsus-a.akamaihd.net/cms/gallery/x/foo.jpg" alt=""></p>"#;
        let (out, refs) = rewrite(html);
        assert!(out.contains("src=\"/images/patch-images/"), "{out}");
        assert!(!out.contains("bnetcmsus-a.akamaihd.net"), "{out}");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0].url,
            "https://bnetcmsus-a.akamaihd.net/cms/gallery/x/foo.jpg"
        );
        assert!(refs[0].dest.starts_with("patch-images/") && refs[0].dest.ends_with(".jpg"));
    }

    #[test]
    fn disallowed_host_and_local_or_data_src_are_left_untouched() {
        for html in [
            r#"<img src="https://evil.example/x.png">"#,
            r#"<img src="/images/heroes/abathur.png">"#,
            r#"<img src="data:image/png;base64,AAAA">"#,
            r#"<img alt="no src here">"#,
        ] {
            let (out, refs) = rewrite(html);
            assert_eq!(out, html);
            assert!(refs.is_empty(), "{html}");
        }
    }

    #[test]
    fn same_url_used_twice_dedupes_to_one_image_ref() {
        let html = r#"
            <img src="https://bnetcmsus-a.akamaihd.net/a/b.png">
            <img src="https://bnetcmsus-a.akamaihd.net/a/b.png">
        "#;
        let (_, refs) = rewrite(html);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn host_and_scheme_matching_is_case_insensitive_and_single_quotes_work() {
        let html = r#"<IMG SRC='HTTPS://BNETCMSUS-A.AKAMAIHD.NET/a/b.PNG'>"#;
        let (out, refs) = rewrite(html);
        assert_eq!(refs.len(), 1);
        assert!(out.contains("/images/patch-images/"), "{out}");
    }

    #[test]
    fn extension_falls_back_to_jpg_when_unrecognised() {
        let html = r#"<img src="https://bnetcmsus-a.akamaihd.net/a/b.unknownext">"#;
        let (_, refs) = rewrite(html);
        assert!(refs[0].dest.ends_with(".jpg"));
    }

    #[test]
    fn src_is_found_regardless_of_attribute_order() {
        let html =
            r#"<img alt="Before" src="https://bnetcmsus-a.akamaihd.net/a/b.png" loading="lazy">"#;
        let (out, refs) = rewrite(html);
        assert_eq!(refs.len(), 1);
        assert!(out.contains("/images/patch-images/"), "{out}");
        assert!(out.contains(r#"alt="Before""#), "{out}");
        assert!(out.contains(r#"loading="lazy""#), "{out}");
    }

    type FakeEntry = Result<(Vec<u8>, &'static str), &'static str>;

    struct FakeFetcher {
        bodies: Mutex<HashMap<String, Result<FetchedBody, String>>>,
    }

    type FetchedBody = (Vec<u8>, String);

    impl ImageFetcher for FakeFetcher {
        fn fetch(&self, url: &str, _max_bytes: u64) -> Result<(Vec<u8>, String), PatchImageError> {
            match self.bodies.lock().unwrap().get(url) {
                Some(Ok((b, ct))) => Ok((b.clone(), ct.clone())),
                Some(Err(e)) => Err(PatchImageError::Fetch {
                    url: url.to_string(),
                    detail: e.clone(),
                }),
                None => panic!("unexpected url in test: {url}"),
            }
        }
    }

    fn fake(entries: &[(&str, FakeEntry)]) -> FakeFetcher {
        FakeFetcher {
            bodies: Mutex::new(
                entries
                    .iter()
                    .map(|(u, r)| {
                        (
                            u.to_string(),
                            r.clone()
                                .map(|(b, ct)| (b, ct.to_string()))
                                .map_err(str::to_string),
                        )
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn fetch_missing_writes_new_images_and_skips_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/old.png".into(),
            dest: "patch-images/old.png".into(),
        };
        std::fs::create_dir_all(tmp.path().join("patch-images")).unwrap();
        std::fs::write(tmp.path().join(&existing.dest), b"cached").unwrap();
        let fresh = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/new.png".into(),
            dest: "patch-images/new.png".into(),
        };
        let fetcher = fake(&[(fresh.url.as_str(), Ok((b"PNGDATA".to_vec(), "image/png")))]);
        let report = fetch_missing(
            tmp.path(),
            &fetcher,
            &[existing.clone(), fresh.clone()],
            1024,
        );
        assert_eq!(
            report,
            FetchReport {
                fetched: 1,
                already_present: 1,
                failed: 0
            }
        );
        assert_eq!(
            std::fs::read(tmp.path().join(&existing.dest)).unwrap(),
            b"cached"
        );
        assert_eq!(
            std::fs::read(tmp.path().join(&fresh.dest)).unwrap(),
            b"PNGDATA"
        );
    }

    #[test]
    fn fetch_missing_rejects_oversized_body_without_writing_it() {
        let tmp = tempfile::tempdir().unwrap();
        let big = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/big.png".into(),
            dest: "patch-images/big.png".into(),
        };
        let fetcher = fake(&[(big.url.as_str(), Ok((vec![0u8; 100], "image/png")))]);
        let report = fetch_missing(tmp.path(), &fetcher, std::slice::from_ref(&big), 10);
        assert_eq!(report.failed, 1);
        assert!(!tmp.path().join(&big.dest).exists());
    }

    #[test]
    fn fetch_missing_rejects_unsupported_content_type() {
        let tmp = tempfile::tempdir().unwrap();
        let svg = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/evil.svg".into(),
            dest: "patch-images/evil.jpg".into(),
        };
        let fetcher = fake(&[(svg.url.as_str(), Ok((b"<svg/>".to_vec(), "image/svg+xml")))]);
        let report = fetch_missing(tmp.path(), &fetcher, std::slice::from_ref(&svg), 1024);
        assert_eq!(report.failed, 1);
        assert!(!tmp.path().join(&svg.dest).exists());
    }

    #[test]
    fn fetch_missing_one_failure_does_not_stop_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/bad.png".into(),
            dest: "patch-images/bad.png".into(),
        };
        let good = ImageRef {
            url: "https://bnetcmsus-a.akamaihd.net/x/good.png".into(),
            dest: "patch-images/good.png".into(),
        };
        let fetcher = fake(&[
            (bad.url.as_str(), Err("connection reset")),
            (good.url.as_str(), Ok((b"ok".to_vec(), "image/png"))),
        ]);
        let report = fetch_missing(tmp.path(), &fetcher, &[bad.clone(), good.clone()], 1024);
        assert_eq!(report.fetched, 1);
        assert_eq!(report.failed, 1);
        assert!(tmp.path().join(&good.dest).exists());
        assert!(!tmp.path().join(&bad.dest).exists());
    }

    #[test]
    fn fetch_one_refuses_a_disallowed_host_even_if_passed_directly() {
        let tmp = tempfile::tempdir().unwrap();
        let sneaky = ImageRef {
            url: "https://evil.example/x.png".into(),
            dest: "patch-images/sneaky.png".into(),
        };
        let fetcher = fake(&[]);
        let report = fetch_missing(tmp.path(), &fetcher, std::slice::from_ref(&sneaky), 1024);
        assert_eq!(report.failed, 1);
    }
}
