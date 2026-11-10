# Step 0 spike — results (in progress)

Spec: `docs/specs/2026-11-09-rust-frontend-migration-design.md` §7. Throwaway code, own workspace
and lockfile. Measured on the Windows gaming PC (rustc 1.94, Chrome 152 in the Browser pane, local
server on `127.0.0.1`, dev Postgres). **Not** measured in a real OBS browser source: that is the
operator's check and the go/no-go gate.

To run: build with `trunk build --release` (Leptos, in the app folder) or
`dx build --release --platform web` (Dioxus), then `./serve.ps1 -Port 8095 -Dist <dist folder>`
(starts the real server on top of the build; needs the dev Postgres and a debug server build).

## Status

| Item | Leptos 0.8.23 | Dioxus 0.7.10 |
|---|---|---|
| Cast overlay: `ingame` + `series` (map strip) | done | done |
| Cast overlay: post-game slides, `lineups`, `draft` | not ported (slide > 0 shows a placeholder) | not ported |
| Player profile `/player/:toon` (Stats + Identity tabs, `?tab=`) | done, no Chat section | done, no Chat section |
| Cast control room, reduced (`/cast`) | not started | not started |
| Canvas + `MediaRecorder` check | pass (framework-free, `checks/`) | same crate |
| Sanitising check | pass (`ammonia`, `checks/`) | same crate |
| Real OBS browser source | operator | operator |

All apps share one framework-neutral crate (`common/`): wire models, pure logic ported from
`castState.ts` / `Player.tsx` / `api.ts` (score, badge number R9/R22, map cards, fade layers,
slideshow, win rate, KDA, mode and award labels, hero lookup, tab from query), plus `fetch`,
WebSocket with reconnection and a 10 s poll. Only the view layer differs. 18 native unit tests.

## Cast overlay (both frameworks)

- Rendered DOM of the `series` scene against the real server is the same as React's apart from
  attribute order and framework comment nodes.
- Transparent body; scene change keeps the outgoing layer mounted with class `out` and drops it after
  about 300 ms; a `cast.updated` is applied live; after a server restart under an open page the page
  reconnects on its own and applies the next change (no reload).
- Not checked: slideshow rotation (needs a game with a replay summary), skins, a pixel screenshot diff.

## Player profile (both frameworks)

Compared as a normalised DOM tree (tag, classes, style, key attributes, text) against the React page
for a real player (2,128 games, 90 heroes, 20 recent games):

- Leptos: the first 490 lines are identical (header, tiles, tabs, hero pool of 18 rows, 20 recent
  games). The only difference left is the Chat section, left out on purpose.
