use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Request};
use tower::ServiceExt;

#[tokio::test]
async fn platform_precompressed_assets_and_cache_headers() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("app");
    std::fs::create_dir(&web).unwrap();
    let asset = "storm-web-app-1234567890abcdef_bg.wasm";
    let plain = vec![42; 512];
    // Distinct byte markers: a run-time encoder cannot produce these exact bodies.
    for (name, bytes) in [
        (asset.to_owned(), plain.as_slice()),
        (format!("{asset}.br"), b"prebuilt br"),
        (format!("{asset}.gz"), b"prebuilt gzip"),
        ("local.css".into(), plain.as_slice()),
        ("index.html".into(), b"<html>app</html>"),
    ] {
        std::fs::write(web.join(name), bytes).unwrap();
    }
    let mut cfg = standalone::tests::test_config(dir.path());
    cfg.web_dir = Some(dir.path().to_owned());
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    let mut state = test_app_state(cfg, db);
    let app = app_router(state.clone());
    let request = |path: &str, accept: &str| {
        Request::get(path)
            .header(header::ACCEPT_ENCODING, accept)
            .body(Body::empty())
            .unwrap()
    };
    let asset_url = format!("/v2/app/{asset}");
    for (accept, encoding, expected) in [
        ("br, gzip", Some("br"), b"prebuilt br".as_slice()),
        ("gzip;q=1, br;q=0.5", Some("gzip"), b"prebuilt gzip"),
        ("br;q=0, gzip", Some("gzip"), b"prebuilt gzip"),
        ("identity", None, plain.as_slice()),
    ] {
        let r = app
            .clone()
            .oneshot(request(&asset_url, accept))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let h = r.headers();
        assert_eq!(
            h.get(header::CONTENT_ENCODING).map(|v| v.to_str().unwrap()),
            encoding
        );
        assert_eq!(h[header::CONTENT_TYPE], "application/wasm");
        assert_eq!(h[header::VARY], "accept-encoding");
        assert_eq!(
            h[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(h[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(to_bytes(r.into_body(), 1024).await.unwrap(), expected);
    }
    std::fs::remove_file(web.join(format!("{asset}.br"))).unwrap();
    let r = app
        .clone()
        .oneshot(request(&asset_url, "br, gzip"))
        .await
        .unwrap();
    assert_eq!(r.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(
        to_bytes(r.into_body(), 1024).await.unwrap(),
        "prebuilt gzip"
    );

    for (path, status, cache) in [
        ("/", StatusCode::OK, "no-cache"),
        ("/v2/app/index.html", StatusCode::OK, "no-cache"),
        ("/v2/app/local.css", StatusCode::OK, "no-cache"),
        (
            "/v2/app/not-built-1234567890abcdef.js",
            StatusCode::NOT_FOUND,
            "no-store",
        ),
        ("/not-built.js", StatusCode::NOT_FOUND, "no-store"),
    ] {
        let r = app.clone().oneshot(request(path, "gzip")).await.unwrap();
        assert_eq!(r.status(), status, "{path}");
        assert_eq!(r.headers()[header::CACHE_CONTROL], cache, "{path}");
        assert_eq!(r.headers()[header::REFERRER_POLICY], "no-referrer");
        if path.ends_with("local.css") {
            assert_eq!(r.headers()[header::CONTENT_ENCODING], "gzip");
        }
    }
    state.standalone = Some(Arc::new(standalone::StandaloneState::new(&state.cfg)));
    let r = app_router(state)
        .oneshot(
            Request::get("/")
                .header(header::HOST, "not-local.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(r.status().is_client_error());
    assert_eq!(r.headers()[header::REFERRER_POLICY], "no-referrer");
}
