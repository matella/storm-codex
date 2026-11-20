# Overlay builder (custom OBS layouts)

Date: 2026-11-20 · Status: **APPROVED** by the operator in chat (2026-11-20: all 10 existing OBS
widgets become movable building blocks; grid-snapped drag/resize; layouts stored in the local DB).

## Goal

Let the operator freely compose their own OBS overlay from the widgets this project already ships,
instead of using one fixed page per widget. A new in-app **builder** page: a 1920x1080 canvas, a widget
palette, grid-snapped drag/resize, named saved layouts. Each saved layout gets its own stable URL to paste
into OBS as a Browser Source — the actual rendered overlay, composed from whichever widgets the operator
placed, at the positions and sizes they chose.

## Scope (v1)

**Widget catalog**: all 10 existing session/music OBS sources, as movable building blocks —
`/ticker`, `/widget`, `/queue`, `/now-playing`, `/recap`, `/stream-pool`, `/patch-banner`,
`/record-flash`, `/nemesis`, `/goal`. `/cast/overlay` and `/draft/overlay` are **not** in v1 (they are
multi-scene state machines, not single panels — a separate decision later if wanted).

**Placement**: grid-snapped, not pixel-freeform. Design canvas is the existing 1920x1080 OBS frame,
divided into an 80x45 grid of 24px cells (chosen so both 1920 and 1080 divide evenly; adjustable if a
finer grid turns out to be needed). A widget instance occupies a rectangle of whole cells
(`grid_x, grid_y, grid_w, grid_h`, 0-indexed, bounds-checked against 80x45) plus a `z_order` for
overlapping instances. No rotation, no free-angle resize in v1.

**Persistence**: local DB, consistent with the goal tracker (covered by the existing Export/Import
backup). Two tables:
- `overlay_layout` (`id`, `name`, `created_at`, `updated_at`)
- `overlay_widget_instance` (`id`, `layout_id` FK, `widget_type` text — one of the 10 above,
  `grid_x`/`grid_y`/`grid_w`/`grid_h` int, `z_order` int, `settings` jsonb — per-widget options that
  today come from the URL query string on the standalone page, e.g. `/widget?me=`, `/now-playing?mini`)

## Design

**Widget content is already separable.** Every existing source follows the same shape: `fn mount() {
mount_to_body(Panel) }`, where `Panel` is a `#[component]` that wraps its actual content (a card/pill/
banner div) in `frame::OverlayFrame` (corner anchor + pixel offset, sized for a full 1920x1080 frame).
The refactor needed is mechanical, not a rewrite: split each of the 10 widgets' `Panel` component into
- a content function (`fn goal_content(settings: GoalSettings) -> impl IntoView`, the inner div only,
  taking its options as a plain struct instead of reading the URL query string directly), and
- the existing standalone `mount()` keeps building that same content and wrapping it in `OverlayFrame`
  for backward compatibility (the 10 fixed URLs behave exactly as before — this spec does not remove or
  change them).

**New custom-overlay renderer**, registered as one more `Page` variant (`Page::CustomOverlay`, the
layout id read as a query parameter — `/overlay/custom?id=<uuid>` — same pattern `?me=` already uses,
since `Page` is a simple fieldless enum matched by path): fetches the layout definition from a new
public, unauthenticated endpoint (`GET /api/overlay-layouts/{id}`, no admin token — same posture as every
other OBS-facing endpoint, since OBS Browser Sources cannot send custom headers), then for each widget
instance renders its content function inside an absolutely-positioned `<div style="position:absolute;
left:{x*24}px; top:{y*24}px; width:{w*24}px; height:{h*24}px; z-index:{z}">`, passing the instance's
`settings` as that widget's options struct. Lives in the `sources` bundle (it needs every widget's
content function, same as the fixed pages already do) — no new Trunk bundle.

