# Overlay builder: simulate conditional widgets (2026-11-23)

Date: 2026-11-23 · Status: **DRAFT**, defaults proposed below (operator `clarify` timed out with no
response; proceeding on the recommended option in each, flagged for override).

## Problem

Three widget kinds are condition-triggered by design (`docs/specs/2026-11-20-streamer-widgets.md`):
Patch Banner (visible up to 7 days after a new, not-yet-seen patch), Nemesis (visible only while the
live lobby has an opponent at a ≤35%-over-≥3-games record), Record Flash (visible for ~8s right after
the newest game sets a personal best). The overlay builder's live preview (`/overlay/custom?preview=1`
iframe) renders these against real local-server data with no fixture override, so outside those exact
moments the builder canvas shows the same blank `<div></div>` the OBS source itself would — making it
impossible to see, position or style these three widgets while building a layout.

## Design

**Scope**: builder-only. No change to the real OBS output, the single-purpose fixed source URLs
(`/patch-banner`, `/nemesis`, `/record-flash`), or any backend endpoint.

**Control placement** (default: option A from `clarify`, no response received): a "Simulate" toggle
button in the inspector panel (`web-rs/app/src/overlay_builder/inspector.rs`), shown only when the
selected widget's kind is `PatchBanner`, `Nemesis` or `RecordFlash`. Each of the three widget kinds has
its own independent toggle state (selecting a different widget does not reset another's simulated
state); only applies to widgets actually placed in the current draft layout.

**Sample content** (default: obviously-fake placeholder, no response received): a clearly-fake,
hardcoded sample value per kind, never fetched from the real archive:
- Patch Banner: "Sample Patch 2.58" (fixed string, not parsed from `/api/patches`).
- Nemesis: "SampleRival" with a fixed "2W–5L" record (not a real toon/account).
- Record Flash: "Kills · 14" (a plausible stat/value pair, not read from `/api/records`).

**Duration** (default: stays visible until toggled off, no response received): the simulated widget
renders indefinitely once toggled on, with no 8s auto-hide (Record Flash's real `FLASH_MS` timer is
bypassed in simulate mode) — easier for positioning/styling without racing a timeout. Toggling off (or
removing the widget, or closing the builder) clears it immediately.

**Mechanism**: the builder (parent window) already drives the preview iframe via `postMessage`
(`storm-overlay-preview`/`storm-overlay-preview-ready`, same-origin checked, `composition.rs`). Add a
new message kind `storm-overlay-simulate` carrying `{kind: "patch-banner" | "nemesis" |
"record-flash", active: bool}`. `Feeds` (`web-rs/sources/src/feeds.rs`) gains a `simulate: RwSignal<...
3 bools ...>` (or equivalent small struct) that the composition's existing preview-message listener
sets; `patch_banner_content`/`nemesis_content`/`record_flash_content` (or their `Feeds`-fed call sites
in `composition.rs`/`placement()`) check the simulate flag first and render the fixed sample view when
set, before falling through to the real signal. No change to the real per-widget signals themselves
(`patch`, `grudges`, `flash` in `Feeds`) — simulate is a render-time override, not a data override, so
toggling it off always reverts to showing real current state with no stale data risk.

The inspector's Simulate button posts the message to the iframe (needs the iframe's `window` reference,
already held by `canvas.rs` for the `ready_event`/settle logic — may need lifting that ref or plumbing
the postMessage call through `Editor`/a shared signal the canvas component reads and forwards).

## Out of scope

Simulating any of the other widget kinds (they already always render something representative from
real local data, e.g. a zero-game ticker still shows the pill). Any backend change. Changing what the
real single-purpose OBS source URLs show. Making the simulated state persist across a builder reload
(it resets to off every time, by design — no new `localStorage` key).

## Acceptance

- With a Patch Banner / Nemesis / Record Flash widget placed and selected, clicking its Simulate
  toggle shows the fixed sample content in the builder's preview iframe immediately; toggling off (or
  deselecting then re-selecting and toggling off) returns to the real (possibly blank) state.
- Simulating one widget kind does not affect the real signals used by any other widget in the same
  composed layout (e.g. simulating Nemesis does not change what Record Flash or the real OBS source
  for Nemesis itself show).
- The real fixed-URL sources (`/patch-banner`, `/nemesis`, `/record-flash`) and the real
  `/overlay/custom` output (no `preview=1`) are completely unaffected — simulate only ever applies
  inside the `?preview=1` iframe driven by the builder.
- No bundle-size budget change expected (small addition to existing `sources`/`app` code, no new
  dependency).
- Native tests for the new message parsing/dispatch (valid kind/active combinations, an unknown kind
  ignored) where testable without a DOM; browser-checked for the full toggle-on/off/reselect flow.
