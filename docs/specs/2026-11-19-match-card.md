# Shareable match card (PNG), rendered locally

Date: 2026-11-19 · Status: **approved** by the operator in chat (2026-11-19)

## Goal

From a match page, save a one-image summary of the game (map, result, duration, both teams' heroes and
K/D/A, highlights) as a PNG file the player can post wherever they like. Rendering and saving are fully
local: no upload, no share API, no social integration. Works on the box (browser) and in the standalone app.

## Current state

- Match page (`web-rs/app/src/pages/match_detail.rs`) reads `GET /api/matches/{id}` (full storm-stats
  `match` + `players` with `gameStats`, awards, `apm`) and `GET /api/matches/{id}/insights` (team fights with
  winner per fight, camps). Awards/MVP labels: `core::session::award_label`, `matchui::AwardTag`.
- Images are served locally: hero portraits `/images/heroes/…` (`ui::portrait_src`), minimap art
  (`core::aggregates::minimap_image`), fonts from `web-rs/app/fonts/` (`fonts.css`). In the standalone app the
  images come from the reference download; before the first sync they are missing (portraits fall back to
  initials).
- Canvas + download already exist in the app: the scouting **Export HTML** (`app/src/export.rs`: loads images,
  redraws them through a canvas, encodes WebP) and the 2D replay **Export clip** (`app/src/clip.rs`,
  `download_blob`: temporary `<a download>` + object URL). `web-sys` features already enabled in
  `web-rs/app/Cargo.toml`: `HtmlCanvasElement`, `CanvasRenderingContext2d`, `HtmlImageElement`, `Blob`, `Url`,
  `HtmlAnchorElement`.
- Desktop (`desktop/`): the main window loads the local server URL (`WebviewUrl::External`,
  `main.rs`), with `on_navigation` guarding external links and **no `on_download` handler**. Native save
  dialogs exist via `tauri-plugin-dialog` (`commands.rs::choose_export_target`, `blocking_save_file`, local
  path check) for the backup export; app commands are deny-by-default (`build.rs`, `command_names.rs`). Whether
  a blob `<a download>` saves a file inside the WebView2 shell has **never been verified**
  (`docs/standalone-acceptance.md` has no item for it).

## Options

| | A. Client canvas (WASM) — **proposed** | B. Server render (`GET /api/matches/{id}/card.png`) |
|---|---|---|
| New dependencies | none (one `web-sys` feature at most, e.g. `FontFaceSet`, or `js_sys::Reflect`) | `tiny-skia` + `png` + a font rasteriser (`ab_glyph`/`fontdue`) + image decoding for portraits (`image` with webp/png): R2, ~1–2 MB server binary, longer builds |
| Bundle / binary | app +10–20 kB Brotli (draw code, estimated; to measure) | app ~0; server and installer grow |
| Fonts & art | the page's own fonts and cached images, same look as the UI | fonts must be embedded in the server; layout duplicated in a second renderer |
| Saving | browser download; desktop needs verification (below) | same download problem (a link to a PNG) |
| Perf budget | none on the API | a new endpoint rendering ~50–150 ms per card, at or over API p95 < 100 ms |

Pick **A**: no new dependency, nothing new on the API budget, one layout in the app's design language.

## Design

- `app/src/card.rs` (wasm): builds an offscreen 1200 × 675 canvas (16:9, readable when scaled down), waits for
  `document.fonts.ready`, loads the minimap and the 10 portraits (`export.rs::load_image`, same-origin so the
  canvas is not tainted), draws, then `canvas.toBlob("image/png")` → `clip::download_blob`. Missing images:
  coloured disc with initials, like `Portrait`. Pure layout maths (column positions, text truncation widths,
  number formatting, file name) in `core/card.rs` with native tests.
- **Contents**: dimmed minimap background; map, mode, date, duration; result from the perspective player
  (WIN/LOSS banner) or "Blue/Red team wins" if no operator account played; two team columns of 5 rows:
  portrait, hero, optional player name, K/D/A, award tag (MVP crown); end levels per team; up to 3 highlights
  already computed: MVP, "Team fights won N of M" (insights `fights`), first to 10 / first fort (match data);
  footer "Storm Codex" + game version. Perspective player's row highlighted.
- **Privacy**: a "Show player names" toggle next to the button, **off by default**: off = heroes only, no names
  anywhere; on = in-game names without `#tag` and never toon handles. No chat text, no battle-tag, no
  replay file name. The toggle choice is remembered in `localStorage`.
- **File name**: `storm-codex-<map-slug>-<YYYY-MM-DD>-<id>.png`.
- **Desktop saving**: acceptance item on MH-Home that the `<a download>` path saves a file in the installed
  app (WebView2 default download behaviour). If it does not, add one app command `save_card(png bytes)` that
  opens the native save dialog through the existing dialog plugin and reuses the local-path check, called only
  when `window.__TAURI__` is present (`standalone.rs::tauri_invoke`); capability updated for that command only.

## API / UI

- No API change. Match page header: "Save as image" button (+ the names toggle). Disabled with a tooltip while
  the match is loading. The card is never shown inline beyond a small preview (optional, see questions).

## Perf & budget impact

- Rendering in the browser: target < 300 ms from click to download on the box DB copy (images cached).
- App bundle: estimated +10–20 kB Brotli, **to be measured**; needs the cap raise to 1.25 MB (~4 kB left).
- Desktop fallback (if needed): a few lines in `desktop/`, no new crate (dialog plugin already linked).

## Tests / acceptance

- Native tests for `core/card.rs` (layout, truncation, file name slug, names hidden when toggle off).
- Browser check on 3 real games (win, loss, a game without any operator account; one with missing portraits):
  the PNG opens, text is legible at 50 % size, no player name when the toggle is off.
- Installed standalone on MH-Home: save works offline, the file lands where the dialog/download says.
- `scripts/check-web-rs-size.sh` with the measured size recorded in `docs/spec/08-frontend.md`.

## Out of scope

Server-side rendering, sharing to any service, clipboard copy, animated or video cards, session cards,
custom themes or layouts, chat or talent builds on the card.

## Operator decisions (2026-11-19)

The open questions were resolved as follows:

- Player names hidden by default; the toggle shows them without the `#tag`.
- Format: 16:9, 1200×675.
- Desktop save dialog fallback (`save_card`) only if the download link does not save a file in the installed Windows app (MH-Home acceptance).

## Open questions (as proposed)

1. Player names on the card: off by default (proposed), or on by default with the toggle?
2. Implement the native save-dialog fallback upfront, or only if the WebView2 download fails on MH-Home?
3. Card size: 1200 × 675 (16:9, proposed) or a square 1080 × 1080?
