# Storm Codex on GitHub Pages — design

> storm-codex spec. Scope chosen by the operator on 2026-09-08 (all four parts).
> **Awaiting approval** before `writing-plans` and implementation.

## Goal

Publish on `matella.github.io/storm-codex` the part of Storm Codex that **can live without the box**:
the docs, the crates, and above all a **playground that decodes a replay entirely in the browser**.
The box only runs in the evening and is only reachable over Tailscale; Pages is online 24/7 and
public. They are two disjoint surfaces, not a mirror.

## What Pages imposes (constraints, non-negotiable)

- **Static only**: no API, no WS, no Postgres, no secrets. Nothing from
  `storm-codex-server` goes there.
- **Public by construction**: the `matella/storm-codex` repo is public (checked) — so every
  published file is readable by anyone, indexable, and a `git revert` does not unpublish what
  has been scraped. **No personal match data by default** (part 4 = explicit opt-in,
  match by match).
- Platform limits: published site ≤ 1 GB, ~100 GB/month of bandwidth (soft), HTTPS
  provided. Deployment by **GitHub Actions** (`actions/deploy-pages`), not via a `gh-pages` branch.
- **No server-side URL rewriting** → deep links of an SPA using `history` routing
  break. Decision: the Pages build uses **`HashRouter`**, not `BrowserRouter` (the box build
  stays unchanged).

## Decisions made

1. **A single `pages.yml` Action** that builds and publishes the four parts under one site,
   by sub-paths: `/` (landing), `/spec/`, `/api/` (rustdoc), `/play/`, `/share/`.
2. **No part modifies the server or the projection.** If a part requires a change in
   `crates/`, it is done by addition (new `storm-web` crate), never by modifying the box's
   hot path.
3. **The playground uploads nothing.** The dropped replay is read by `FileReader`, decoded in wasm,
   and only exists in the tab. No analytics, no outgoing `fetch`, no storage.
4. **Delivery order**: part 1 (docs) → part 2 (rustdoc) → part 3 (playground) → part 4
   (sharing). The first two are YAML and unlock the deployment chain; the third
   is the real work; the fourth depends on the third.

## Part 1 — Docs site

`docs/spec/` (living spec), `docs/specs/` (dated specs) and `docs/research/` rendered as a static
site. Tool: **mdBook** (single binary, zero JS dependency, Markdown rendering close to GitHub,
built-in search). `docs/runbooks/` and `docs/plans/` are **excluded**: they are operational
notes that name hosts and paths on the box.

- `docs/book.toml` + generated `SUMMARY.md` (script `scripts/gen-summary.sh` or a hand-written
  table, decided at implementation depending on volume).
- Acceptance criterion: the site loads, search finds "parser_version", and **no
  page from `runbooks/` or `plans/` is published** (checked by a `grep` on the output
  directory in CI).

## Part 2 — Rustdoc + crates landing page

`cargo doc --workspace --no-deps` published under `/api/`, plus a home page presenting the
four crates (`storm-replay`, `storm-stats`, `storm-lobby`, `storm-replay-viewer`), their publication
status and the parity figures already measured. `storm-codex-server` is **excluded** from the
published docs (`--exclude storm-codex-server`): its items name admin routes and the shape of the
database.

- Acceptance criterion: `/api/storm_replay/index.html` and `/api/storm_stats/index.html` answer
  200, `storm_codex_server` absent from the site.
- Note: docs.rs will take over when milestone 6 publishes the crates; Pages serves **before**.

## Part 3 — "Drop a replay" WASM playground

The central part. A page that accepts a `.StormReplay` by drag and drop and displays the complete
match page — score, talents, chat, 2D viewer — **without a server**.

### Technical go already measured (2026-09-08)

```
cargo build -p storm-stats -p storm-replay-viewer --target wasm32-unknown-unknown --release
→ Finished in 17.59 s, no code changes
```

The whole chain is pure Rust: `nom-mpq` → `bzip2-rs` + `flate2`/`miniz_oxide`, plus `regex`,
`serde_json`, `serde`. **No C dependency**, which was the only blocking risk. The residual
risk is **size and time**, not feasibility — it is handled by the budgets below.

### Architecture

- **New crate `crates/storm-web`** (`crate-type = ["cdylib", "rlib"]`, `wasm-bindgen`), which
  only exposes two functions:
  - `parse_match(bytes: &[u8]) -> Result<JsValue, JsValue>` → the `{match, players}` object **exactly
    in the shape of `GET /api/matches/{id}`**, so the front-end does not have two data models;
  - `parse_replay2d(bytes: &[u8]) -> Result<JsValue, JsValue>` → the shape of
    `GET /api/matches/{id}/replay2d`.
  This crate is an **adapter, not a re-implementation**: it calls `storm_stats` and
  `storm_replay_viewer` as they are. Any output divergence between wasm and server would be a
  parity regression — a test locks it (see criteria).
