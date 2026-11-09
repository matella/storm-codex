# Storm Codex — complete renovation of Stats of the Storm

**Date:** 2026-06-12 · **Status:** design approved in session (complete brainstorm), spec to be reviewed.
**Source research:** `docs/research/2026-06-12-stats-of-the-storm-renovation.md` (+ replay format
reference: `docs/research/hots-replay-data-reference.md`). To be read before any implementation.
**Mockups:** `docs/superpowers/specs/2026-06-12-storm-codex-mockup.html` (all screens).

## Vision

Rewrite [stats-of-the-storm](https://github.com/ebshimizu/stats-of-the-storm) (HotS stats tracker,
Electron/jQuery/NeDB 2018) as a modern service: **open-source Rust parsing core**,
a single server on the box, a front-end reusing the Nexus Codex design system, integrated into our
ecosystem (existing client-rs uploader, HotsPatchNotes reference data, Jarvis events,
stream/Twitch overlay). **Full** functional parity with SotS in V1, plus: real time (end of game
→ page updated < 5 s), post-game stream widget, backfill of several years of replays in minutes.

## Decisions made (brainstorm 2026-06-12)

| # | Decision |
|---|---|
| 1 | **Rust engine** (perf + publishable crates; no Rust HotS parser exists → a real contribution). .NET fallback (Heroes.StormReplayParser) if the spike fails. |
| 2 | **PostgreSQL** (SQL aggregations, concurrent writes, multi-tenant V2-ready). |
| 3 | Front-end: **Nexus Codex design system** reused (tokens, badges, universe rings). |
| 4 | The new server **replaces** Hots-Overlay's **local** Node/Mongo server (decommissioned at the end of V1); OBS overlay re-pointed; the Twitch extension stays served by the existing Azure EBS, fed by a box→Azure push (absorbing the EBS = V2). |
| 5 | **V1 = complete functional parity** with SotS, including leagues/teams/collections. |
| 6 | No pre-game mode (`.battlelobby`) in V1. |
| 7 | Scope **C**: self-hosted V1 (box), open-source code from the start, architecture ready for public V2 hosting (without enabling it). |
| 8 | Upload **B**: multiple personal tokens from V1 (in practice: a single user at first). |
| 9 | Data: **keep everything** via 3 tiers (raw file = source of truth; Postgres = complete projection; dump decoded on demand with an LRU cache — NO full pre-decoding at upload). |
| 10 | Name: **Storm Codex** — crates `storm-replay`, `storm-stats`, binary `storm-codex-server`. |

## Architecture

```
Gaming PC (Windows)                      box 192.168.129.85
┌────────────────────┐  POST /upload   ┌─────────────────────────────────────┐
│ client-rs (existing│ ───────────────►│ storm-codex-server (Rust, axum)     │
│ re-pointed +       │     token       │ ├ parse worker pool                 │
│ backfill mode)     │                 │ │   storm-replay → storm-stats      │
└────────────────────┘                 │ ├ PostgreSQL (projections)          │
                                       │ ├ replay archive (volume/NAS)       │
   browser ◄── WS push ────────────────│ ├ REST + WebSocket                  │
   (React SPA, SotS parity pages)      │ ├ serves the built front (1 binary) │
                                       │ ├ overlay/Twitch endpoints (replace │
   OBS (stream widget) ◄── WS ─────────│ │  the Node server)                 │
                                       │ └ events → Redis ─► Jarvis          │
                                       └─────────────────────────────────────┘
```

## Components

### `storm-replay` crate (open-source, crates.io)
`.StormReplay` decoding: MPQ reader + bit-packed/versioned decoder (machinery proven by
s2protocol-rs on the SC2 side — same Blizzard serialisation engine) + **protocol tables generated
automatically** from [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol)
(maintained, released Feb. 2026) by a `protocol-gen` script (committed, rerunnable on each HotS patch).
- API: `Replay::open(path)` → typed access to `header`, `details`, `initdata`, `attributes`,
  `messages`, `trackerevents`, `gameevents` (lazy decoding per stream).
- Unknown builds: "latest known protocol" fallback + warning (heroprotocol behaviour).
- Tests: corpus of multi-build reference replays (2023→2026) hosted on the NAS/box with a
  `fetch-corpus` script (no Git LFS — public repos stay light; a mini-corpus of
  2-3 free replays is committed for public CI).

### `storm-stats` crate (open-source, crates.io)
Complete port of the [hots-parser](https://github.com/ebshimizu/hots-parser) logic (3,360 lines
of JS analysed — see research folder): decoded replay → typed `MatchStats`.
- Scope: picks/bans + draft order; full score screen (~80 stats/player); awards;
  enriched takedowns (positions, participants, revenges); **objective timeline specific to the
  16 maps** (immortals+durations, dragon, spiders, tributes, altars, skulls, ships, punishers,
  Braxis with wave strength, temples, thrones, mines, seeds, gems, protector); team fights;
  periodic XP/levels + level advantage/uptime; first objective/fort/keep; taunts/BM (bsteps,
  dances, sprays, voicelines + context); messages/pings; votes; globes; camps.
