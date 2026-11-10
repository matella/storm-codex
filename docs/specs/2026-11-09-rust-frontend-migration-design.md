# Rust front-end migration — design

**Date:** 2026-11-09
**Status:** draft — awaiting operator validation
**Language:** English, at the operator's request (the rest of `docs/` is still French).
**Follow-up (separate spec, not this one):** a Windows installer so other players can run Storm
Codex locally (pick the replays folder, import everything, watch for new games). This migration
comes first so the installer packages the final front-end only once.

## 1. Intent

Replace the React/TypeScript app in `web/` with a front-end written in Rust, compiled to
WebAssembly and still served by the existing Axum server. The operator wants a Rust-first
codebase: one language and one toolchain from replay decoding to the screen.

This is a port, not a redesign. A player or caster should not be able to tell which front-end is
running, except perhaps by load time.

## 2. Goals

1. Every route in `web/src/App.tsx` works the same in the Rust front-end (§5 inventory).
2. The six OBS sources keep their exact URLs and query parameters, render transparently at
   1920×1080, and keep their timing (scene fade ≈ 300 ms, WS-driven updates).
3. The server side does not change: same REST endpoints, same WebSocket events, same database,
   same `ADMIN_TOKEN` behaviour. The front-end is still static files served from `WEB_DIR` with
   the SPA fallback.
4. Nexus Codex visual parity: same tokens, fonts (self-hosted), `ds-` classes, layouts.
5. The test suite is carried over: every Vitest test (302 today) has a Rust counterpart before
   React is removed.
6. React stays live and deployable until the Rust front-end has reached parity and the operator
   approves the switch.

## 3. Non-goals

- No change to `storm-replay`, `storm-stats`, the server's business logic, the API or the schema.
  If the port reveals an API gap, it gets its own small change under the normal rules (§10).
- No redesign, no new features during the port.
- No desktop shell, installer, local database or folder watcher (follow-up spec).
- No Rust chart library: charts are hand-drawn SVG today and stay hand-drawn SVG.
- No translation of UI strings (already English).

## 4. Anti-goals — the port has failed if

- An OBS scene breaks or glitches on stream, or an existing OBS URL stops working.
- A feature is silently lost (anything listed in `docs/spec/08-frontend.md` that the Rust app
  does not do, without an explicit operator decision).
- The project drifts into months with two half-finished front-ends. The React app is frozen for
  new features once the port starts (§8); fixes are allowed and must be mirrored.
- OBS sources become noticeably slower to appear. Today the cast overlay is an 11 kB chunk on top
  of a 79 kB gzip main bundle; a multi-megabyte WASM file for an overlay is not acceptable.

## 5. What exists today (source of truth: the code)

Measured on `main` at `ee4bd53`:

- `web/src`: 85 TS/TSX files, ~10,500 lines (tests excluded), ~1,170 lines of CSS, 302 Vitest
  tests (node environment, components rendered with `renderToStaticMarkup`).
- Runtime dependencies actually used: React 18, TanStack Query 5, react-router 7, DOMPurify,
  Fontsource fonts. **uPlot is listed in `package.json` but imported nowhere** — dead dependency.
