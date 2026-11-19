# storm-codex: Modern Web Guidance audit

- **Date:** 2026-11-19. Read-only audit. Nothing in the repo or the containers was modified.
- **Code audited:** `origin/main` @ `4ff2790`, from a fresh shallow clone in `scratch/sc-main`. `~/apps/storm-codex` has the stale branch `fix/standalone-identity-import` (12 behind) checked out, so it was not used. File:line references point to that clone.
- **Live instance:** http://127.0.0.1:3398 (standalone). Some pages may come from an older build than `main`.
- **Public site:** https://matella.github.io/storm-codex/
- **Guidance used:** Chrome Modern Web Guidance (skill version 2026_11_10). Guide names are given as `folder/file`.
- **Targets:**
  - App: WebView2 or Chrome, evergreen.
  - OBS: the floor is **CEF 127**. OBS 31 and 32.x ship Chromium 127. OBS 33.0 (released 2026-10-24) ships CEF 150, but many streamers will still run 32.x for months.
- **GADS:** every change below needs an approved dated spec (`gads/PROJECT.md`). Severity: H = High, M = Medium, L = Low. Effort: S, M or L.

## Tool outputs summary

| Tool | Target | Result |
|---|---|---|
| `modern-web-guidance legacy-scan` (52 rules / 38 guides) | web-rs/app | 1 finding: `dynamic-sibling-animations` at ds.css:250 (`:nth-child` stagger delays) |
| | web-rs/sources | 1 finding: same rule, now-playing-reveal.css:69 (equaliser delays) |
| | web-rs/overlay, site | 0 findings |
| Lighthouse 13 desktop (Chrome-for-Testing 158 headless) | `/` | Perf 91 · A11y 91 · BP 96 · SEO 91. LCP 1.5 s, CLS 0.03, TBT 0 |
| | `/matches` | 91 · 100 · 100 · 91. LCP 1.4 s, CLS 0.002 |
| | `/match/1` | 86 · 96 · 100 · 91. LCP 1.8 s, **CLS 0.092** |
| | `/heroes` | 83 · 100 · 100 · 91. **LCP 2.4 s** (image, 1.15 s load delay) |
| | `/records` | 94 · 84 · 100 · 91. LCP 1.7 s |
| | site, mobile and desktop | 100 · 100 · 100 · 92. LCP 0.9 s / 0.2 s, CLS 0 |
| axe-core 4.x (10 app pages + site) | app | `color-contrast` (serious) on `/`, `/match/1` (33 nodes), `/maps` (10), `/records` (44), `/admin` (10). `select-name` (critical) on `/records`. `image-redundant-alt` (minor) on `/match/1` and `/records` |
| | site | 0 violations |
| Custom keyboard probe | app | Pills, the `?` help button and the `ds-btn` spans are `cursor:pointer` but cannot receive focus. Native `<a>` focus ring is visible. `document.title` is always "Storm Codex". Computed `color-scheme` is `normal` |
| Network probe | OBS + app | `/cast/overlay` and `/draft/overlay` request **fonts.googleapis.com** (and gstatic). `/patch/:id` requests **bnetcmsus-a.akamaihd.net** images. The other pages make no third-party requests |
| Bundles, served br/gzip | | app wasm br **1 256 249 B** (right at the 1.25 MB target). overlay gzip 206 564 B. sources gzip 173 478 B. Both OBS bundles are under the 300 kB gzip budget |

Lighthouse's raw a11y score misses the keyboard issues: it does not flag a `<span on:click>`. The probe and the source review did.

---

## 1. Web app (web-rs/app: Leptos CSR, browser + Tauri WebView2)

### High