- **Parity validation**: a harness that runs hots-parser (Node) and storm-stats on the same
  corpus and diffs the outputs field by field (documented tolerances). No stats milestone is
  "done" without this green diff.

### `storm-codex-server` (Rust binary, axum, Docker on the box)
- **Upload**: `POST /api/upload` (personal Bearer token) → anti-duplicate fingerprint (same
  BlizzIDs+random MD5 formula as the existing one/HeroesProfile) → file stored in the archive →
  parse job in a pool (never on the HTTP thread) → Postgres transaction → event.
  **Lifecycle of a parse failure**: the file is ALWAYS archived first; a failure
  (corrupt file, isolated worker panic, stats logic error) marks the `uploads` row
  as `failed` with a typed error class + message, visible in Admin/health, and
  retryable via re-process (idempotent, driven by `parser_version`). No failure blocks
  the queue. **Response semantics**: the request waits for the parse result for up to 2 s
  (typically < 0.5 s) and returns the final status (`parsed` / `duplicate` / `parse_failed`); if the
  pool is saturated (backfill), it immediately returns `202 {status:"accepted"}` and the outcome
  is observable via WS/Admin. The client never has anything to redo.
- **Reads**: REST endpoints for all pages (matches, match, player, heroes, trends,
  rankings, teams/leagues, collections, maps, sessions, CSV/JSON exports).
- **Real time**: WebSocket `/ws` — push `match.parsed` (and backfill progress) to all
  clients (site, OBS overlay, Twitch extension).
- **Full dump**: `GET /api/matches/{id}/raw?stream=…` — decodes the archived file on the fly,
  compressed disk cache, **bounded LRU** (config `RAW_CACHE_MAX_BYTES`, default 5 GB); never any
  mass pre-decoding.
- **Overlay/Twitch**: **new, clean** API (no route-by-route compatibility with the
  Node server); we own all the clients, so the OBS overlay page and the Twitch extension's
  config are re-pointed/adapted during milestone 5. Capabilities carried over: last game/session
  summary, recent matches lookup, patch-digest panel (the HotsPatchNotes → server push contract
  is kept as is, same token/format).
  **Twitch in V1**: the extension runs in the viewers' browsers (outside the LAN) and stays served
  by the existing **Azure** overlay instance; the box PUSHES the post-game summaries to it via the same
  authenticated mechanism as the patch-digest (outgoing box→Azure push, nothing incoming). The
  Twitch JWT validation therefore stays in the Azure EBS; absorbing it into storm-codex-server is
  a V2 project (public hosting). Only the **local** Node server is decommissioned in V1.
- **Jarvis**: publishes `hots.match.completed` (payload: result, heroes, KDA, map, duration,
  awards, gaps vs averages) on the box's Redis — the event respects the Jarvis invariants:
  `schema_version`, `correlation_id`/`causation_id`, `occurred_at`/`recorded_at`,
  past-tense `entity.verb` type.
- **Admin**: token management, re-process (all or by filter, idempotent, follows `parser_version`),
  backfill state, health.
- V1 auth: personal upload tokens; read-only UI on LAN/Tailscale (no web accounts).
  V2-ready: `uploader_id` everywhere, stateless server, config by env.

