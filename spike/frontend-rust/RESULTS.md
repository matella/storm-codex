# Step 0 spike — results (in progress)

Spec: `docs/specs/2026-11-09-rust-frontend-migration-design.md` §7. Throwaway code, own workspace
and lockfile. Measured on the Windows gaming PC (rustc 1.94, Chromium-based Browser pane, local
server on `127.0.0.1`, dev Postgres). **Not** measured in a real OBS browser source: that is the
operator's check and the go/no-go gate.

## Status

| Item | Leptos 0.8.23 | Dioxus 0.7.10 |
|---|---|---|
| Cast overlay: `ingame` + `series` (map strip) | done | done |
| Cast overlay: post-game slides, `lineups`, `draft` | not ported (slide > 0 shows a placeholder) | not ported |
| Player profile (`/player/:toon`) | not started | not started |
| Cast control room, reduced (`/cast`) | not started | not started |
| Canvas + `MediaRecorder` check | not started | not started |
| Sanitising check | not started | not started |
| Real OBS browser source | operator | operator |

Both overlays are built from one framework-neutral crate (`common/`: wire model, pure logic, `fetch`
+ WebSocket with reconnection, 10 s poll). Only the view layer differs. 10 native unit tests cover
the logic (ported from `castState.ts`: score, badge number R9/R22, map card priority, strip layout,
slideshow tick, scene transition and fade layers, tab anchors, map slug, JSON shape with extra fields).

## What was checked on the overlay (both frameworks)

- Rendered DOM of the `series` scene against the real server and data is the same as React's, apart
  from attribute order and framework comment nodes (compared as `innerHTML` in the same state).
- Transparent body (`rgba(0, 0, 0, 0)`).
- Scene change keeps the outgoing layer mounted with class `out` for the fade and removes it after
  about 300 ms (sampled at 60, 210, 410 ms…), like React.
- WebSocket: a `cast.updated` is applied live; after a server restart under an open page, the page
  reconnects on its own and applies the next change (no reload).
- Not checked: slideshow rotation (needs a game with a replay summary), skins (`?skin=`), a 1920×1080
  screenshot diff against React. The visual match rests on the DOM + the same CSS file.

## Sizes (overlay only, release, `opt-level = "z"`, LTO, wasm-opt `-Oz`)

| | wasm raw | wasm gzip -9 | JS glue raw / gzip | CSS raw / gzip | transferred (server compression) |
|---|---|---|---|---|---|
| Leptos | 294 kB | 122 kB | 38 kB / 7.1 kB | 11 kB / 3.1 kB | ≈ 128 kB |
| Dioxus | 522 kB | 216 kB | 44 kB / 10.9 kB | 9 kB / 2.5 kB | ≈ 220 kB |
| React (today) | — | — | 102 kB + 14 kB of chunks | 26 kB + 6 kB | ≈ 143 kB |

Proposed limit: overlay ≤ 300 kB gzip. Both fit; Leptos leaves a lot of margin, Dioxus about 27 %.
These are *two* scenes; `lineups`, `draft` and the post-game slides will add code (to be re-measured
when ported). Per-route splitting was not tested yet (the overlay is a single entry).

## Timing

- Cold origin (`localhost`, first visit, assets over loopback): last asset byte received at about
  396 ms (Leptos), 473 ms (Dioxus), 443 ms (React chunks). Local network only.
- Warm: the data request starts about 20 ms after navigation for both Rust apps (React: 334 ms,
  because its JS chunks load in sequence).
- **Time to first scene on a cold load, in Chrome and in OBS, was not measured end to end** (the
  Browser pane was hidden, so paint timings were empty). The assets arrive in under 0.5 s on
  loopback and the first scene needs one more `/api/cast` round trip of a few ms plus wasm
  compile/instantiate, which was not isolated. To be measured in OBS by the operator.

## Size of the code

| | lines |
|---|---|
| Shared Rust crate (model 83, logic 230 + tests, net 70) | 383 (+ 133 test lines) |
| Leptos view layer | 165 |
| Dioxus view layer | 182 |
| React equivalent: `CastOverlay` + `InGameScene` + `SeriesScene` + `SceneBackdrop` + `useSlideTick` + `anchors` | 196, plus about 250 of `castState.ts`, 60 of types and 60 of `useLiveUpdates` it relies on |

About the same size as React once the shared logic is counted; no hand-written JavaScript in any
of it (everything goes through `gloo` / `web-sys`).

## Dev loop

`cargo build --target wasm32-unknown-unknown` after touching the view file: 1.4 s (Leptos), 1.2 s
(Dioxus). This excludes wasm-bindgen / bundling, which `trunk` and `dx` add. Release build from
cold dependencies: Leptos about 1 min of compile + wasm-opt, Dioxus about 90 s.

## Friction met (worth knowing for the port)

- `trunk` 0.22.1 needs rustc 1.96 (this machine has 1.94): 0.22.0-beta.1 was installed. Its bundled
  wasm-opt 123 rejects what rustc 1.94 emits unless
  `--enable-bulk-memory --enable-nontrapping-float-to-int ...` are passed (`data-wasm-opt-params`).
  `strip = true` in the profile also removes the target-features section (warning from wasm-bindgen).
- `dx` downloads `wasm-opt` from GitHub on the first build: a network dependency for CI.
- Leptos `<For>` needs `Hash` on the key; needed a derive on the shared view type.
- Dioxus `rsx!` does not accept an `if` expression inside an interpolated class string: moved to a
  helper function.
- Dioxus `asset!` copies the CSS into its output with a hash (and minifies it); the stylesheet is
  therefore duplicated in `dioxus-overlay/assets/` (a symlink is awkward on Windows). Leptos reads the
  shared file through a trunk `data-trunk rel="css"` link.
- The server's existing SPA fallback (`WEB_DIR`) serves both builds unchanged, so `/cast/overlay`
  works with no server change.

## Not yet answered

- How each Vitest test of the screens translates for *render-to-string* tests (only the pure-logic tests were
  ported, as native `cargo test`).
- The three remaining screens, canvas / `MediaRecorder`, sanitising.
- Real OBS (CEF) behaviour: wasm support, fonts, performance.

## Reading so far

On the gating question (overlay size and time), both frameworks fit the proposed 300 kB budget,
Leptos with a wide margin. That is not a go: the overlay has two of its five scenes, OBS was not
tested, and the other screens are not built.
