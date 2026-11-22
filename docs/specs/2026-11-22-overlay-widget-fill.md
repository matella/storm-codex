# Overlay builder: widgets fill their placed box

Date: 2026-11-22 · Status: **approved** by the operator in chat (2026-11-22: uniform scale, the
operator's own recommended option).

## Problem

In the overlay builder (`docs/specs/2026-11-20-overlay-builder.md`), a widget instance occupies a
grid rectangle (`grid_x/y/w/h`), rendered as an absolutely positioned `<div class="custom-placement"
style="width:{w}px;height:{h}px">` (`web-rs/sources/src/composition.rs::placement`). Only
`WidgetSettings::Queue` scales its content to that box today (`transform:scale` to the smaller of
width/height ratio, preserving its 1920x1080 design). Every other widget's `*_content` function
renders at its own intrinsic CSS size: a box bigger than the content leaves empty space, a box
smaller clips it (`.custom-placement { overflow: hidden; }`). The operator asked for every widget
to fill the box it's given.

## Design

Apply the same approach `Queue` already uses — uniform scale, not stretch/reflow — to every widget
kind rendered by `composition.rs::placement`: ticker, widget, recap, stream-pool, patch-banner,
record-flash, nemesis, goal (8 remaining of the 9 non-queue-exempt kinds; queue keeps its existing
code path unchanged).

Each of these 8 widgets has a fixed intrinsic design size today (its natural rendered box at 1x,
e.g. a pill or card's natural width/height with no responsive CSS) — call it `(iw, ih)`. Add a
`natural_size(&WidgetSettings) -> (f64, f64)` helper (or a per-kind constant table) in
`composition.rs` reporting each kind's current intrinsic pixel size, measured from its existing
CSS (no CSS changes — this reads sizes that already exist, it does not redesign any widget).
Generalize the existing `fit` closure (currently queue-only, scaling against the fixed 1920x1080
`WIDTH`/`HEIGHT`) into one shared helper used for every kind:

```
scale = min(grid_w*GRID_CELL_PX / iw, grid_h*GRID_CELL_PX / ih)
```

Wrap every widget's content in a scaled inner `<div>` (`transform:scale({scale});transform-origin:
top left`), sized to its own natural `(iw, ih)` before the transform, inside the existing
`.custom-placement` box (which stays the real grid-cell size and keeps `overflow:hidden` as a
backstop for any rounding at the edges). This mirrors Queue's existing pattern exactly, just
generalized to a per-kind natural size instead of the one fixed 1920x1080 case.

No change to `WidgetSettings`, the wire contract, migrations, the builder's placement/resize
logic, or any fixed OBS source URL (`/ticker`, `/widget`, etc. keep their current unscaled
rendering — this only changes the **custom-overlay composition renderer** and its preview, i.e.
layouts built in `/overlay-builder` and viewed at `/overlay/custom?id=`).

## Acceptance

- Every one of the 8 widget kinds, placed at a box smaller than its natural size, than larger than
  its natural size, and than its natural aspect ratio, renders fully visible and centered-by-origin
  with no clipping and no empty margin beyond what the aspect mismatch requires.
- Queue's existing behavior is pixel-identical to before (regression check against current code).
- The builder's live preview (same renderer, iframe) reflects the same scaling as the real OBS
  output for an unchanged layout.
- No bundle-size budget change expected (reuses existing `sources`-bundle code, no new dependency).
- Native tests for the new scale-factor helper (exact fit, under, over, non-matching aspect ratio).
- Browser-checked: a layout with at least 3 non-queue widget kinds at non-default box sizes,
  compared between the builder preview and a second tab opening the real `/overlay/custom?id=`
  URL.

## Out of scope

Per-widget stretch/reflow (rejected by the operator in favor of uniform scale), changing any
widget's own CSS/design, the 9 fixed single-purpose OBS source URLs, `/cast/overlay` and
`/draft/overlay` (not builder widgets in v1), multiple canvas resolutions.
