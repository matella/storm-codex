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

/// Router-level matrix for Range/conditional negotiation on `/v2` bundles, across the identity, br
/// and gzip representations of the same fingerprinted asset: Range, If-Range match/mismatch, a
/// conditional `If-Modified-Since` hit, an out-of-range request, and that every response keeps
/// `Vary: accept-encoding` and the fingerprinted `Cache-Control` (a 304 must not fall back to
/// `no-store`, nor to a different representation's policy).
#[tokio::test]
async fn platform_range_and_conditional_negotiation_across_representations() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("app");
    std::fs::create_dir(&web).unwrap();
    let asset = "storm-web-range-1234567890abcdef_bg.wasm";
    let plain = vec![7u8; 512];
    for (name, bytes) in [
        (asset.to_owned(), plain.as_slice()),
        (format!("{asset}.br"), b"prebuilt br payload".as_slice()),
        (format!("{asset}.gz"), b"prebuilt gzip payload".as_slice()),
    ] {
        std::fs::write(web.join(name), bytes).unwrap();
    }
    let mut cfg = standalone::tests::test_config(dir.path());
    cfg.web_dir = Some(dir.path().to_owned());
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    let app = app_router(test_app_state(cfg, db));
    let asset_url = format!("/v2/app/{asset}");

    let call = |accept: &'static str, extra: Vec<(axum::http::HeaderName, &'static str)>| {
        let app = app.clone();
        let asset_url = asset_url.clone();
        async move {
            let mut req = Request::get(&asset_url).header(header::ACCEPT_ENCODING, accept);
            for (name, value) in extra {
                req = req.header(name, value);
            }
            let r = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
            let h = r.headers().clone();
            let status = r.status();
            let body = to_bytes(r.into_body(), 1 << 20).await.unwrap();
            (status, h, body)
        }
    };

    for (accept, total, body_whole) in [
        ("identity", plain.len(), plain.clone()),
        (
            "br",
            "prebuilt br payload".len(),
            b"prebuilt br payload".to_vec(),
        ),
        (
            "gzip",
            "prebuilt gzip payload".len(),
            b"prebuilt gzip payload".to_vec(),
        ),
    ] {
        // Baseline GET: capture Last-Modified (the only validator ServeFile offers here).
        let (status, headers, body) = call(accept, vec![]).await;
        assert_eq!(status, StatusCode::OK, "{accept}: baseline GET");
        assert_eq!(headers[header::VARY], "accept-encoding", "{accept}: Vary");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{accept}: cache on 200"
        );
        assert_eq!(body, body_whole, "{accept}: full body");
        let last_modified = headers[header::LAST_MODIFIED].to_str().unwrap().to_owned();

        // Range: the first byte only, 206 with a Content-Range total matching this representation.
        let (status, headers, body) = call(accept, vec![(header::RANGE, "bytes=0-0")]).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT, "{accept}: ranged GET");
        assert_eq!(
            headers[header::CONTENT_RANGE].to_str().unwrap(),
            format!("bytes 0-0/{total}"),
            "{accept}: Content-Range total"
        );
        assert_eq!(body.len(), 1, "{accept}: ranged body length");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{accept}: cache on 206"
        );
        assert_eq!(
            headers[header::VARY],
            "accept-encoding",
            "{accept}: Vary on 206"
        );

        // If-Range matching the current Last-Modified: the Range is honoured (206).
        let range_header = Box::leak("bytes=0-0".to_string().into_boxed_str());
        let if_range_match = Box::leak(last_modified.clone().into_boxed_str());
        let (status, headers, _) = call(
            accept,
            vec![
                (header::RANGE, range_header),
                (header::IF_RANGE, if_range_match),
            ],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PARTIAL_CONTENT,
            "{accept}: If-Range match honours Range"
        );
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{accept}: cache on If-Range match"
        );

        // Unsupported: tower-http 0.7's ServeFile/ServeDir never reads If-Range at all, so a
        // stale validator does *not* fall back to the full body — Range is honoured regardless.
        // Documented here rather than assumed, since a spec-compliant If-Range would return 200.
        let (status, headers, body) = call(
            accept,
            vec![
                (header::RANGE, "bytes=0-0"),
                (header::IF_RANGE, "Wed, 21 Oct 2015 07:28:00 GMT"),
            ],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PARTIAL_CONTENT,
            "{accept}: If-Range is not implemented upstream; Range still wins"
        );
        assert_eq!(
            body.len(),
            1,
            "{accept}: stale If-Range still returns the ranged body"
        );
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{accept}: cache with a stale If-Range"
        );

        // If-None-Match with the current ETag (tower-http 0.7 does generate one): 304, keeping
        // the immutable policy rather than falling back to `no-store` because the status isn't 2xx.
        let etag = headers
            .get(header::ETAG)
            .map(|v| v.to_str().unwrap().to_owned());
        if let Some(etag) = etag {
            let if_none_match = Box::leak(etag.into_boxed_str());
            let (status, headers, body) =
                call(accept, vec![(header::IF_NONE_MATCH, if_none_match)]).await;
            assert_eq!(
                status,
                StatusCode::NOT_MODIFIED,
                "{accept}: If-None-Match hit"
            );
            assert!(body.is_empty(), "{accept}: 304 has no body");
            assert_eq!(
                headers[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable",
                "{accept}: cache on ETag 304 (must not become no-store)"
            );
        }

        // Conditional GET with the current Last-Modified: 304, keeping the immutable policy (not
        // falling back to `no-store` because the status isn't 2xx).
        let if_modified = Box::leak(last_modified.into_boxed_str());
        let (status, headers, body) =
            call(accept, vec![(header::IF_MODIFIED_SINCE, if_modified)]).await;
        assert_eq!(
            status,
            StatusCode::NOT_MODIFIED,
            "{accept}: conditional GET"
        );
        assert!(body.is_empty(), "{accept}: 304 has no body");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{accept}: cache on 304 (must not become no-store)"
        );

        // Out-of-range request: 416, with a `Content-Range: bytes */<total>` giving the real size.
        let range_oob = Box::leak(format!("bytes={}-", total + 1000).into_boxed_str());
        let (status, headers, _) = call(accept, vec![(header::RANGE, range_oob)]).await;
        assert_eq!(
            status,
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{accept}: out-of-range"
        );
        assert_eq!(
            headers[header::CONTENT_RANGE].to_str().unwrap(),
            format!("bytes */{total}"),
            "{accept}: 416 Content-Range total"
        );
    }
}