**Builder UI**: new page `/overlay-builder` in the **app** bundle (admin-token gated like other
write-capable settings pages — `net::write`, same contract as Admin's existing writes), not in `sources`
(it's an editing tool, never meant to be an OBS source itself):
- Layout list (create/rename/delete) + an editor for the selected layout.
- Widget palette (the 10 types, each with a short label/icon).
- Canvas: a `<div>` grid showing the 80x45 cells at a browser-scaled-down size (same "design at
  1920x1080, scale to fit" approach the existing overlay pages use); drag a palette item onto the canvas
  to place a new instance at the nearest free cell; drag an existing instance to move it; drag its corner
  handle to resize (both snapped to the grid); click an instance to open its settings panel (the options
  that widget's `settings` struct exposes) and a delete button; a live preview renders each instance's
  actual content function at its current grid rect (so the canvas shows real widget output, not
  placeholders).
- "Copy OBS URL" button once a layout is saved, giving `/overlay/custom?id=<uuid>` as an absolute URL
  (same `location.origin` + `route::app_href`-style construction the "This PC" OBS URLs already use).

**Backend**: admin-gated CRUD (`GET/POST /api/admin/overlay-layouts`,
`GET/PUT/DELETE /api/admin/overlay-layouts/{id}`, same contract as other Admin writes) plus the public
read endpoint above. One migration (next number after `0019_app_goal.sql`).

## Out of scope (v1)

True pixel-freeform placement, rotation, `/cast/overlay`/`/draft/overlay` as movable widgets, sharing a
layout between machines (export/import of a single layout as JSON — plausible follow-up, not v1), any
new widget content (this spec composes the 10 that already exist; new widget ideas are their own spec),
multiple canvas resolutions/aspect ratios (locked to 1920x1080, matching every other OBS source).

## Budgets and acceptance

- No bundle-size budget change expected: the custom-overlay renderer reuses existing widget code already
  linked into `sources`; the builder page is one more `app`-bundle page, well under the 1.25 MB cap.
- Existing 10 fixed OBS source URLs behave identically before and after (regression-checked).
- A layout with 2+ overlapping/adjacent instances renders all of them at their saved positions, respecting
  `z_order`; an instance's settings round-trip through save/reload.
- The builder's drag/resize always snaps to a whole grid cell; an instance cannot be placed or resized
  outside the 80x45 bounds.
- `GET /api/overlay-layouts/{id}` for an unknown id returns 404 (the custom-overlay page shows a plain
  "Layout not found" message, consistent with `/recap`'s own unknown-session handling); no admin token
  required.
- Admin-gated endpoints reject writes without a valid token, same as other Admin writes.
- Browser-checked: build a layout with at least 4 widget types in the builder, save it, open
  `/overlay/custom?id=<uuid>` in a second tab and confirm it matches the builder's preview exactly.

## Implementation plan

Two phases, because the renderer and the builder both depend on the widget-content refactor and the new
tables, but not on each other once those land:

1. **Phase A (foundational, do first)**: migration + CRUD/read endpoints, the content-function refactor
   across all 10 widget files (mechanical, one PR, regression-checked against the existing fixed URLs).
2. **Phase B (parallel once A is merged)**: the custom-overlay renderer page (`sources` bundle) and the
   builder UI page (`app` bundle) — independent files, no shared-file conflict risk between them.

## Phase A backend implementation notes

Migration 0020 and the REST/wire contract are documented in `docs/spec/05-api.md` and
`docs/spec/06-modele-donnees.md`. Both future clients use `storm_web_core::overlay_layout`. `PUT`
replaces the complete widget set, keeps the layout UUID stable, and regenerates widget IDs; this
is not a patch API. Draft IDs stay stable across local edits, not across saves. The server, editor
and renderer share typed wire settings. The database checks the per-widget allowlist even during
data-only restore, and reads fail closed if stored widget settings are invalid. The
[storage ADR](../adr/2026-11-20-overlay-layout-persistence.md) records the approved persistence and
access model. Widget-content changes are tracked separately from this backend slice.

## Implementation notes — Phase A widget slice

The ten `*_content` functions in `web-rs/sources/src` take reactive inputs and return only widget
content. The fixed-page adapters still choose endpoints, WebSocket events, polling cadence and the
outer frame. `/queue` already was a single 1920×1080 scene (camera/game slots and music included),
so its intrinsic-size content is preserved without an extra `OverlayFrame`. A future placement host
must choose the same scale, clipping and shadow/animation overflow rules for preview and output.

Only `/widget` and `/now-playing` have URL-derived options. The adapters map `?me=` to `WidgetOptions`
(first decoded value, then the existing configured-operator/first-player fallback) and the music flags
to `NowPlayingMode` (full by default; flag presence selects mini or reveal; reveal wins if both occur).
The renderer can construct these types from validated instance settings rather than synthesizing
query strings. For saved music settings, absent and `false` are disabled and `true` on both selects
Reveal. This differs intentionally from flag presence on fixed URLs.

**Multi-instance lifecycle.** A compositor should own the feeds above the individual views. Share
hero dimension context (required by ticker, widget, queue, recap and stream pool), settings, match
lists by limit, latest review, and music by cadence; use one WebSocket per composition, not one per
placement. Do not reuse the standalone polling/reconnect adapters as per-instance hooks: they assume
document lifetime. Dispose the composition's subscriptions and timers when it ends and ignore stale
async results (notably lobby changes during opponent lookups). Keep a separate reactive owner per
stable instance ID. Moving or resizing should not restart timers; replacing settings may remount
that instance. Music's progress/reveal timers already clear with their owner; an already queued
short ticker timeout can finish but cannot write to a disposed signal. The host supplies record
flash visibility and patch visibility, rather than having either view query or acknowledge on mount.

**Preview and OBS.** The fixed patch page alone retains `patch_banner_last_seen`. A same-origin iframe
shares local storage: preview must use in-memory acknowledgement and must not consume the fixed
page's or a live layout's banner. Decide a per-layout/per-instance live key before composing banners.
Prefer one `sources` document for the live canvas and one corresponding iframe for preview, not one
iframe per widget or a second renderer. Its CSS includes document-wide rules and local fonts; do not
load those styles into the app document. Use the same transparent 1920×1080 canvas and scale the
preview as a whole. Preserve stable instances across draft updates; if communicating with an iframe,
check the expected origin and window, and never send admin tokens. Test triggered content under the
same event fixtures in both hosts, and check a real OBS Browser Source separately from Chromium
(including scene hide/show, page refresh and alpha over underlying video). No renderer/editor was
implemented in this slice.

### Fixed-route regression

The source options have native tests: `cargo test --locked -p storm-web-sources --lib` from `web-rs`.
To compare release outputs, build `web-rs/sources` with `trunk build --release --locked` at the base
and candidate revisions. With Playwright 1.65.1 installed in an isolated npm directory and its
Chromium shell installed (`playwright install chromium --only-shell`), run from the repository root:

```sh
NODE_PATH="$test_dir/node_modules" PLAYWRIGHT_BROWSERS_PATH="$test_dir/browsers" \
  node web-rs/sources/tests/fixed-sources.cjs "$base_dist" "$candidate_dist"
```

The script serves both builds and deterministic API/WS responses from browser routes only. It
compares the normalized DOM and requests for populated, empty and `/v2` route variants, asserts
legacy query precedence and clock-driven behavior, and samples a transparent background pixel.
For markup comparison it removes comment nodes from a detached body clone before serializing it.
The harness first checks that this preserves literal comment-like text and element order without
changing the live DOM. It does not replace real-data or real-OBS acceptance.