### Migration & decommissioning (explicit decision)
**No data migration.** The match history is rebuilt entirely by
backfill from the raw replays (better than any migration: modern parser, complete
stats). Definitions that cannot be derived from replays (SotS teams/leagues/collections in
NeDB, possible test matches in the overlay's Mongo — empty to date: 0 replays uploaded)
are **not** imported: they are recreated by hand in the new UI (single operator,
negligible volume). The Mongo is inspected one last time before shutdown, then deleted.

### Hero/talent reference data (single source)
The front-end and storm-stats need hero metadata (names, roles, universes, portraits,
talents per tier + icons). Single source: **the HotsPatchNotes API** (:5001, same box), which
already syncs heroes-talents + local images. storm-codex-server replicates them at its
startup (and on admin request) into Postgres dimension tables (`dim_heroes`,
`dim_talents`) — no inter-service call on the hot path, no double sync pipeline.

### Front-end (React + Vite + TS SPA, served by the binary)
- TanStack Query (cache + invalidation via WS), TanStack Table + Virtual (SotS's "big tables"
  without DataTables), uPlot (XP/objective timelines — the fastest), Nexus
  Codex design system (tokens, `.bdg`, kickers, universe rings, hairlines).
- Pages (SotS parity 1:1 + additions): Dashboard/session · Matches (real-time filterable
  list) · Match detail (10-player score, ordered draft, XP/objectives/team fights timeline,
  taunts/BM, chat, votes) · Player (summary, progression, hero pool, hero detail, talent builds,
  duos with/against, awards, big tables) · Heroes/collection · Trends per patch (Nexus Codex link)
  · Player/team rankings · Teams & Leagues (rosters, collections) · Maps · Admin/Import ·
  CSV/JSON exports. Visual detail: see the mockups file.

### client-rs (light changes to the existing one)
- Re-pointing of URL + token to storm-codex-server (existing config).
- **Backfill mode**: full scan of the archive (not only new files), throttled burst
  upload, resume on interruption, progress bar in the tray app.
- Live latency: lower the file stabilisation (~5 s today → ~1 s).

## Data model (3 tiers, "keep everything")

1. **Raw archive**: each `.StormReplay` kept as is (box volume, backupable to the NAS).
   Source of truth — any future stat can be recovered by re-process (< 150 ms/replay).
2. **Postgres (complete projection)** — each match carries `parser_version`:
   `matches` (build, mode, map, duration, date, teams, winner, firsts, fingerprint, Codex patch)
   · `match_players` (10/match: hero, full score screen, awards, level, cosmetics)
   · `talents` (player×tier + timestamp) · `draft` (ordered picks/bans)
   · `timeline_events` (typed JSONB: takedowns, objectives, structures, camps, periodic XP,
   team fights, taunts, messages/pings, votes)
   · `players` (ToonHandle, aliases/tags) · `teams`/`leagues` (rosters) · `collections`
   · `uploads`/`upload_tokens` (traceability per uploader).
   Indexes on the filter axes (map, hero, player, date, patch, collection). Views/materialised views
   for the hot aggregates (hero stats, trends per patch, duos, rankings).
3. **Dump decoded on demand**: `…/raw` endpoint + LRU cache (see server). Possible mass analyses
   (game-event heatmaps): one-off Parquet export + DuckDB, off the hot path.

## Performance budgets (verified, not claimed)

| Step | Budget |
|---|---|
| File detection → upload received (LAN) | < 2 s (stabilisation ~1 s + transfer ~0.3 s) |
| Full parse (decode + stats) | **< 150 ms** /replay (1 core) — go/no-go spike: < 500 ms |
| Postgres write + WS push | < 200 ms |
| **End of game → page updated (without F5)** | **< 5 s** |
| 3-year archive backfill (~3,000 replays, 2–3 GB) | < 5 min end to end (workers = number of cores) |
| Pages: API request p95 | < 100 ms (indexed aggregates/materialised views) |

## Risks & mitigations

- **Protocol drift** (new HotS builds): `protocol-gen` rerunnable from the Blizzard
  repo; latest-protocol fallback; the test corpus includes each new build.
- **Stats parity**: the diff harness vs hots-parser is a **deliverable of milestone 2**, not an
  option. Accepted gaps (fixed hots-parser bugs) are documented.
- **Spike no-go**: .NET fallback (Heroes.StormReplayParser + C# stats port), identical architecture.
- **Box off during the day**: acceptable in V1 (personal use in the evening); public V2 = always-on
  hosting (out of V1 scope, the stateless architecture allows it).
- **Large V2 volumes**: fingerprint + `uploader_id` + Postgres → pooling already planned.

## V1 milestones (each: deliverable + acceptance criterion)

> **Plan framing:** this document is the *programme* design. Each milestone below gets
> **its own implementation plan** (writing-plans), in order — not a single
> plan for the whole.

0. **Go/no-go spike (1–2 d)** — decode 3 real replays (builds ≥ 2024) in Rust; bench 50
   replays Rust vs .NET vs Python. *Accept: complete decode < 500 ms/replay, necessary fields
   present.* Otherwise: documented switch to plan B.
1. **storm-replay** — decoding of the 7 streams, protocol-gen, multi-build corpus tests.
   *Accept: 100 % of the corpus decoded, bench < 150 ms excluding stats.*
2. **storm-stats** — complete hots-parser port + parity harness.
   *Accept: diff vs hots-parser green on the corpus (documented tolerances).*
3. **Server + DB + backfill** — upload/fingerprint/workers/Postgres/WS/raw+LRU/admin/tokens;
   client-rs backfill mode. *Accept: 100 % of the archive archived and attempted; ≥ 99 % parsed
   (failures listed and classified in Admin — pre-2015 alpha builds may legitimately
   fail); end of game → page < 5 s.*
4. **Front-end parity** — all pages, Codex design, real time.
   *Accept: every SotS feature can be found (checklist per page), API p95 < 100 ms.*
5. **Stream + Jarvis + switchover** — OBS widget, `hots.match.completed` → Redis, Jarvis connector,
   post-game box→Azure push for the Twitch extension, **decommissioning of the local Node/Mongo server**
   (the Azure EBS stays, see § Overlay/Twitch). *Accept: game played → OBS widget + Jarvis brief
   + Twitch extension updated; local Node server stopped with no loss of function.*
6. **Publication** — crates.io + READMEs + docs (public GitHub repos).

## Out of V1 scope
Pre-game/battlelobby · web accounts/quotas (V2) · public cloud hosting (V2) · computed MMR
(HeroesProfile remains the reference; optional forward-upload to be decided later) · game-event
heatmaps (one-off Parquet export if needed).