- **Web Worker**: decoding runs off the main thread (100k game events per replay), the
  file is transferred as a transferable `ArrayBuffer`. The UI shows progress, never freezes.
- **Front-end**: the same SPA, with a **switchable data source**. `web/src/api.ts` is already the
  single layer (convention of `08-frontend.md`) — an adapter
  `source = "remote" | "wasm"` driven by `import.meta.env.VITE_SOURCE` is introduced there. The box build keeps
  `remote` and does not change behaviour.
- **Pages build routes**: `/play` (drop + match page) only. Pages that only make
  sense against the database (Matches, Player, Heroes, Trends, Draft, Admin, OBS overlays) are not
  mounted; the nav does not show them.

### Accepted degradations (no `dim_*` reference data outside the box)

| Data | On the box | On Pages |
|---|---|---|
| 2D viewer minimaps | `/images/minimaps` | **identical** — `assets/minimaps` is committed (5.2 MB), embedded in the build |
| Hero portraits | downloaded from HotsPatchNotes into `images_dir` | **missing** → initials fallback, already handled by `Avatar` |
| Talent names (`dim_talents`) | resolved by `/api/dim/talents` | **de-camelised `talentTreeId`**, already the fallback of `TalentStrip` |
| Ring colour per universe | `useDimHeroes` | neutral |

A possible V2 (**outside this part**): publish a `dim-snapshot.json` generated from the box's API
and loaded on demand, which would restore talent names and portraits. To be costed before
opening it — not in the scope approved here.

### Budgets (contracts, to be measured — not estimated)

| Budget | Value | How it is measured |
|---|---|---|
| Size of the served `.wasm` | **≤ 3 MB gzip** | size of the published file, printed by CI |
| Decoding + stats of a typical replay | **≤ 1 s** on the gaming PC, Chrome | `performance.now()` around the worker call, median over the 4 test replays |
| Initial load of `/play` | ≤ 2 s | browser measurement, unthrottled network |

Reference: 133 ms natively (`docs/spec/03-storm-stats.md`). A ~5× factor in wasm stays within the
budget; beyond 1 s, `wasm-opt -Oz` then, if needed, re-examination of the part.

### Acceptance criteria

1. The **4 committed test replays** (`crates/*/tests/data/*.StormReplay`) dropped into the
   playground produce a complete match page: score, chat, timeline, 2D viewer that
   plays.
2. **wasm ↔ server parity proven**: a test compares, for those 4 replays, the output of
   `storm_web::parse_match` with that of the server path — **strict JSON equality**. A divergence
   is a failure, not a tolerance.
3. The budgets above measured and written into the living spec.
4. **No outgoing network request** during a decode: checked in the network tab (only the
   site's static assets appear).

## Part 4 — Static sharing of a match

Send a game to teammates without opening the box or giving them Tailscale.

- **Mechanics**: an export from the box's match page produces a file
  `share/<slug>.json` (the `{match, players}` object + the `replay2d` block); the file is committed
  to the repo, published by the same Action, and read by part 3's viewer via `/play#/share/<slug>`.
  No new server code: the export is a button that downloads what the API already returns.
- **Privacy decision** (to be settled by the operator, default recommendation): the export
  **pseudonymises the BattleTags of non-operator players** (`Player 2`… ), with a checkbox to
  keep the real names when the game is a consenting team's game. Heroes, stats,
  positions and **chat** cannot be anonymised without emptying the export of its value: the chat
  feed goes out as is, which the operator accepts at export time.
- Acceptance criterion: a match exported from the box opens on Pages, chat and viewer
  included, without any request going to the box; and a pseudonymised export no longer contains
  any BattleTag of the other players (`grep` on the JSON in the test).

## Non-goals (explicit)

- No mirror of the database on Pages, no public aggregated statistics.
- No OBS overlay on Pages (they depend on the WS and on session state).
- No replacement of the box: Pages **never** serves as a fallback for the server.
- No dedicated domain name in V1 (`matella.github.io/storm-codex` is enough).

## Documentation impact (rule no. 0)

| File | What changes |
|---|---|
| `docs/spec/09-operations.md` | new `pages.yml` Action, what it publishes, what it excludes |
| `docs/spec/08-frontend.md` | `VITE_SOURCE`, `HashRouter` routing of the Pages build, `/play` route, degradations |
| `docs/spec/01-architecture.md` | measured wasm budgets; Pages set as a disjoint public surface |
| `docs/spec/README.md` | pointer to the `storm-web` crate |

## Open questions for the operator

1. **Part 4**: pseudonymisation by default — agreed, or real names by default?
2. **Site name**: `matella.github.io/storm-codex` or a domain of yours later?
3. Publish the test **mini-corpus of replays** on Pages so the playground has a
   "try with an example" button — these files are already committed to the public repo, so it is
   zero additional exposure. Recommendation: yes.