- Charts and diagrams: inline SVG (level advantage, XP lead, trends histogram, meters).
- Browser-specific features the port must cover:
  - `components/Replay2D.tsx`: 2D replay viewer on `<canvas>`, plus clip export through
    `MediaRecorder` (`clipExport.ts`).
  - `components/SafeHtml.tsx`: the single place external HTML (patch notes) is inserted,
    sanitised by DOMPurify.
  - `localStorage` (admin token, cast skin, Live visibility choice, onboarding/what's new) —
    always wrapped so private windows and OBS sources work without it.
  - File uploads (team logos, `.StormReplay` files, scouting imports), downloads, clipboard.
  - WebSocket with 2 s reconnection and query invalidation (`useLiveUpdates` in `api.ts`).
  - Scouting HTML export (`reportHtml.ts`, pure string rendering).
  - Code splitting: one lazy chunk per page so OBS sources stay small.
- Routes: 19 pages under `Layout`, 2 legacy redirects (`/draft`, `/draft/replay` →
  `/cast?scene=draft`), and 6 standalone OBS sources:
  `/widget`, `/queue`, `/ticker`, `/now-playing` (`?mini`, `?reveal`), `/draft/overlay`,
  `/cast/overlay` (`?skin=`). Full behaviour: `docs/spec/08-frontend.md`.

## 6. Decided

| Area | Decision |
|---|---|
| Where it runs | In the browser, as WebAssembly, served by Axum. No native-only UI toolkit (Slint, egui): OBS needs a real web page. |
| Rendering mode | Client-side rendering only (static files, like today). No server-side rendering in Axum. |
| Candidates | Leptos and Dioxus (web target). The step-0 spike decides between them (§7). |
| Server/API | Unchanged. The Rust front-end calls the same JSON endpoints. |
| Shared types | The front-end may depend on server-side Rust types only if they are moved into a small, WASM-compatible crate; it must never pull `sqlx`, `tokio` or `axum` into the WASM build. Whether to share types or keep hand-written ones is the spike's call. |
| Charts | Hand-written SVG components, as today. |
| HTML sanitising | A Rust sanitiser in the front-end (e.g. `ammonia`, if it builds to WASM at acceptable size), or sanitising server-side before the HTML is sent — spike's call. Never inserting unsanitised HTML. |
| Coexistence | During the port, the Rust app is built separately and served under a prefix (`/v2/…`) by the same server. React keeps `/`. Switching is one deployment change (which directory is served at `/`); rolling back is the reverse. |
| React during the port | Frozen for features; bug fixes allowed and mirrored in the Rust app. Removed only after operator approval of the switch. |

## 7. Step 0 — go/no-go spike

Throwaway code in `spike/frontend-rust/` (outside the Cargo workspace, like the existing
milestone-0 spike). Build the same three screens in **both** Leptos and Dioxus, against the real
API of a local server with the dev Postgres and real data:

1. **Player profile** (`/player/:toon`): API loading, tabs (Stats / Identity), tiles, hero bars,
   recent games, loading and error states, URL query parameter (`?tab=identity`).
2. **Cast control room, reduced** (`/cast`): scene tabs with the keyboard behaviour described in
   `08-frontend.md`, the series rail with one team editor, "Put … on air", the scaled
   `<iframe>` preview, WebSocket-driven refresh.
3. **Cast overlay** (`/cast/overlay`): `ingame` and `series` scenes, transparent background,
   300 ms fade in and out while keeping the outgoing scene mounted, slideshow driven by the
   server anchor, WebSocket updates and reconnection.

Plus two small checks, either framework:

- Canvas: draw the 2D replay viewer's map and dots on a `<canvas>` and record a short clip with
  `MediaRecorder`.
- Sanitising: sanitise one real patch-notes HTML page and insert it.

**Measure and record** in `spike/frontend-rust/RESULTS.md`, for each framework:

- WASM + JS size per screen, raw and gzip/brotli; whether per-route splitting works.
- Time until the overlay shows its first scene on a cold load (no cache), in Chrome and in an
  OBS browser source.
- Lines of code compared with the React version of the same screens.
- Incremental rebuild time during development.
- How each Vitest test of those screens translates (pure-logic tests and render-to-string tests).
- Anything that needed raw JavaScript interop, and how much.

**Go if** one framework does all three screens with no visible difference from React, the
overlay passes in real OBS (operator check), and the cast overlay's cold-load size and time stay
within the limits the operator approves from the measurements (proposal: overlay ≤ 300 kB gzip
total, first scene ≤ 1 s on the local network). **No-go** stops the port and React stays; the
results are kept for later. **Operator decision (2026-11-09): an overlay that misses its size or
time limit is a no-go for the whole port.** There is no mixed outcome where the OBS sources stay
in TypeScript or plain JavaScript while the rest moves to Rust; the goal is a single Rust
front-end, so the spike must show the overlays can fit.

The spike ends with a short report and a recommendation; the operator picks the framework.

## 8. Port order (after go)

1. **Foundation**: new crate (e.g. `web-rs/`, excluded from the server's native build or built
   for the `wasm32-unknown-unknown` target only), design tokens and `ds-` CSS reused as-is (the
   CSS files move over unchanged), fonts, router, API layer (fetch + cache + invalidation),
   WebSocket client with reconnection, `localStorage` wrapper, admin `Authorization` header,
   layout and top bar, shared `ds` components, served under `/v2/`.
2. **OBS sources** — smallest pages, highest stakes; each verified in real OBS:
   `/ticker`, `/now-playing`, `/widget`, `/queue`, `/draft/overlay`, `/cast/overlay`.
3. **Read-only pages**: Dashboard, Matches, Match detail (without Replay 2D), Players, Player,
   Heroes, Hero, Synergies, Maps, Trends, Patches, Patch, Hero changes, Leagues, Live.
4. **Interactive pages**: Match detail Replay 2D tab (canvas, clip export), Scouting, Scouting
   report (uploads, in-place editing, HTML export), Admin, Cast control room (with the Draft
   workspace and replay panel), legacy redirects.
   **Checkpoint after step 2 (builder's call, marked as such):** once the OBS sources are ported,
   the builder reports per-page effort and sizes, and the operator decides whether to continue
   or stop. Stopping means React stays at `/`; the ported overlays are discarded or kept only if
   the operator says so.
5. **Switch**: operator approval, serve the Rust app at `/`, keep React buildable for one
   release as rollback, then delete `web/`, its CI job and the Node stage of the `Dockerfile`.

A page counts as ported only when: its behaviour in `08-frontend.md` is reproduced, its tests
are ported and green, it was checked in the browser on real data side by side with React, and
for OBS sources, in real OBS.

Builder's calls on how this works (marked so the operator can overrule):

- **Where checks run:** on the gaming PC, against a local server (OBS runs there too). `/v2/` does
  not need to be deployed to the box during the port. OBS checks use the `/v2/…` URLs; after the
  switch, one quick OBS pass on the final URLs.
- **"Side by side" means** screenshots of React and Rust on the same data and viewport, checked by
  eye and attached to the PR. No automated pixel diff.
- **React fixes during the port:** if the page is already ported, the same PR fixes the Rust page
  too; otherwise the fix is noted in the port checklist in `docs/STATUS.md` so the port includes it.

## 9. Quality gates

- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` stay green
  and also cover the new crate (including its WASM build target in CI).
- Pure logic currently in `castState.ts`, `session.ts`, `scouting.ts`, `players.ts`, `live.ts`,
  `draftReplay.ts`, `replay2d.ts`, etc. becomes plain Rust functions with unit tests.
- Component tests: render-to-string tests where the framework supports them, matching what the
  Vitest render tests check.
- "Rust counterpart" means **the same behaviours are covered**, not a one-to-one mapping of the
  302 tests. Where a component cannot be rendered to a string (canvas, WebSocket), a headless
  browser test (`wasm-bindgen-test`) is acceptable; a manual check alone is not.
- Until the switch, the existing `npm test` and `npm run build` stay in CI unchanged.
- Visual parity: side-by-side screenshots of each page, React and Rust, same data and viewport,
  kept with the PR.
- Performance: per-page WASM size recorded in `docs/spec/08-frontend.md`; overlays within the
  limits approved at step 0.

## 10. Documentation (rule 0)

- Each port PR updates `docs/spec/08-frontend.md` for the pages it ports (file names, components,
  where logic lives). The section is rewritten for the Rust app at the switch.
- Build changes (Dockerfile, CI, dev commands, `.claude/launch.json`) update
  `docs/spec/09-operations.md` and `docs/spec/10-developpement.md` in the same commit.
- `docs/STATUS.md` tracks progress (pages ported / remaining).

## 11. Left to the builder

- Exact crate layout, build tool (Trunk or the Dioxus CLI) and how Docker builds it.
- How the `/v2/` prefix is wired in the server (a second static directory with its own SPA
  fallback is the expected shape).
- Component structure inside the chosen framework, as long as the `ds` component set and CSS
  classes are preserved.

## 12. Open points for the operator

- Approval of the overlay size/time limits after the spike's measurements (the proposed limits
  are binding: missing them ends the port, see §7).
- Installing the toolchain pieces the spike needs on the dev machine and in CI
  (`wasm32-unknown-unknown` target, Trunk or Dioxus CLI).
- Real-OBS checks are the operator's (the agent cannot run OBS).