- Dioxus: identical too, apart from the Stats / Identity links (absolute `/player/…?` hrefs from the
  typed router instead of React's relative `?` and `?tab=identity`) and the same missing Chat section.
- Checked in both: `?tab=identity` loaded directly selects the Identity tab; clicking a tab changes the
  URL and the content with no page load, back restores the Stats tab; "show all 90" / "top 18" toggles
  18 ↔ 90 rows; hero portraits fall back to initials when the image fails (the dev data has no
  images); an unknown toon renders the empty profile the API returns (HTTP 200), like React.
- Not covered: the "not found" state after a failed request (the API never fails for an unknown toon),
  the Chat section, keyboard activation of rows beyond the handler being wired.

## Canvas, MediaRecorder, sanitising (`checks/`, plain web-sys, no framework)

- Canvas: a 640×640 canvas with a map rectangle and 10 moving dots driven by `requestAnimationFrame`,
  recorded for 2 s with `captureStream(30)` + `MediaRecorder`: one WebM chunk of 75 kB
  (`video/webm;codecs=vp8`, the browser's default when no type is passed; VP9 is also supported).
- Sanitising: `ammonia` 4 (pure Rust) on patch-notes-like HTML plus hostile fragments (`<script>`,
  `onerror`, `onclick`, `javascript:` link, `<iframe>`, `<style>`), `id` attributes kept like
  `keepIds`, result inserted with `innerHTML`: all hostile parts removed, `h2#id`, `ul` and the https
  link kept, nothing executed. The sample is hand-written; the HotsPatchNotes API is on the box and was
  not reachable, so a real patch page was **not** tried.
- Cost: that crate was **710 kB raw / 296 kB gzip** (wasm), almost all of it the sanitiser. That cannot
  sit in a bundle that the overlays load. Options: sanitise on the server (it already holds the HTML),
  or the browser's own sanitiser API, or a separate wasm bundle for the pages that need it.

## Sizes (release, `opt-level = "z"`, LTO, wasm-opt `-Oz`)

| Screen | wasm raw / gzip | JS glue raw / gzip | CSS gzip | total gzip (transferred with server compression) |
|---|---|---|---|---|
| Overlay, Leptos | 294 kB / 122 kB | 38 kB / 7 kB | 3 kB | ≈ 132 kB (≈ 128 kB) |
| Overlay, Dioxus | 522 kB / 216 kB | 44 kB / 11 kB | 2.5 kB | ≈ 229 kB (≈ 220 kB) |
| Overlay, React (today) | — | ≈ 112 kB of JS (transferred) | ≈ 32 kB (transferred) | ≈ 143 kB |
| Profile, Leptos (+ router) | 468 kB / 194 kB | 46 kB / 8 kB | 13 kB | ≈ 215 kB |
| Profile, Dioxus (+ router) | 662 kB / 271 kB | 46 kB / 11 kB | 11 kB | ≈ 294 kB |

Proposed limit for the overlay: ≤ 300 kB gzip. Both fit; Leptos with a wide margin, Dioxus with
about 25 %, and the overlay still has three scenes to add. A single Dioxus bundle holding the profile
would already be at the limit, so the OBS sources would need their own bundle in that case.

Per-route splitting: **Dioxus** works with `--wasm-split` plus `dioxus-router/wasm-split` (the flag is
`--wasm-split` in dx 0.7.10, not the one in the docs): the profile build became a 632 kB main file
plus a 270 kB module for the route, a gain of 4 % for the first load and 36 % more bytes in total,
because the framework runtime dominates; not tried in a browser. **Leptos**: `#[lazy]` / `LazyRoute`
is documented for `cargo leptos --split`, not for `trunk`; not tried (cargo-leptos is not installed).
Practical conclusion so far: one small wasm bundle per page family (OBS sources / app) rather than
route splitting inside one bundle.

## Timing

- Cold origin (`localhost`, first visit, loopback): the last overlay asset byte arrived at about 396 ms
  (Leptos), 473 ms (Dioxus) and 443 ms (React chunks). Warm: the data request starts about 20 ms
  after navigation for the Rust apps (React: 334 ms, JS chunks load in sequence).
- **Time to first scene on a cold load was not measured end to end** (Browser pane hidden, paint
  timings empty), nor in OBS. The profile page timing was not measured either.

## Size of the code

| | lines |
|---|---|
| Shared Rust crate: model 83, overlay logic 230, profile logic 265, net ≈ 90 (+ about 280 test lines) | ≈ 670 |
| Overlay view layer: Leptos 165, Dioxus 182 | |
| Profile view layer: Leptos 222, Dioxus 213 | |
| React: overlay files 196 + about 370 of `castState.ts` / types / `useLiveUpdates` it relies on; `Player.tsx` 110 + the ds components it uses | |

Comparable in size to React once the shared logic is counted (the React `Player.tsx` is 110 lines
because the design-system components live elsewhere; the Rust views inline them). No hand-written
JavaScript in any of it: everything goes through `gloo`, `web-sys` and `js-sys` (`Date.toLocaleString`
for `fmtTime` is the one direct JS call).

## Dev loop

`cargo build --target wasm32-unknown-unknown` after touching a view file: 1.2–1.4 s, excluding
wasm-bindgen and bundling. Cold release builds: Leptos about 1 min, Dioxus 50–90 s.

## Friction met (worth knowing for the port)

- `trunk` 0.22.1 needs rustc 1.96 (1.94 here): 0.22.0-beta.1 installed. Its wasm-opt 123 rejects what
  rustc 1.94 emits unless `--enable-bulk-memory --enable-nontrapping-float-to-int …` are passed
  (`data-wasm-opt-params`). `strip = true` removes the target-features section (warning).
- `dx` downloads `wasm-opt` from GitHub on first build: a network dependency for CI.
- Leptos `<For>` needs `Hash` on the key; a component taking a closure that owns a non-`Copy` value
  must be rebuilt around `Callback` to be callable from a reactive closure.
- Dioxus `rsx!` does not accept `if` expressions or escaped quotes inside interpolated strings: compute
  the string first. `Option<&'static str>` props do not accept a `String` from an `if`: use `String`
  with `#[props(into, default)]`.
- Dioxus `asset!` copies and hashes CSS (and cannot reach outside the crate): `cast-overlay.css`,
  `theme.css` and `ds.css` are copied into the Dioxus crates; Leptos links the shared files with trunk.
- Dioxus sets the page title to "dioxus | ⛺" unless `Dioxus.toml` gives one.
- The server's SPA fallback (`WEB_DIR`) serves both builds unchanged. It reads `index.html` once at
  startup, so the server must be restarted after each rebuild (`serve.ps1` does it, and refuses to
  stop any process that is not `storm-codex-server`).

## Not yet answered

- The cast control room (`/cast`: scene tabs with keyboard behaviour, series rail, "Put on air",
  scaled `<iframe>` preview, WebSocket refresh).
- How each Vitest test translates for render-to-string tests (only pure logic was ported, as native
  `cargo test`; Leptos has SSR rendering and Dioxus has `dioxus-ssr`, neither tried).
- Remaining overlay scenes; real OBS (CEF) behaviour: wasm, fonts, performance; cold time-to-first-scene.

## Reading so far

Both frameworks reproduce the overlay and the profile page with the same DOM as React, using one
shared Rust core, and no raw JavaScript. Leptos is the smaller (overlay ≈ 130 kB, profile ≈ 215 kB
gzip) and Dioxus about 70–80 kB larger on each, with the overlay at roughly three quarters of the
proposed limit before its remaining scenes. The sanitiser must not live in the shared bundle.
This is not a go: the control room, the other overlay scenes and the OBS check are still open.
