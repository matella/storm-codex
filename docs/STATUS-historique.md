# STATUS — history (2026-06-12 → 2026-08-03)

Sections removed from `docs/STATUS.md` on 2026-11-07 to keep it readable; content unchanged (translated to English on 2026-11-09).

## Where we are (2026-06-12)
- **Research finished**: anatomy of SotS + hots-parser, dependency verdicts, engine comparison,
  ecosystem (HeroesProfile alive, HeroesMatchTracker archived). → `docs/research/`.
- **Programme spec approved** (operator + agent review, 2 passes): Rust core, Postgres,
  3 data tiers, full SotS parity in V1 (leagues included), replacement of the local Node
  server, stream widget, scope C (self-hosted V1, open-source, public-ready V2), multi-tokens.
  → `docs/specs/2026-06-12-storm-codex-design.md`.
- **Mockups approved** (14 screens, view-by-view coverage audited against SotS; defeat widget
  enriched after operator feedback). → `docs/specs/2026-06-12-storm-codex-mockup.html`.
- **Milestone 0: GO** (2026-06-12, run on the gaming PC). Rust spike `spike/storm-decode`:
  50/50 replays decoded (header+details+tracker), median **12 ms** / p95 21 ms vs .NET 37 ms
  (1 failure) and Python 161 ms; heroprotocol cross-check 3/3 identical. .NET fallback dropped.
  → report: `docs/research/2026-06-12-jalon0-bench.md` · executed plan:
  `docs/plans/2026-06-12-jalon0-spike-decode.md`.

- **Milestone 1: DONE** (2026-06-12). Crate `crates/storm-replay`: 7 streams decoded (versioned +
  bitpacked + attributes), embedded protocol-gen tables (390 builds → 32 deduplicated),
  unknown-build fallback flagged, typed errors. **Criteria: real archive 2,821/2,821
  decoded (100%, 22 builds 2024→2026); 7-stream bench median 102 ms < 150** (p95 205 ms,
  including a ~50–115 ms incompressible bzip2 decompression floor — point raised with the operator).
  **Bit-exact** parity proven vs heroprotocol (deep-compare 7 streams × 4 replays,
  `tools/crosscheck_streams.py`). Plan: `docs/plans/2026-06-12-jalon1-storm-replay.md`.

