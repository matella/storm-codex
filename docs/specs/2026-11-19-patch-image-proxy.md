# Patch-note image proxy/cache — local fix for P1

Date: 2026-11-19 · Status: **approved** by the operator via `clarify` (2026-11-19): "proxy/cache
[Blizzard patch-note images] through the local server for a true offline fix."

Addendum to `docs/specs/2026-11-19-web-audit-fixes.md`. That spec (approved 2026-11-19) explicitly
put "proxying remote patch-note images" **out of scope** and limited P1 to `referrerpolicy="no-referrer"`
+ `loading="lazy"` on the `<img>` (done, `fix/web-audit-platform`, merged). This addendum reverses that
one line for P1(a) — the rest of that spec is unaffected and stays as implemented.

## Problem (P1, from `docs/research/2026-11-19-modern-web-guidance-audit.md`)

Patch-notes HTML (`dim_patches.data.detail.content`, `patch_hero_sections.content`) embeds `<img>`
tags whose `src` points at Blizzard's CDN (observed: `bnetcmsus-a.akamaihd.net`). Every patch page
therefore makes a live third-party request — a privacy leak on a "no third-party request" app, and a
broken image with no fallback when the app is used offline (the explicit standalone/offline goal in
`docs/spec/01-architecture.md`).

## Goal

Make patch-note images load from the local server origin only, with the fetch happening once (at
reference-data ingestion), not per page view and not per browser. No change to the app/front-end:
the rewrite happens **server-side**, before the HTML ever reaches the client, so a local path already
passes `web-rs/core/src/sanitize.rs::url_safe` unchanged (no scheme ⇒ always allowed) — no edit needed
there beyond what `fix/web-audit-platform` already shipped.

## Design

New module `crates/storm-codex-server/src/refdata/patchimg.rs`:

- **Pure rewrite** `rewrite(html: &str) -> (String, Vec<ImageRef>)`: scans `<img src="...">` for an
  absolute `http(s)` URL whose host is in a small hardcoded allowlist (`ALLOWED_HOSTS`, currently just
  `bnetcmsus-a.akamaihd.net`; extend it if the operator finds another Blizzard CDN host in practice).
  A matching `src` is rewritten to `patch-images/<sha256(url)>.<ext>` (`<ext>` taken from the URL's own
  extension when it is one of `png|jpg|jpeg|gif|webp|avif`, else `jpg`); the original URL + dest are
  returned as an `ImageRef` for the caller to fetch. Anything else (disallowed host, relative path,
  `data:`, malformed markup) is left untouched. No I/O, no network: unit-tested directly.
- Called from `refdata::build_patch` (where `html` and the article `body` are assembled, same place
  that already builds `detail_sections[].content` and `detail.content`) for every section and every
  patch body, **before** the strings are put in `Rows` / stored in Postgres. `Rows` gets a new field
  `patch_images: Vec<ImageRef>` (deduplicated by dest) collected across the whole dataset.
- **Fetch** `fetch_missing(images_dir, fetcher, refs, max_file_bytes) -> FetchReport` (sync, run inside
  the same `spawn_blocking` as the rest of `build`/`store`): for each `ImageRef` whose `images_dir.join(dest)`
  does not already exist, fetches through a small `ImageFetcher` trait (prod impl `Ureq`, same HTTP
  crate already used by `refdata::load::Http` — no new dependency), capped by `Content-Length` and by
  bytes actually read (`max_file_bytes`, config `PATCH_IMAGE_MAX_FILE_BYTES`, default 8 MiB), checked
  against a content-type allowlist (`image/png|jpeg|gif|webp|avif`). Written to a temp file in the same
  dir then renamed into place (atomic, same pattern as `replay2d.rs`/`raw.rs`). One failing image is
  logged and skipped — it does **not** fail the whole refresh (same policy as the existing
  "images not copied" / per-hero-art skip in `refdata::load`).