**A1. About 48 click targets are `<span>` or `<div>` with no role, no tabindex and no key handler, so they can't be used with a keyboard.**
- Guide: `accessibility/accessibility.md` (keyboard, 2.1.1), `ui-behaviors/custom-button-actions.md`.
- Where:
  - Filter pills: patches.rs:69,72; records.rs:36,39; hero_changes.rs:124-125; and on matches, heroes, maps.
  - `ds-btn` spans in scouting_report.rs: 268-269, 748-749, 991, 1104, 1295-1300 (Save, Delete, Copy pack, Import).
  - The `?` help button: layout.rs:134.
  - Modal close pills: layout.rs:183 ("skip") and the WhatsNew "got it" pill (layout.rs about line 232).
  - Rename-on-click title: scouting_report.rs:275-276.
  - Clickable team side: scouting_report.rs:714.
  - Clickable fight row: match_insights.rs:125.
- Verified live: on `/matches`, Tab never reaches the mode pills. On every page, the `?` help button is unreachable.
- Fix:
  - Render `<button type="button" class="ds-pill">` and `<button class="ds-btn">`. Add a small CSS reset (`font: inherit; background…`) so the look is unchanged. 73 `<button>` already exist in cast/*, so the pattern is in use.
  - Toggle pills get `aria-pressed`. Radio-like groups get `role="radiogroup"`, as live.rs:293 already does.
  - The rename title becomes a pencil `<button>`.
- Effort: M (mechanical, many files). No design change.

**A2. Muted text fails contrast: `--kicker #5d6275` is used for text.**
- Guide: `visual-design/color.md`, `accessibility/accessibility.md` (1.4.3).
- Where: theme.css:12. Used by table `th` (theme.css:101, ds.css:401), `.ds-label` (ds.css:28), `.ds-meta`, `.ds-gcard-aw.none`, `.ds-lane-empty`, `.ds-trend-x`, and Admin `.ahint`.
- Contrast is 3.03 on panel-2, 3.16 on panel and 3.33 on bg, against a 4.5 target. The text is 9–11 px, which makes it worse. axe found up to 44 nodes per page.
- Fix:
  - Raise `--kicker` to about `#7b8197` (≥ 4.5:1 on `--panel-2`; must be verified).
  - Or move text uses to `--muted-2` (6.3:1) and keep `--kicker` for rules and borders only.
- Effort: S for the token change. **Needs a design decision**, because it changes the palette.

**A3. No `color-scheme: dark`, so browser-drawn UI renders light inside the dark app.**
- Guide: `visual-design/browser-ui-theming.md`, `visual-design/dark-mode.md`.
- Where: theme.css:2 (no declaration anywhere in web-rs). The computed value is `normal`.
- Effect: in WebView2 on Windows, scrollbars (page, `.ds-evlist`, `.ds-lv-feed`, `.ds-toc`, `.ds-modal`), `<select>` popups, the `<datalist>` dropdown, date inputs, autofill and the default `<textarea>` are drawn in light mode.
- Fix:
  - Add `:root { color-scheme: dark; }`.
  - Add `<meta name="color-scheme" content="dark">` in index.html so the pre-WASM blank frame is dark too.
  - Optionally add `scrollbar-color: var(--line-2) transparent` on `html` (see A12).
- Baseline: widely available since 2022. Effort: S. Quick win.

**A4. Modals are plain overlay `<div>`s: no dialog role, no Esc key, no focus containment or return.**
- Guide: `ui-components/dialog.md`, `ui-behaviors/light-dismiss-a-dialog.md`, `ui-behaviors/prevent-page-scroll-under-modal-dialog.md`.
- Where: the Onboarding modal (layout.rs:179-205), WhatsNew (layout.rs about line 227) and the scouting import modal (scouting_report.rs:1097-1101, `.ds-modal-bg` at ds.css:239).
- Effect: focus stays behind the overlay and screen readers are not told a modal opened.
- Fix:
  - Use `<dialog>` + `showModal()` through a `NodeRef` and close on the `close` event.
  - Add `closedby="any"` for light-dismiss (Chrome 134+, fine in WebView2), plus a backdrop click fallback.
  - Use `::backdrop` instead of the rgba overlay.
- Baseline: `<dialog>` is widely available. Effort: M. No visual change.

### Medium

**A5. Text inputs and selects remove the outline on `:focus`; the only cue is a border colour change.**
- Guide: `accessibility/accessibility.md` (2.4.7), `visual-design/browser-ui-theming.md`.
- Where: ds.css:24 (`.ds-input:focus{outline:none}`), theme.css:88, cast.css:104 and cast.css:153.
- The border goes from `#2a2e42` to the accent (3.56:1 between the two states), with no thickness change.
- Fix: replace with `:focus-visible { outline: 2px solid var(--accent-2); outline-offset: 2px }`. Add one global `:where(a,button,[tabindex],summary):focus-visible` ring in ds.css so every control gets a consistent ring. Today only the thin default ring shows.
- Effort: S. Quick win.

**A6. The SPA never updates `document.title` and doesn't move focus on navigation.**
- Guide: `accessibility/accessibility.md` (2.4.2), `ui-behaviors/same-document-transitions.md` (focus routing).
- Verified: every route reports "Storm Codex". This affects WebView2 window title, history entries, AT announcements and the taskbar.
- Fix:
  - Use `leptos_meta::Title`, or an `Effect` that sets the title from page data, e.g. "Industrial District · Match · Storm Codex".
  - After route change, focus the page `<h1 tabindex="-1">`.
- Effort: S–M. Quick win (title part).

**A7. Navigation and landmark gaps.**
- Guide: `accessibility/accessibility.md` (landmarks, skip links).
- Where: layout.rs:111.
  - The top `<nav>` has no `aria-label`.
  - The active tab is shown only by class `on`, with no `aria-current="page"`.
  - There is no skip link before an 11-link nav.
- Rows built as `<div role="link" tabindex="0">` (matches.rs:229, heroes.rs:93, patches.rs:99, player.rs:278) lose middle-click, "open in new tab" and copy-link. Only Enter is handled.
- Fix:
  - Add `aria-current` in `Link` when the tab is active.
  - Add `<a class="skip" href="#main">`.
  - Make the rows real `<a href>` through the existing `Link` component, which keeps the grid styling.
- Effort: S. Quick win.

**A8. Form control naming.**
- Guide: `forms/forms.md`.
- Where: records.rs:40. The `<select class="ds-input">` has no label (axe: critical).
- Fix: `aria-label="Battleground"`, or wrap it in the existing `<label>` pattern from `.ds-filters label`.
- Effort: S. Quick win.

**A9. CSR boot dominates LCP; fonts and portraits are discovered late.**
- Guide: `performance/performance.md`, `performance/optimize-image-priority.md`, `performance/optimize-preload-priority.md`.
- Lighthouse shows LCP render delay of 0.9–1.15 s after TTFB on every page (WASM boot). On `/heroes`, the LCP is a portrait loaded `loading="lazy"` with no fetchpriority, giving a 1.15 s resource load delay. The lazy portraits are at ui.rs:281 and ui.rs:314.
- Fix:
  - Add `<link rel="preload" as="font" type="font/woff2" crossorigin>` for the 2–3 above-the-fold faces (Figtree 400 latin, Big Shoulders 800/900 latin) in index.html.
  - Add a `loading: bool` prop to `Portrait`/`Avatar`. Use eager loading for the first row or grid of tiles and for the hero/player cover portrait, `fetchpriority="high"` only on the XL cover.
  - Optional: put a static shell (top bar skeleton) in index.html so FCP isn't blank.
- Effort: S–M. Quick win.

**A10. Hero portraits are 40–44 KB PNGs at 128×128, shown at 24–48 px.**
- Guide: `performance/deliver-optimized-decorative-images.md`, `performance/optimize-image-priority.md`.
- Lighthouse estimates 560–580 KiB of savings per page (`/heroes`, `/match/1`). They are also served with `cache-control: no-cache`, even though the `?ref=<uuid>` revision makes them immutable.
- `/match/1` downloads deathwing.png twice: once with `?ref=` and once without.
- Fix (server and reference-data side):
  - Transcode to WebP/AVIF, at 64 and 128 px, when reference data is downloaded.
  - Send `immutable, max-age=31536000` when `?ref` is present.
  - Unify the URL builder so the image is fetched once.
- Effort: M. Server change, so it needs its own spec.

**A11. Font swap causes layout shift; there are no fallback metric overrides.**
- Guide: `visual-design/visually-stable-font-fallbacks.md`, `visual-design/visually-stable-mixed-fonts.md`.
- Where: fonts.css:4-146 (`font-display: swap`, no `size-adjust`/`ascent-override`). Big Shoulders Display (condensed) falls back to Arial Narrow, and Figtree falls back to Segoe UI. The swap happens after WASM paints, which contributes to CLS (`/match/1` 0.092, near the 0.1 limit).
- Separately, theme.css:44 sets `body` to `Inter`, which is **never bundled**. The non-`.ds-page` bits render in Segoe UI, a silent mismatch.
- Fix:
  - Add `font-size-adjust: from-font` (Chrome 127+, Baseline 2024) on `.ds-app`.
  - Or define `@font-face` fallback families with `size-adjust` for Arial Narrow and Segoe UI.
  - Replace `Inter` with `var(--body)`. This is a design-token decision, because the OBS sources share that stack.
- Effort: S–M.

**A12. Horizontal scrollers hide their scrollbar with no other affordance.**
- Guide: `visual-design/customize-scrollbar-color-and-thickness.md`, `ui-atoms/scrollability-affordance-hints.md`.
- Where: ds.css:62 (`.ds-nav`) and ds.css:287 (`.ds-topnav`) use `scrollbar-width:none`. At ≤ 860 px, or with zoom or a narrow Tauri window, the nav overflows with no visible cue, and mouse users can't scroll it.
- Fix: use `scrollbar-width: thin; scrollbar-color: var(--line-2) transparent` instead of `none`. Or add a scroll-driven edge fade (`ui-atoms/scrollability-affordance-hints.md`; Chrome 115+).
- Effort: S.

**A13. Text sizing is all in px (body 13 px, many 8–10 px labels).**
- Guide: `accessibility/respect-os-text-scale.md`, `visual-design/typography.md`.
- Effect: the browser default font-size preference is ignored. Ctrl+zoom still works, so this is not a WCAG fail.
- Notes: `<meta name="text-scale" content="scale">` (Chrome/Edge 146+) needs rem-based type, so it isn't an option yet. The 8–9 px labels (`.ds-dban em` 8 px, `th` 9 px, `.ds-tag` 9 px) are below comfortable legibility.
- Fix: move type tokens to rem and set a 10–11 px minimum. **Needs a design decision.**
- Effort: L.

### Low

**A14. Reduced motion only covers `.ds-page` descendants.**
- Guide: `ui-behaviors/motion.md`.
- Where: ds.css:252. `.ds-app`-level hover transforms (`.ds-gcard`, `.ds-htile`, `.ds-rcard`, `.ds-mapt::before` scale) and the cast room (cast.css:162) aren't covered unless they sit inside `.ds-page`.
- Live check: 0 animations under `reduce` on `/matches`. Still, scope the rule to `.ds-app`.
- Effort: S.

**A15. Paint cost from the fixed background and blur.**
- Guide: `performance/interactions-in-complex-layouts.md`, `visual-design/visual-effects.md`.
- Where: `background-attachment: fixed` on `.ds-app` (ds.css:282) forces a repaint on every scroll in Chromium. `backdrop-filter` on the sticky topbar (ds.css:284) and on every `.ds-stat` (ds.css:27).
- Fix: move the radial gradients to a `position: fixed` pseudo-element (`.ds-app::before{position:fixed;inset:0;z-index:-1}`).
- Effort: S. No visual change.

**A16. Wide sections could use `content-visibility`.**
- Guide: `performance/defer-rendering-heavy-content.md`.
- `/patches` (260 rows), `/records` (thousands of nodes) and `/match/1` (11 sections, 4.6k px tall) would benefit from `.ds-sec{content-visibility:auto; contain-intrinsic-size:auto 600px}`.
- Effort: S. Measure INP before and after.

**A17. Component layouts switch on the viewport, not the container.**
- Guide: `css/size-aware-styling.md`, `css/responsive-design.md`.
- Seven `@media (max-width:860px)` blocks drive component layouts (`.ds-gl` 4-column grid, `.ds-poolrow`, `.ds-srow`, `.ds-mrow`, `.ds-board`). Inside `.ds-grid2` half-width panels they never adapt.
- Fix: `container-type:inline-size` on `.ds-panel`/`.ds-shell` plus `@container` rules.
- Baseline: widely available since 2023. **Needs a design decision** (layout QA).
- Effort: M.

**A18. Typography polish.**
- Guide: `visual-design/improve-text-layout-and-legibility.md`.
- `text-wrap: balance` on `.ds-title`, `.ds-sec-hd h2`, `.ds-mboard h3` and `h1`. `text-wrap: pretty` on `.ds-prose`, `.ds-quote`, `.ds-overview`.
- Baseline 2024 for balance. Progressive enhancement. Effort: S. Quick win.

**A19. Forced colours: gradient text disappears in High Contrast.**
- Guide: `visual-design/dark-mode.md`, accessibility.
- Where: `.ds-title` (ds.css:19) uses `color: transparent` + `background-clip:text`. In Windows High Contrast (forced colours), the background is dropped and the title can vanish or turn plain. Status pips and win/loss carry meaning by colour only (`.ds-pips i`).
- Fix: add `@media (forced-colors: active){ .ds-title{color:CanvasText;background:none} }`.
- Effort: S.

**A20. Redundant alt text on portraits.**
- Guide: `accessibility/accessible-generated-content.md`.
- Where: ui.rs:281/314. The image `alt` repeats the hero name printed beside it (axe: up to 55 per page).
- Fix: add an `alt=""` option when a caption is present.
- Effort: S.

**A21. View transitions for route changes.**
- Guide: `ui-behaviors/same-document-transitions.md`, `performance/faster-spa-view-transitions.md`.
- Leptos router can wrap navigations in `document.startViewTransition`, e.g. a portrait morph from list to `/hero/:name`.
- Baseline newly available (2025-10). Chrome 111+, fine in WebView2. Gate it on `prefers-reduced-motion`.
- **Needs a design decision.** Effort: M.

**A22. legacy-scan finding: stagger delays.**
- Guide: `ui-behaviors/dynamic-sibling-animations.md`.
- Where: ds.css:250-251. `:nth-child` stagger delays could use `sibling-index()`, but that's Chrome 138+ and not Baseline. Keep the current rules, or wrap the new form in `@supports`. Effort: S. Optional.

**A23. Missing head metadata.**
- Guide: `visual-design/browser-ui-theming.md`.
- Where: index.html. No `<meta name="theme-color" content="#06070b">` and no `meta description` (Lighthouse SEO 91). Effort: S.

---

## 2. OBS sources (web-rs/overlay, web-rs/sources; CEF 127 floor)

Feature check against Chromium 127:
- In use, all supported: `cqw` and `container-type: size` (Chrome 105), `color-mix()` (111), `inset`, `aspect-ratio`, `-webkit-line-clamp`.
- Not in use: `@starting-style`, anchor positioning, `sibling-index()`, `interpolate-size`, `text-wrap`.
- **CEF 103 (OBS ≤ 30) would break** `cqw` (the cast overlay layout) and `color-mix`. Document "OBS ≥ 31 required".

### High

**O1. The overlays load Google Fonts at runtime, which breaks the local-first and no-third-party rules.**
- Guide: `privacy/privacy.md` (third-party audits), `security/secure-external-subresources.md`.
- Where: overlay/cast-overlay.css:1 (`@import …fonts.googleapis.com…Cinzel`) and overlay/draft-overlay.css:1 (`Oswald` + `Cinzel` again).
- Verified live: `/cast/overlay` and `/draft/overlay` request fonts.googleapis.com.
- Effects:
  - The streamer's IP and OBS UA are sent to Google.
  - Offline, the "nexus" and "tactical" skins fall back to Trajan Pro/serif and Arial Narrow, a visible style change on stream.
  - The `@import` chain is render-blocking, so text can flash at scene start.
- Fix: self-host Cinzel 700/800 and Oswald 500/700, latin subset woff2, about 15–25 KB each, exactly like the JetBrains Mono 700 already in overlay/fonts. Delete both `@import`s. Check that overlay gzip stays under 300 kB (fonts are separate files, so the wasm budget is unaffected).
- Effort: S. Quick win, no design change.

### Medium

**O2. Infinite box-shadow animations keep the compositor busy for the whole broadcast.**
- Guide: `ui-behaviors/motion.md`, `performance/performance.md`.
- Where: draft-overlay.css (`@keyframes pulse` animates `box-shadow`; `blink` on `.dot`). Repainting box-shadow each frame costs CPU and GPU in OBS's offscreen CEF while it is also encoding.
- Fix: animate `opacity` on a pseudo-element that carries the glow (compositor-only).
- Effort: S. No visual change.

**O3. The now-playing reveal animates layout properties.**
- Guide: `css/animate-to-intrinsic-sizes.md`, `ui-behaviors/motion.md`.
- Where: now-playing-reveal.css:16-30 animates `width`/`height`/`border-radius` with `will-change: width, height` (layout every frame).
- The spec says "timings fixed, do not touch". A future spec could use a `clip-path: inset()` or `scale` morph instead. **Needs a design decision.** Effort: M.

### Low

**O4. Missing `lang` and an undefined font.**
- Where: overlay/index.html:2 and sources/index.html:2 have no `lang` attribute. Harmless in OBS, but the "Inter" font is never shipped either: sources/base.css:43 and the overlay CSS set `Inter` first, so OBS on a PC without Inter renders Segoe UI.
- Fix: decide on Figtree/Inter and self-host it, or drop it from the stack.
- Effort: S.

**O5. legacy-scan finding on the equaliser stagger.**
- Where: now-playing-reveal.css:69-71. `sibling-index()` is not available in CEF 127 or 150-era OBS without a fallback. **Keep as is.**

**O6. The reduced-motion block in now-playing-reveal.css:80 is good practice, but OBS never reports reduced motion.** Nothing to do.

---

## 3. Download page (site/index.html + site/style.css)

The page is in good shape: Lighthouse 100/100/100/92, axe 0 violations, strict CSP, `referrer no-referrer`, system fonts only, `color-scheme: dark`, `focus-visible` styled.

### Medium

**S1. The download button's white text fails AA contrast. The button is not rendered yet; it appears once a stable release exists.**
- Guide: `visual-design/color.md`.
- Where: site/style.css:28-33. White on `#7f77dd` is 3.76:1, and the hover/focus `#8f88e6` is 3.07:1. The text is 1.1 rem / 600 (about 17.6 px), below the "large text" threshold.
- Fix: use dark text, as the app's `.ds-btn.primary` does (`#0b0c12` on accent = 5.19:1), or darken the button to about `#5f57c4`.
- Effort: S. Needs a design decision (the button's visual style changes).

### Low

**S2. `<header>` and `<footer>` sit inside `<main>`, so they are not banner/contentinfo landmarks.**
- Guide: `accessibility/accessibility.md`.
- Where: site/index.html:16 and site/index.html:41.
- Fix: move them to be siblings of `<main>`. Effort: S. Quick win.

**S3. Small additions:**
- `<meta name="theme-color" content="#06070b">` (`visual-design/browser-ui-theming.md`).
- `text-wrap: balance` on `h1` and `.lead` (`visual-design/improve-text-layout-and-legibility.md`).
- Effort: S. Quick win.

**S4. The logo has no fetch priority.** `.logo` (96 px, 6 KB) is the LCP on mobile; `fetchpriority="high"` would help marginally. LCP is already 0.9 s. Optional.

**S5. Lighthouse SEO and agentic flags.** `robots.txt` and `llms.txt` are missing. These are not Modern Web Guidance items. Optional.

---

## 4. App privacy (cross-cutting)

**P1 (Medium). Patch-notes HTML loads remote images from Blizzard's CDN.**
- Guide: `privacy/privacy.md`, `security/restrict-outbound-connections.md`.
- Where: core/src/sanitize.rs:88-102 allows `http(s)` image `src`. `.ds-prose img` (ds.css:572) has no width/height.
- Verified: `/patch/:id` requests `bnetcmsus-a.akamaihd.net` (1–4 images per patch).
- Effects: a third-party request on a "no third-party" app, broken images offline, and layout shift.
- Fix options:
  - (a) Rewrite remote `img src` to a local cache populated during patch sync. Needs a spec.
  - (b) Minimum: add `referrerpolicy="no-referrer" loading="lazy" decoding="async"` in the sanitizer.
  - (c) Click-to-load placeholder.
- Also add a `Referrer-Policy: no-referrer` response header on the app (`curl -I` showed none).
- Effort: S for (b), M for (a).

---

## Quick wins (safe, small, no design change)

1. O1: self-host Cinzel and Oswald, and remove the Google Fonts `@import`s.
2. A3: add `color-scheme: dark` (CSS + meta), and `theme-color`.
3. A1: convert span/div click targets to `<button>`, keeping the look.
4. A5: replace `outline:none` with `:focus-visible` rings, plus a global ring.
5. A8: label the `/records` select.
6. A7: `aria-current`, nav `aria-label`, a skip link, and `<a href>` rows.
7. A6: per-route `document.title`.
8. A9: preload the critical fonts; eager-load the above-the-fold portraits plus `fetchpriority` on the cover.
9. A15: move the fixed background to a fixed pseudo-element.
10. A18 and S3: `text-wrap: balance/pretty`.
11. A12: thin coloured scrollbars instead of hidden ones.
12. A14: widen the reduced-motion scope. A19: forced-colors title fix. A20: empty `alt` on captioned portraits.
13. O2: compositor-only pulse in the draft overlay.
14. P1(b): `no-referrer` + lazy on patch-note images, and a `Referrer-Policy` header.
15. S2: move the site's header and footer out of `<main>`.

## Needs a design decision

- A2: raise `--kicker` or restrict it to non-text (palette change).
- S1: download button text colour or button shade.
- A13: rem-based type scale and a minimum label size.
- A11: Inter vs Figtree as the body font, shared with the OBS sources (O4).
- A17: container-query refactor of the component breakpoints.
- A21: view transitions between routes.
- A4: dialogs. Behaviour only, but it changes the UX (Esc, light dismiss, focus trap).
- A10 and P1(a): server-side image transcoding and caching (needs a spec and changes the reference-data pipeline).
- O3: changing the now-playing morph technique (the spec currently freezes it).

## Counts

| Interface | High | Medium | Low | Total |
|---|---|---|---|---|
| App (A) | 4 | 9 | 10 | 23 |
| OBS (O) | 1 | 2 | 3 | 6 |
| Site (S) | 0 | 1 | 4 | 5 |
| Privacy (P) | 0 | 1 | 0 | 1 |
| **Total** | **5** | **13** | **17** | **35** |

## Reproduce

- Scratch dir: `~/.hermes/cache/scratch/mwa`. Chrome-for-Testing headless shell 158 is at `chrome-headless-shell-linux64/`.
- `npx lighthouse <url> --preset=desktop` with `CHROME_PATH` set. JSON reports are in `mwa/lh/`.
- `node axe.mjs [base] [paths] [out]` writes axe + keyboard-probe output to `axe-app.json` and `axe-site.json`.
- `node obs.mjs` and `node patch.mjs` run the network probes.
- legacy-scan JSON: `scratch/ls-*.json`.