- **Milestone 2: DONE** (2026-06-12). Crate `crates/storm-stats`: faithful port of hots-parser
  (3,360 lines of JS) → `{match, players, status}`. **Criterion: automatic field-by-field diff
  vs hots-parser 7.55.7 (Node) — 114/114 green** (79 identical full parses, 35 identical
  rejections on maps missing from the reference's `MapType`). 1 documented and
  favourable tolerance (ping coordinates: storm-stats more correct than the reference). Full parse
  bench **133 ms median** (representative sample, < 150 budget; 151 ms, at budget, on the
  worst case, decode-dominated). Harness: `tools/parity-harness/` (`dump.js`, `diff.py`,
  `tolerances.json`). Report: `docs/research/2026-06-12-jalon2-parite.md` · plan:
  `docs/plans/2026-06-12-jalon2-storm-stats.md`.

- **Milestone 2.5 (prerequisite for milestone 3, operator decision)**: storm-stats extension to the 4 recent ARAM
  maps (Silver City, Lost Cavern, Braxis Outpost, Industrial District) that hots-parser
  7.55.7 rejects — ~30% of the archive. `EXTRA_MAPS`, minimal objective, handlers kept. Diff
  still 114/114 (79 full parses + 25 extension + 10 brawl rejections).
- **Milestone 3: DONE** (2026-06-12, dev against local Docker Postgres). `crates/storm-codex-server`
  (axum 0.8 + sqlx 0.8): upload (token, archive, parse pool, ≤ 2 s/202 semantics),
  idempotent Postgres projection, WebSocket `match.parsed`, REST (matches/match/player/heroes),
  `…/raw` dump + LRU cache, admin (tokens/reprocess/health). **Criteria: archive 2781/2781
  archived+attempted; 99.4% parsed** (failures classified, all legitimate); **backfill 1.8 min < 5**;
  **end of game → page 1.4 s < 5**; **API p95 52 ms < 100**. Concurrent deadlock bug found
  by the backfill and fixed. client-rs re-pointed (Hots-Overlay repo). Report:
  `docs/research/2026-06-12-jalon3-bench.md` · plan:
  `docs/plans/2026-06-12-jalon3-serveur-db-backfill.md`.

- **Milestone 4: base shipped** (2026-06-12). SPA `web/` (Vite + React + TS + TanStack Query),
  Nexus Codex design (mockup tokens), served by the binary (`WEB_DIR` → ServeDir +
  index.html fallback for SPA routing). Data-backed pages verified against the backfilled DB
  (with screenshot): **Session/dashboard, Matches (filterable, real-time WS), Match detail
  (score 10 players ×2 teams, draft), Heroes (sortable aggregate), Maps, Player (hero pool)**.
  New endpoint `/api/maps`. Plan: `docs/plans/2026-06-12-jalon4-front-parite.md`.
  **Still to complete for 1:1 parity**: trends per patch, rankings, teams/leagues/
  collections (manual definitions), admin/import UI, CSV/JSON exports, real universe rings
  (`dim_heroes` from HotsPatchNotes :5001), uPlot timelines in the match detail.

- **Milestone 5: code shipped** (2026-06-12). **Jarvis** emitter (`jarvis.rs`):
  `hots.match.completed` → Redis with spine invariants (tested E2E against local Docker Redis,
  well-formed event). **OBS widget** (`/widget`, transparent background, live WS). **Azure push**
  (`azure.rs`, authenticated outgoing POST — code ready, not tested against the real EBS).
  **Node cutover/decommission**: runbook `docs/runbooks/2026-06-12-bascule-decommission-node.md`
  (box/operator step, in the evening). Plan: `docs/plans/2026-06-12-jalon5-stream-jarvis.md`.
- **Milestone 5: Jarvis integration LIVE + verified E2E** (2026-06-13). Prod backfill done (2007
  matches, 97.3%). The Jarvis bridge is written, deployed and **proven end to end on the box**:
  - storm-codex connected to the `jarvis_default` network; publishes on the pub/sub channel
    `storm-codex:match_completed` (REDIS_URL=redis://redis:6379/0). `postgres` alias collision
    (jarvis-postgres) fixed → DATABASE_URL via the container name `storm-codex-pg`.
  - **Jarvis bridge** (`jarvis/ingest/hots_matches.py`, in the Jarvis repo): SUBSCRIBEs to the channel
    → adapts into a spine Event **`hots.match_completed`** (type with 1 dot; the storm-codex emitter
    produced `hots.match.completed` with 2 dots, **rejected** by the Event model — fixed at the
    boundary). Gated by `STORM_CODEX_MATCHES_ENABLED`. Worker started (`jarvis run … hots_matches`),
    subscribed. + 3 tests (apostrophe preserved).
  - **Real E2E test**: re-upload of an archived replay → parse → storm-codex emit → bridge →
    event `hots.match_completed | storm-codex | info | Silver City | winner 1` **present in the
    `events` table of the Jarvis spine**. ✅
  - `reject_class` made exhaustive (stats_failure/-2, unverified_build/-7) → readable admin.
  - **Proactive FR brief: LIVE + proven** (2026-06-13). Spine worker `jarvis/notify/hots_brief.py`
    (3rd consumer group, start `$`): `hots.match_completed` → FR notification from the
    operator's point of view (identified by `HOTS_PLAYER_NAME=matella`, auto-inferred = present in 2001/2007
    matches). Example proven on a real upload: `🏆 HotS — Victoire — Tyrande sur Silver City — 6/8/3`
    (defeat = high priority). Delivered via the notify channel (`send`/ntfy). **ntfy wiring completed**
    along the way: `NTFY_URL` was empty (onboarding oversight) → `http://ntfy:80`; this also reactivates the
    other Jarvis notifications (reminders…). Gated by `HOTS_BRIEF_ENABLED`. + 5 tests.
  - **Local OBS widget matching the mockup (screen 10)** (2026-06-13). Fixed: it showed
    `players[0]` + a generic "team X wins" → now **operator perspective** via
    `?me=<name>` (browser source: `http://192.168.129.85:5102/widget?me=matella`). Shows
    W/L + hero + map + **K/A/D + KP** (KP = takedowns / enemy deaths; the sum of
    takedowns over-counted). Verified live: "VICTOIRE · Chen · Dragon Shire · 3/17/2 · KP 67%".
    Transparent background OK for OBS. `kills/deaths/takedowns` added to `/api/matches`.
  **Scope decision (operator 2026-06-13)**: **we drop the Twitch cloud / Azure extension**.
  The only overlay will be the **local OBS widget** (`/widget`, already served by storm-codex, zero Azure).
  `azure.rs` stays dormant (not wired). → no more dependency on EBS credentials.
  **Remaining (operator)**:
  - **Subscribe to the ntfy topic** `jarvis-62a7e8eba161` to *receive* the briefs (0 subscribers;
    publication proven). Alternative/complementary option: inject the Jarvis sentence into the widget.
  - Add the widget in OBS (browser source, URL above) + validate on a **real game**.
  - **Decommission of the Node server** (runbook, after validation on a real game).

  ⚠️ **Deployment trap**: `rsync --delete` to `~/apps/storm-codex` **deletes the box's `.env`**
  (gitignored, missing on the source side). ALWAYS `--exclude .env`. Secrets recoverable from the
  live container (`docker inspect storm-codex-server --format '{{range .Config.Env}}…'`) —
  especially `POSTGRES_PASSWORD` (the volume is initialised with it).
- **Milestone 6: ready to publish** — see below.

## D3 — leagues + JSON export (2026-06-13)
- **JSON export** of the matches (follows the filters) ✅.
- **Leagues**: grouping above the teams (column `teams.league`, migration 0004);
  `GET /api/teams` returns the league, `PUT /api/teams/{id}` (re)assigns; **Leagues** page (nav)
  groups the teams by league; creation + assignment in Admin. ✅
- **Remaining**: `dim_talents` (talent reference data from HotsPatchNotes → display of the talents per player
  in the match detail) — next item.

## Orpheus (music) — configured, OAuth pending (operator)
Spotify creds in place (box `.env`), Orpheus restarted, `/api/auth/login` redirects correctly to
Spotify (PKCE, redirect 127.0.0.1:3010). Remaining: the operator authorises via an SSH tunnel
`-L 3010:127.0.0.1:3010` → `http://127.0.0.1:3010/api/auth/login` → Agree. Then the music widget
(built into `/queue` + the `/now-playing` page) shows the track.

## OBS scene "between games" (2026-06-13, verified live)
- **`/queue`** (browser source, transparent background, EN): TONIGHT'S SESSION (W-L, streak, WR),
  WR sparkline, recent games (mode badges + portraits + W/L + KDA), heroes tonight (W-L),
  best game, Jarvis sentence. Session = games of the same calendar day as the last one.
- **`/now-playing`** (persistent source, EN): music widget reading **Orpheus** via the proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Shows "Music — off" while Spotify is asleep.
  Three variants: default (rich card), `?mini` (compact badge), `?reveal` (big announcement
  at each playback start — entry 440 ms, hold 2.6 s, morph 620 ms — then folds onto the
  badge). Spec `docs/specs/2026-08-29-now-playing-reveal-design.md`, plan
  `docs/plans/2026-08-29-now-playing-reveal.md`. Polling changed from 5 s to 2 s.
  **Still to check on the box**: (a) that Orpheus does not relay the Spotify API without a cache — the
  2 s polling makes ~30 requests/min per open source; documented fallback = WS push
  `music.changed`; (b) that the reveal actually shows, which requires the Spotify OAuth to be done
  (see the "Orpheus (music)" section further down). Until then only the geometry is verified,
  by driving the state classes by hand.
- **Widget `/widget`** switched to English (VICTORY/DEFEAT) — consistent. Jarvis's voice stays FR (persona).
- Mockup: `docs/specs/2026-06-13-scene-obs-entre-games-mockup.html` (left panel, modest cam+game
  on the right, persistent music bottom-right).
- **Language: everything in English** (operator decision — option B). All pages + nav + Admin +
  match detail translated; **Jarvis's voice stays FR** (persona). Verified live.
- **Orpheus (music)**: for `/now-playing` to show the track, Spotify still has to be configured
  on the Orpheus side (Premium + dev app + `cargo`/auth) — see the recap in the chat / `Orpheus/SETUP.md`.

## D1 — multi-account operator identity (2026-06-13, verified live)
The **operator_names** setting (list, multi-account) is the single source of "who am I":
- Server: table `app_settings` (migration 0003), `GET /api/settings`, `PUT /api/admin/settings`.
- Front: `useSettings` + `pickOperator`/`matchOperator` → **widget by default without `?me=`**,
  Matches rows (W/L + your hero), **Session = YOUR stats** (real win rate 53.5% vs 50% global,
  my games, heroes played, main hero). Admin UI "My identity" to edit.
- Jarvis: `HOTS_PLAYER_NAME` accepts a **list** (the brief matches any of your names).
- **JSON** export of the matches added (follows the filters), next to the CSV. ✅
**Rest of D**: D2 Jarvis sentence in the widget (round trip); leagues (above the teams);
detailed `dim_talents`.

## Images: hero portraits + map backgrounds (2026-06-13, verified live)
- **Deterministic vendoring**: at startup, `dim::vendor_images` downloads the hero
  portraits (90) + standard map images (15) from HotsPatchNotes into `/data/images`
  (idempotent), served by storm-codex on `/images`. **Self-sufficient**: no runtime dependency
  on HotsPatchNotes (the download fails silently → front fallback). `IMAGES_DIR`.
- **Avatar = real portrait** (ringed in the universe colour), initials fallback if unknown/error.
  `heroIcon()` reads `dim_heroes.icon`. ⚠️ the widget calls `useDimHeroes()` itself (it is outside
  Layout, otherwise DIM is empty → initials + default ring).
- **Map background** on the Matches rows + the widget: `mapImage()` (slug = lowercase name, no
  apostrophe, spaces→dashes) as a veiled `background-image` (gradient → readable text). ARAM maps
  without an image (Silver City, Industrial District, Lost Cavern, Braxis Outpost) → plain background (silent,
  deterministic CSS fallback).

## Widget + front: pass of 2026-06-13 (verified live)
- **OBS widget = operator perspective**: `?me=<name>` (e.g. `/widget?me=matella`) → W/L from
  your team, KDA k/a/d, KP, mode, duration (before: `players[0]` + a generic "team X wins").
  KDA added to `/api/matches`. Matches mockup screen 10.
- **Mode codes fixed** (real bug): the front table was shifted (Storm League showed
  "HL", QM "—"). Checked on the archive: **50091=SL, 50101=ARAM, -1=Custom** dominate.
  No more "—" badge.
- **SotS-style filters** on Matches: mode pills + **map** (20) + **hero** (91) dropdowns,
  sort `played_at DESC` (already in place). CSV export follows the filters.
- **SPA cache fix (root cause)**: `index.html` was served **without `no-cache`** → after each
  redeploy the browser stayed on a stale bundle = **blank page**. Fixed (ServeDir no longer
  serves the index; SPA handler with `no-cache`). Same bug class as HotsPatchNotes.
  ⚠️ Process: a `tsc` failure made `npm run build` fail **silently** (the old
  bundle stayed) — always check `✓ built` at deployment.
- **Reverse proxy + ntfy**: runbook `docs/runbooks/2026-06-13-reverse-proxy-et-ntfy.md`
  (the box uses **Nginx Proxy Manager**, not nginx files; ntfy `:8093`). Remaining for the operator:
  subscribe to the ntfy topic to receive the briefs.
- **Known / refinements**: global operator identity (Session/Matches/Heroes still show
  `players[0]`/aggregate, not you — the widget has `?me=`); Jarvis sentence in the widget; deltas vs
  average. + milestone 4 leftovers (leagues, JSON export, dim_talents).

## Milestone 4 — additions done (2026-06-12, after the base)
`dim_heroes` replicated from HotsPatchNotes at startup (90 heroes/6 universes) → **real universe
rings** on the avatars; **uPlot timeline** of the level advantage on the match
detail; **CSV export** (`/api/matches.csv` + UI link); **enriched** match detail (ordered
draft + first objective/fort/keep). Endpoint `/api/dim/heroes`.

## Milestone 4 — secondary items done (2026-06-12)
**Trends per patch** (win rate/duration per build, `/api/trends`), **teams/collections** (admin-protected
CRUD, `/api/teams`+`/api/collections`), **Admin/Import page** (upload health, token
creation, team/collection management, re-process). Every SotS page has its equivalent.

## Box deployment — DONE (non-invasive, 2026-06-12 evening)
Packaging shipped: multi-stage `Dockerfile` (Vite web + Rust server + slim runtime, migrations
embedded), prod `docker-compose.yml` (dedicated Postgres `storm-codex-pg` + server; opt-in push
left empty), `.dockerignore`, `.env.example`. **Deployed on the box** in `~/apps/storm-codex`
(rsync + `docker compose up -d --build`). Host port **5102** (8088 taken by gluetun). Verified:
both containers **healthy**; `/api/health` 200 (db up); 11 tables migrated; **dim_heroes = 90
heroes replicated from HotsPatchNotes :5001** (via `host.docker.internal`); SPA `/` 200; widget
`/widget` 200; empty DB ready; reachable from the Mac via Tailscale (`192.168.129.85:5102`).
**Non-invasive**: Node overlay, Mongo and Jarvis Redis **not touched** (REDIS_URL/AZURE_* empty);
no decommissioning. The server is in place, healthy, waiting for uploads from the gaming PC.

## client-rs uploader compatibility — DONE (2026-06-12)
The `client-rs` client (Hots-Overlay v1.1.2) posts to **`/api/upload-raw`** (raw bytes,
`X-Filename` header, `Bearer`). storm-codex only exposed `/api/upload` → **alias added**
(`/api/upload-raw` → same handler). End to end proven on the box: token created via admin,
bytes upload → auth → archive → parse → classified status, 401 without a token, admin health OK. The
client therefore works by just re-pointing `SERVER_URL` (no client code change).
Upload token **matella-pc** created (value given in the chat; can be recreated via `POST /api/admin/tokens`
with the `name` field + `ADMIN_TOKEN` from the box's `.env`). Database + archive reset to empty.

## Two fixes (2026-06-12, operator feedback)
- **Backfill truly complete** (Hots-Overlay repo, `client-rs`): the PC commit d3efb77 announced
  "full scan" but `candidates.truncate(INITIAL_UPLOAD_LIMIT=10)` had stayed → only the 10
  most recent were sent. Fixed: `scan_and_upload` sends **everything** not yet uploaded (oldest-first,
  chronological history), idempotent (persisted set + server 409). Cap removed.
- **Apostrophe in the map name** (Blackheart's Bay): the **parsed** name in the database was already safe
  (sqlx bind, never any interpolation; front via URLSearchParams + React escaping). What remained was the
  `X-Filename` header stored percent-encoded (`Blackheart%27s Bay`) → **decoded on the server** + test;
  verified end to end on the box (`… Blackheart's Bay.StormReplay` stored cleanly).

## What remains (real state as of 2026-06-13)
Development is essentially finished (milestones 0→5 shipped + verified; prod backfill done:
2007 matches; Jarvis Redis wired + brief E2E; images vendored; widget/filters). Remaining:

**A. Operator actions (not code)**
- **Subscribe to the ntfy topic** `jarvis-62a7e8eba161` (ntfy app, server `http://192.168.129.85:8093`)
  to *receive* the briefs (publication already proven; 0 subscribers today).
- **Add the widget in OBS**: browser source `http://192.168.129.85:5102/widget?me=matella`.
- (optional) **NPM reverse proxy** for a clean name + TLS — runbook
  `docs/runbooks/2026-06-13-reverse-proxy-et-ntfy.md`.

**B. Final milestone 5 cutover (together, in the evening, on a real game)**
- Play a game → validate the OBS widget + page < 5 s in real conditions (the rest is proven).
- **Decommission the overlay's Node server** — runbook
  `docs/runbooks/2026-06-12-bascule-decommission-node.md`. (Azure/Twitch extension **dropped** —
  operator decision: local overlay only.)

**C. Milestone 6 — publication (crates.io operator action)**
- `cargo publish` storm-replay then storm-stats; public GitHub repos. `--dry-run` validated, MIT in place.

**D. Refinements — ✅ SHIPPED (2026-06-13)**
- ✅ Global operator identity (multi-account `operator_names` setting + `pickOperator`/`matchOperator`
  everywhere: Session, Matches, widget, Jarvis brief).
- ✅ Jarvis sentence in the OBS widget (deterministic FR repertoire, butler voice).
- ✅ Milestone 4 leftovers: **leagues** (/leagues page + Admin assignment), **CSV/JSON export**,
  **`dim_talents`** (reference data of 1918 talents synced from HotsPatchNotes → builds named
  per player in the match page; icons not served by HotsPatchNotes → text in V1).
- ✅ Hero portraits + map backgrounds vendored; UI **entirely in English**.
- ✅ OBS scene `/queue` (1920×1080, session panel + framed cam/game slots + built-in music);
  music widget `/now-playing` (Orpheus proxy, album cover, nested shape handled).
- ✅ **ARAM hero bug fixed**: the ARAM shuffle (3 choices) means the replay's attribute 4002
  sometimes captures an *offered* hero, not the one played (e.g. stored "D.Va" instead of Johanna). The
  projector now derives the played hero by **majority vote of the talents** via `dim_talents`
  (locale-independent, immune to the shuffle), with `hero_id` aligned on the parser's canonical name
  (`attributeId`→`attr.json`) so as not to create false format diffs (Li-Ming, E.T.C.…).
  Backfill: **5557 rows fixed, 0 divergence left**; trace in `heroCorrectedFrom`.

## Test campaign — DONE (2026-08-03, operator request)
Coverage before: 25 Rust tests (mostly draft), 0 front tests, no test CI. Added:
- **storm-stats `tests/invariants.rs`** (4): invariants of the full pipeline on a real committed
  replay — match identity/consistency (winner/teams/duration), **messages** (chronological order,
  authors = players, payload per type chat/ping/announcement, LoadingProgress filtered), score+talents+
  takedowns, monotonic XP timeline.
- **Server**: `auth.rs` factored out (admin/manage debt settled) + 3 tests; Jarvis contract
  (spine invariants + player summary) 2 tests; exhaustive `reject_class` + `game_fingerprint`
  (stability/order) 2 tests; **router-level integration** (`api_tests` in main.rs,
  `api_router()` extracted, tower::oneshot): health, admin auth open/closed 401/201, and
  **end to end real upload → parse → projection → detail with messages → 409 dedup → 401
  without a token**. DB-gated (skipped without DATABASE_URL), run in CI.
- **Front**: vitest (`web/src/api.test.ts`, 13 tests) — fmtDur/fmtClock, announceLabel,
  modeBadge, matchesUrl, heroKey, awardLabel, classBadge, parseTrack (2 Orpheus shapes),
  pickOperator, sideOfStep, initials/universeColor/mapImage fallbacks. `npm test`.
- **CI `ci.yml`**: strict clippy + `cargo test` with a **Postgres 17 service** (the DB integration
  runs on every push/PR) + vitest + web build.
Total: **51 tests** (38 Rust + 13 vitest). Node parity (114 replays) and box E2E stay outside
CI (private corpus/infra) — unchanged.

## Full audit + cleanup — DONE (2026-08-03, operator request)
Complete pass (code, risks, speed, hygiene):
- **Quality**: strict clippy 0 warnings (11 `unwrap`s in the tests of `draft/mod.rs` → `allow`
  attribute in line with the convention); 25 Rust tests green; front build green.
- **Dependency security**: `npm audit fix` → dompurify + postcss (high) fixed. Remaining: react-router 6
  (fix = v7, major decision) and vite/esbuild (dev-only) → "Known risks" of `spec/09`.
- **Code risks**: no SQL interpolation (100% bind), no path traversal (archive
  named by hash, X-Filename only in the database), no committed secret, no `unsafe`.
  Accepted/documented: admin mode open by default, root container, init panics on embedded
  data, duplicated admin/manage auth.
- **Speed**: bench re-measured on the local corpus (114 replays): **141 ms median < 150**,
  p95 243 ms. Budgets held.
- **Hygiene**: `.DS_Store` removed from the repo; `.gitignore` completed (`.DS_Store`, `.images/`,
  `backups/`); draft docs tidied (`docs/specs/` + `docs/plans/`, refs updated);
  local `__pycache__` purged. No orphan front component, no committed junk.

## Living spec — DONE (2026-08-03, operator request)
**`docs/spec/`**: living documentation of the current state (spec-driven) — 9 files:
architecture/invariants, storm-replay, storm-stats/parity, server/config, **complete API
reference**, data model (migrations 0001→0008), WS+Jarvis events, frontend
(routes/overlays), operations (dev/box/backup). Maintenance rule in `docs/spec/README.md`
and `CLAUDE.md`: **every commit that changes a contract updates the matching section**.
The dated specs (`docs/specs/`) stay the frozen decisions; STATUS stays the log.
Game chat deployed on the box along the way (bundle `index-BmQMoQ9z.js`, verified).
**Completed (same day, "is everything documented?" audit)**: `10-developpement.md` (spec-first
workflow: doc+proof checklist per type of change, definition of "done", AI limits),
repo map in the spec README, **ghcr.io CI documented** (publish.yml — image published
at each push to main, it was undocumented), and **rule no. 0 in CLAUDE.md**: no code diff
carrying a contract without its doc diff in the same commit.

## Game chat — DONE (2026-08-03, operator request)
The match page shows the **complete chat log** (`Match chat` block, `ChatLog` component in
`web/src/pages/MatchDetail.tsx`): chat by default, **pings** and **callouts** (ability/vitals/
behavior) as optional filters; chronological order with signed time (the draft/loading
messages are before the start of the game), avatar + name coloured by team, recipient
(all/allies/obs). **No server/DB/re-process change**: storm-stats already decodes
`replay.message.events` → `match.messages`, projected into `matches.data` and already returned by
`/api/matches/{id}` — already backfilled matches show it as is. Along the way, the
"Pings" column of the BM table counted *all* of the player's messages (chats and callouts included) →
filtered on the Ping type. Verified on a real replay: 3 chats / 52 pings / 6 callouts, total
pings in the BM table = 52.

