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