- **Cache bound**: `patch-images/` is a cache, not a permanent vendored asset (unlike the rest of
  `IMAGES_DIR`, which is never evicted — D3). After fetching, `raw::enforce_lru` (already `pub(crate)`,
  reused as is) is applied to `images_dir.join("patch-images")` against `PATCH_IMAGE_CACHE_MAX_BYTES`
  (config, default 200 MiB — small, since a typical patch has 1–4 images and old patches are rarely
  re-fetched). Same LRU-by-mtime eviction as the raw/2D disk caches; a request that reuses a cached
  image does not currently re-touch its mtime (no new read path — see Serving below), so eviction order
  follows fetch recency, not access recency. Acceptable for v1; note if the operator wants LRU-by-access
  instead.
- **Serving**: no new route. `patch-images/` is a plain subfolder of `IMAGES_DIR`, already served at
  `/images/**` by the existing `ServeDir` in `main.rs`. A rewritten `src` is `/images/patch-images/<hash>.<ext>`.
- **Box vs standalone**: the pure `rewrite` runs in `build()`, shared by both `refdata::load::refresh`
  (box) and `refdata::load::ingest_bundle` (standalone) — the HTML is rewritten to a local path in both
  modes, so neither mode ever embeds a live Blizzard URL in stored content (closes the privacy leak
  everywhere, not just on the box). The actual **fetch** only runs from `refresh` (box, which already
  has a network connection and is where `refdata::load`'s only outbound HTTP calls happen); standalone's
  `ingest_bundle` stays 100% network-free as today — a patch image never fetched by the box before the
  bundle was trimmed is simply missing offline (broken `<img>`, no request attempted), which is strictly
  better than the current leak. Carrying patch images into `trim`'s bundle is a possible follow-up, out
  of scope here (it would need the box to have warmed the cache for that dataset release first).

## Typed errors

`PatchImageError` (`thiserror`): `DisallowedHost`, `InvalidUrl`, `Fetch` (wraps the `ureq` error text),
`TooLarge { url, bytes }`, `UnsupportedContentType { url, content_type }`, `Io`. No `unwrap()` outside
tests, matching the rest of `refdata`.

## Config (`docs/spec/04-serveur.md`, Rule 0)

- `PATCH_IMAGE_MAX_FILE_BYTES` (default 8 MiB): per-image cap, checked against `Content-Length` and
  against bytes actually streamed (a server lying about `Content-Length` cannot bypass it).
- `PATCH_IMAGE_CACHE_MAX_BYTES` (default 200 MiB): total bound on `IMAGES_DIR/patch-images`, LRU-evicted.

## Budgets

No new request-path endpoint, so no new API p95 entry. The fetch runs inside the existing box refresh
background task (already unbounded/offline-tolerant — a slow or failing CDN delays or skips the image
copy, never the patch data write, which already commits first). Not a 01-architecture.md budget-table
item; flagged here in case the operator wants one after real-world timing.

## Acceptance

- Unit tests (TDD, no network) for `rewrite`: allowed host rewritten + `ImageRef` returned; disallowed
  host and relative/`data:` src left untouched; multiple `<img>`s same URL dedup to one `ImageRef`;
  case-insensitive host/scheme; malformed `src` does not panic.
- Unit tests for `fetch_missing` against a fake `ImageFetcher`: size cap (both `Content-Length` and
  streamed-bytes enforcement), content-type allowlist, atomic write, skip when the dest already exists,
  one failing image does not stop the others.
- Integration test through `refdata::load::refresh`/`store` on a fixture dataset with a `bnetcmsus-a.akamaihd.net`
  image in a section's `html`: stored `patch_hero_sections.content` holds the local path, not the
  original URL.
- `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, full `scripts/full-check.sh`.
- `git grep` after the change: no HTML stored in Postgres contains `bnetcmsus-a.akamaihd.net` (or any
  other allowlisted host) once a dataset carrying such an image has been ingested.

## Out of scope (unchanged from the parent spec, still true)

Image transcoding/resizing (A10's WebP thumbnails), bundling patch images into the standalone installer
(`trim`), any change to `web-rs/core/src/sanitize.rs` beyond what `fix/web-audit-platform` already
shipped, any App-slice CSS/markup file.
