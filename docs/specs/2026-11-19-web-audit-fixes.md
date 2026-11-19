# Web interface fixes from the Modern Web Guidance audit

Date: 2026-11-19 · Status: **approved** by the operator in chat (2026-11-19: "all the fixes you can do for the interface audit").
Source: `docs/research/2026-11-19-modern-web-guidance-audit.md` (finding IDs below refer to it).

## Goal

Fix the accessibility, privacy, performance and platform findings of the audit across the web app, the OBS
sources and the download page, with no new runtime dependency, no third-party request, and no change to
features. Look changes are limited to the explicitly approved items (A2, A4, A21, S1).

## Scope

**Platform slice** (server, OBS sources, download page; branch `fix/web-audit-platform`):
- **Serve pre-compressed bundles.** The server compresses `/v2/*` bundle files on the fly (lower Brotli
  quality: the app wasm goes out at ~1.26 MB instead of ~0.996 MB at build quality). Produce `.br` and `.gz`
  siblings at build time (Docker image and `release-desktop.yml`, max quality) and serve them with
  `ServeDir::precompressed_br/gzip`; files without a sibling keep the on-the-fly path. Fingerprinted bundle
  files get `Cache-Control: public, max-age=31536000, immutable`; `index.html`/page routes stay `no-cache`.
- **Images caching (A10 partial).** Reference images whose URL carries the reference revision get a long
  cache lifetime instead of `no-cache`, so portraits are not re-downloaded per page. WebP thumbnails are out
  of scope (needs image processing in the reference-data producer; follow-up issue).
- **Privacy (P1).** `Referrer-Policy: no-referrer` on every response; patch-notes remote images get
  `referrerpolicy="no-referrer"` and `loading="lazy"` (sanitizer, `web-rs/core/src/sanitize.rs`). Blocking or
  proxying Blizzard images is out of scope.
- **OBS (O1, O2, O4, O5).** Self-host Cinzel and Oswald (OFL, woff2 subsets) and drop the Google Fonts
  `@import`s; compositor-only pulse animations; `lang` and a defined font stack; replace the `:nth-child`
  stagger with a custom property. Document **OBS 31 or later** (CEF Chromium 127) as the minimum.
- **Download page (S1, S2, S3, S4).** AA-contrast button text, header/footer outside `<main>`, small head
  additions, `fetchpriority` on the logo.

**App slice** (web app, after the session review / replay search / match card branches merge; branch
`fix/web-audit-app`):
- **Keyboard (A1, A7, A8).** Real `<button>`s for clickable spans/divs (same look via a CSS reset),
  `aria-pressed` on toggle pills, labelled controls, `aria-current` on the active nav item, nav `aria-label`,
  a skip link, and real links (`<a href>`) for clickable rows.
- **Focus and titles (A5, A6).** `:focus-visible` rings everywhere (no bare `outline:none`), per-route
  `document.title`, focus moved to the page heading on navigation.
- **Dark platform UI (A3, A12).** `color-scheme: dark` (CSS + meta), `theme-color`, thin themed scrollbars
  instead of hidden ones.
- **Dialogs (A4, approved).** Onboarding, What's new and scouting import become `<dialog>` + `showModal()`
  (Esc, focus containment and return, `::backdrop`), with a backdrop-click fallback.
- **Contrast (A2, approved).** Raise `--kicker` to a value with at least 4.5:1 on `--panel-2` (verified), or
  move text uses to `--muted-2`, whichever keeps the hierarchy; borders keep their tone.
- **Loading (A9, A11, A15, A16, A22, A23).** Preload the critical fonts, eager-load above-the-fold portraits
  (`fetchpriority` on the cover), fallback font metric overrides, fixed background on a pseudo-element,
  `content-visibility` on long sections, stagger via custom property, head metadata.
- **Polish (A14, A18, A19, A20).** Reduced motion everywhere, `text-wrap: balance/pretty`, forced-colours
  fallback for gradient text, empty `alt` on captioned portraits.
- **View transitions (A21, approved).** Same-document view transitions on route changes as progressive
  enhancement (`document.startViewTransition` when available, skipped under reduced motion).

## Out of scope

A13 (rem type scale), A17 (container-query refactor), O3 (now-playing morph, frozen by its spec), WebP
thumbnails, proxying remote patch-note images. Each needs its own design decision.

## Budgets and acceptance

- App bundle ≤ 1.25 MB Brotli (build); OBS bundles ≤ 300 kB gzip. Report deltas.
- axe: 0 serious/critical violations on Session, Matches, Match, Heroes, Maps, Records, Patch notes, Scouting,
  Admin; keyboard probe reaches every former span/div control; Esc closes each dialog and focus returns.
- Lighthouse (desktop) accessibility ≥ 95 on those pages; no performance regression; download page stays 100.
- Network probe: OBS overlays make zero third-party requests; the app wasm is served from the `.br` sibling.
- Existing tests green; new tests for the precompressed serving, headers and sanitizer attributes.

## Platform implementation notes

- Docker and the desktop workflow share a build-only Node script; they retain original files and add
  siblings only for CSS, JS and WASM. On the same Linux release build, app WASM transfers at 969,647
  bytes with precompressed Brotli versus 1,255,662 with dynamic Brotli (22.8% smaller).
- Standalone cache eligibility follows the existing portrait URL contract (`?ref=<revision>`). The
  server does not treat arbitrary revisions, legacy fallback files, or misses as immutable. Unkeyed
  references (including self-hosted art) do not get the new policy.
- Linux Chromium on loopback verified all six OBS sources, including local Cinzel/Oswald, draft pulse
  and music stagger, with no third-party requests on the fixtures. Existing operator media URLs are
  unchanged. The patch fixture's cross-origin image reached its local receiver without a `Referer`.
  Windows NSIS/WebView2 and real OBS acceptance remain pending; the app slice's audits are not gates
  for this platform-only commit.
