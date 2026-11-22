# Overlay builder: widgets fill their placed box

Date: 2026-11-22 · Status: **approved** by the operator in chat (2026-11-22: uniform scale, the
operator's own recommended option). **Amended 2026-11-22**: a fixed per-kind size table does not
work (see Implementation notes) — replaced with runtime measurement before the implementation
dispatch below ran any code, so no prior version of this design was ever committed as code.

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

Each of these 8 widgets renders content whose size can depend on live data (e.g. stream-pool's
height grows with the number of heroes shown) — there is no fixed per-kind intrinsic size that is
always correct. Instead, measure each instance's **actual unscaled rendered size** at runtime:
render its content once, unscaled, inside an off-screen or zero-opacity measuring pass (e.g. a
hidden sibling, or the real node observed via `ResizeObserver`/`getBoundingClientRect` on mount and
on every content update), then apply:

```
scale = min(grid_w*GRID_CELL_PX / measured_w, grid_h*GRID_CELL_PX / measured_h)
```

to a `transform:scale({scale});transform-origin:top left` wrapper around the same content, inside
the existing `.custom-placement` box (still the real grid-cell size, `overflow:hidden` stays as a
backstop for rounding at the edges). Re-measure and re-derive `scale` whenever the content's
natural size could have changed (new data, not just a resize of the placement box, which is already
handled by the existing `grid_w`/`grid_h` reactive style). Queue is exempt and unchanged: its content
is already a fixed 1920x1080 design, so its existing fixed-constant `fit` closure is correct as is
and does not need runtime measurement.

No change to `WidgetSettings`, the wire contract, migrations, the builder's placement/resize
logic, or any fixed OBS source URL (`/ticker`, `/widget`, etc. keep their current unscaled
rendering — this only changes the **custom-overlay composition renderer** and its preview, i.e.
layouts built in `/overlay-builder` and viewed at `/overlay/custom?id=`).

## Implementation notes (2026-11-22)

The first implementation attempt found that a static per-kind `(iw, ih)` table (as originally
drafted above) cannot be correct: Chromium measured the real `stream-pool` widget at 270×85 CSS px
with one hero shown and 270×481 with twelve — the same widget kind, two different natural sizes
depending on live data. Any other data-driven widget (recap, ticker, goal, etc.) can have the same
property. The design above was amended to measure actual rendered size at runtime instead of a
fixed table, before any code was written against the old table-based design, so there is no
regression to reconcile.

## Acceptance

- Every one of the 8 widget kinds, placed at a box smaller than its measured size, than larger
  than its measured size, and than its measured aspect ratio, renders fully visible and
  centered-by-origin with no clipping and no empty margin beyond what the aspect mismatch
  requires. For a widget whose content size changes live (e.g. stream-pool's hero count), the
  scale updates to match without a page reload.
- Queue's existing behavior is pixel-identical to before (regression check against current code).
- The builder's live preview (same renderer, iframe) reflects the same scaling as the real OBS
  output for an unchanged layout.
- No bundle-size budget change expected (reuses existing `sources`-bundle code, no new dependency).
- Native tests for the new scale-factor helper (exact fit, under, over, non-matching aspect ratio)
  using measured dimensions as fixed inputs to the pure scale-factor function (the measuring
  itself is a DOM concern, not unit-testable in `cargo test`).
- Browser-checked: a layout with at least 3 non-queue widget kinds at non-default box sizes,
  compared between the builder preview and a second tab opening the real `/overlay/custom?id=`
  URL.

## Out of scope

Per-widget stretch/reflow (rejected by the operator in favor of uniform scale), changing any
widget's own CSS/design, the 9 fixed single-purpose OBS source URLs, `/cast/overlay` and
`/draft/overlay` (not builder widgets in v1), multiple canvas resolutions.
