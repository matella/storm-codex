# Research file — complete renovation of "Stats of the Storm"

**Date:** 2026-06-12 · **Status:** research finished, brainstorm in progress (nothing implemented).
**Goal:** rewrite https://github.com/ebshimizu/stats-of-the-storm (HotS stats tracker,
Electron 2018) on a modern, high-performance stack, integrated into our ecosystem (Hots-Overlay +
Rust uploader, HotsPatchNotes/Nexus Codex, Jarvis), with a "Jarvis" stream widget (result,
KDA, performance summary) and an **instant** replay → web page pipeline.

---

## 1 · Anatomy of Stats of the Storm (the existing product to match)

**Original stack (all from 2018, all dead or frozen):**
Electron 4 · jQuery 3 + Semantic UI 2 + Handlebars (client-side HTML templates) · Chart.js 2 ·
DataTables 1.10 (+7 plugins) · moment.js · NeDB 1.8 / LinvoDB3 + medea/medeadown (stores
abandoned ~2016-2018) · `request` (deprecated) · vis.js 4 · floatThead/tablesort (jQuery plugins).
Process: Electron main + a hidden `background.html` that does the parsing so as not to freeze the UI.

**Data model (4 LinvoDB collections, schemaless):**
- `matches` — 1 doc per game: version, mode, map, date, duration, teams, picks/bans + draft
  order, levels/XP timeline, detailed takedowns (positions, participants), structures
  destroyed, **per-map objective timeline**, messages/pings, end-of-game votes, team stats.
- `heroData` — 1 doc per player×game: hero, talents per tier, ~80 individual stats
  (score screen gameStats + computed stats), awards, taunts/BM (bsteps, dances, sprays…).
- `players` — cumulative profile per ToonHandle (cache of aggregates + aliases/tags).
- `settings` — settings + definition of "collections" (subsets of matches, e.g. leagues).
- Cross-cutting concepts: **collections** (persistent match filters), **teams** (named
  rosters), "My ID" focus, import sets (folder → collection).

**Pages / features (templates + js/):**
- *Matches*: filterable list (map/mode/hero/player/date/patch), drill-down.
- *Match detail* (the richest, 3,084 lines): complete 10-player score, draft with order,
  XP graphs (vis/chart), objective timeline + takedowns, team stats, taunt/BM table, chat,
  export/print by section.
- *Player*: summary, progression, hero pool, per-hero detail, talent builds + pick rates,
  duos ("with" / "against"), award tracker, big tables.
- *Hero collection*: global stats per hero (picks, win rate, talents, comps).
- *Trends*: per patch — picks/bans/win rates, comps.
- *Player ranking / Team ranking / Teams*: internal rankings, leagues (rosters), MVP ratios.
- *Maps*: win rates per map/side, first objective.
- *Settings/Parser*: watch folder, import by date, import → collection, HotsAPI/HotsLogs upload
  (both dead today — the living equivalent is HeroesProfile).
- CSV exports (hero-csv, hero-draft-csv) + shareable "external" databases.

**Aggregations (js/database/summarize-*.js, ~2,000 lines in total):** hero/map/match/player/
talent/team/trend — everything is done **in JS in memory** on the results of NeDB queries.
It is the 2nd performance bottleneck after parsing (the first load of a large history takes
tens of seconds, RAM climbs fast, manual compaction).

**Awards:** complete mapping `EndOfMatchAward*Boolean` → name/icon in `js/game-data/awards.js`
(MVP, Dominator, Avenger, … ~40 entries). To keep as is (data, not code).

---

## 2 · hots-parser (the gem to port — it is THE real asset)

Repo: ebshimizu/hots-parser (npm `hots-parser`, v7.55.x — **already used by our Hots-Overlay**).
3,360 lines of `parser.js` + 446 of `constants.js`. Input: `.StormReplay`; output:
`{match, players}` ready for the DB. Relies on `heroprotocol` (JS port by GaryIrick/nydus).

**What it computes (beyond the score screen) — the differentiating value:**
- Picks/bans with draft order, first pick, teams, MMR-less.
- Enriched takedowns: positions, participants, chain kills, **revenges**.
- **Per-map objectives**: specific logic for EACH battleground (immortals + duration,
  dragon, spiders, altars, tributes, skulls, ghost ships, punishers, Braxis with **computed wave
  strength** (`braxisWaveStrength`), temples, thrones, towers…) → normalised timeline.
- Team fights (internal definition), per-player "team fight *" stats.
- Levels/XP timeline of both teams (PeriodicXPBreakdown), level diff, **time at level adv**.
- Uptime/"hero advantage" (analyzeUptime), first fort/keep/objective.
- Taunts/BM: bsteps (detection by frame sequence), dances, sprays, voice lines + context
  (near an enemy, followed by a kill…).
- Globes picked up, camps captured, structures, chat, votes.
- `getHeader` (quick peek) and `getBattletags` without a full parse.

**Current limits:** `MAX_SUPPORTED_BUILD = 87774` + `overrideVerifiedBuild` option (our
overlay parses the 2026 builds with the override); synchronous (blocks the Node event loop ~1-3 s per
replay); depends on the JS port of heroprotocol (per-build protocols, community maintenance).

**Precious doc:** the SotS repo's `docs/hots-replay-data.md` = **complete reference of 1,090
lines** on where each piece of data sits in the replay (game loops 16/s, offset -610,
fixedData /4096, reserved PlayerIDs 11/12, quests NOT stored in the replay, etc.).
To keep as the bible for any new parser.

---

## 3 · Verdict dependency by dependency

| 2018 dependency | Role | 2026 verdict |
|---|---|---|
| Electron 4 | desktop shell | **Remove** — becomes a web service on the box (our model) |
| heroprotocol JS (GaryIrick) | replay decoding | **Replace** (see §4) |
| hots-parser | replay → stats | **Port** the logic (it is the functional spec), not the code |
| NeDB / LinvoDB3 / medea | embedded DB | **Replace** with a real DB (SQLite/Postgres/DuckDB) |
| jQuery / Semantic UI / Handlebars | UI | **Replace** (modern front-end) |
| DataTables (+7 plugins) | tables | **Replace** (TanStack Table or home-made virtualised tables) |
| Chart.js 2 / vis.js 4 | charts | **Replace** (uPlot/ECharts/observable-plot — uPlot = the fastest) |
| moment.js | dates | **Replace** (native Intl / date-fns / dayjs) |
| heroes-talents (heroespatchnotes) | talent/icon data | **Keep** — we ALREADY sync it into HotsPatchNotes |
| electron-settings/-updater/-window-state, extract-zip, fs-extra, request, node-watch, xregexp | Electron plumbing | **Disappear** with Electron |
| HotsAPI/HotsLogs upload | sharing | Dead → **HeroesProfile** is the living equivalent (optional) |

---

## 4 · Candidate parsing engines (structuring choice no. 1)

| Option | State | For | Against |
|---|---|---|---|
| **Blizzard/heroprotocol** (Python, official) | **Alive** — release 2.55.15.96477, updated Feb. 2026 | exact reference, follows patches, auto-generated per-build protocol | Python (slow, ~s/replay), just a decoder (no stats) |
| **Heroes.StormReplayParser** (HeroesToolChest, .NET) | v2.2.1 June 2024, NuGet, MIT | **high-perf native .NET parser** (Span-based, Benchmarks project), clean API (`StormReplay.Parse`), score results + awards + draft + tracker events + **parsing of the `.battlelobby`** (pre-game!), no per-build protocol to regenerate; **aligns with our .NET 10 stack (HotsPatchNotes)** | last release 2024 → to validate on 2026 replays (to benchmark on our real files); the derived stats (per-map objectives, team fights, BM) remain to be ported from hots-parser |
| **hots-parser JS** (status quo) | used by our overlay today (build override) | zero work, known semantics | the perf/maintenance we are fleeing; synchronous; Node |
| **Rust from scratch** | **no existing HotS crate** (s2protocol-rs = SC2 only) | max perf, fun | MPQ + bit-packed decoder + protocol versioning + ALL the stats logic must be rewritten: by far the longest, for a marginal gain vs .NET on 1 replay/15 min |
| **Hybrid**: existing decoder + **our own stats engine** | — | raw decoding is NOT the differentiator; the hots-parser logic (objectives/team fights/BM) is | — |

**Honest reading of the perf need:** we parse 1 replay every ~15 min live + a one-off backfill
of a few thousand replays. Even 1 s/replay is enough live; the backfill wants
parallelism (8 cores × 1 s ≈ 7,500 replays/15 min). The perceived "instant" comes mostly from the
**push architecture** (watcher → upload as soon as the file is closed → parse → WS → UI), not from the
language. Default candidate: **Heroes.StormReplayParser (.NET 10)** + port of the hots-parser logic
to C# (typed, tested on reference replays) — to confirm by a real benchmark
on ~50 replays from the gaming machine before writing the spec.

---

## 5 · Our assets (to reuse, not to rewrite)

**Hots-Overlay** (box, Node :8086 + Mongo + Twitch extension):
- **client-rs**: Rust uploader (egui + tray + notify + ureq, Windows installer `installer.iss`,
  auto-update `updater.rs`) that detects `Documents/Heroes of the Storm/Accounts/*/*/Replays/
  Multiplayer`, watches and POSTs `/upload` (auth token, anti-duplicate MD5 fingerprint of the BlizzIDs
  + random value — same formula as HeroesProfile). **It is exactly the uploader asked for**;
  it only needs to be pointed (also) at the new backend, or the new backend put
  behind the same route.
- Server: chokidar watcher (`awaitWriteFinish` 5 s — latency to trim), **synchronous** hots-parser
  parse (to replace), 1 Mongo doc/match (10 players + embedded events), rich routes
  (today/sessions/recent/matches/lookup/modes/players, raw replay download), overlay WS,
  patch-digest panel (pushed by HotsPatchNotes), Twitch EBS (JWT).
- `docs/plans/2026-03-14-xp-graph-*.md`: XP graph work already done on the overlay side.

**HotsPatchNotes / Nexus Codex** (box, .NET 10 + SQLite + Blazor WASM :5100):
- Hero/talent/ability reference data (heroes-talents sync) + local portraits + universe
  colours + history of classified patches (BUFF/NERF/…) + complete codex design system.
- The new stats tracker can **consume this reference data** (icons, talents per tier,
  hero metadata) instead of re-embedding heroes-talents, and link each match to the matching Codex
  patch ("this match was on the May 11 patch → see the notes").

**Jarvis** (box, FastAPI + Postgres/pgvector + Redis + voice):
- `hots.py` / `hots_overlay.py` connectors already in place (console cards).
- Event spine: a `match.completed` event → post-game voice brief, stream summary,
  long-term memory ("3rd loss with X on Y today").
- The "Jarvis" stream widget = an overlay panel (same mechanism as the patch-digest panel)
  fed by an endpoint of the new backend + (optional) a sentence generated by Jarvis.

**External ecosystem:**
- **HeroesProfile**: alive, public API + upload (compatible fingerprint), MMR source.
  Optional "forward upload" integration like SotS did to HotsAPI.
- **HeroesDataParser / heroes-talents**: up-to-date game data.
- **HeroesMatchTracker** (.NET, modern equivalent of SotS): **archived July 2023** — the
  "rich local tracker" niche is vacant, our project fills a real gap.

---

## 6 · Proposed architecture directions (for the brainstorm)

**Anticipated target** (to challenge together):
```
Gaming PC                         box (192.168.129.85)
┌─────────────────┐   POST /upload   ┌──────────────────────────────┐
│ client-rs (as   │ ───────────────► │ stats-api (.NET 10)          │
│ is, re-pointed) │  + battlelobby?  │  ├ parse (StormReplayParser) │
└─────────────────┘                  │  ├ stats engine (port of     │
                                     │  │  hots-parser: objectives, │
        WS push "match parsed"       │  │  TF, BM, XP, uptime…)     │
┌─────────────────┐ ◄─────────────── │  ├ SQLite (1 file, WAL)      │
│ stats front-end │                  │  │  or the box's Postgres    │
│ (fast SPA)      │   GET /api/...   │  └ events → Redis (Jarvis)   │
└─────────────────┘                  └──────────────┬───────────────┘
┌─────────────────┐                                 │
│ stream overlay  │ ◄── "Jarvis post-game" widget ──┘
│ + Twitch ext.   │      (result, KDA, summary)
└─────────────────┘
```
- **Target perf budget**: replay closed → visible in the UI < 5 s (watch ~1 s + LAN upload ~0.3 s
  + parse < 1 s + SQL aggregates < 0.2 s + WS push). Backfill: parallel per core.
- **DB**: aggregations = SQL (no more summarize JS in RAM). SQLite (consistent with HotsPatchNotes,
  one file, backup-friendly) vs Postgres (already on the box for Jarvis, richer analytical
  windows). Relational schema: matches / match_players / talents / events(timeline) / teams /
  collections — views = queries, no more caches to invalidate.
- **Front-end**: light SPA (Svelte/SolidJS/React+Vite — to decide), virtualised tables, uPlot
  for the timelines, **Nexus Codex design system reused** (visual consistency with :5100).
- **Live/pre-game**: the `.battlelobby` can be parsed (StormReplayPregame) → the stream widget
  can show the comp BEFORE the end of the match; client-rs can watch the temp file.
- **Jarvis**: `hots.match.completed` event (Redis) → console card, voice brief, stream
  sentence; its connectors read the new API instead of Mongo.
- **Future of the current overlay**: option A — the new backend REPLACES the Node server
  (the overlay/Twitch ext become clients of the new service); option B — coexistence
  (the overlay keeps Mongo, new service alongside). To decide (A = fewer duplicates, more work).

## 7 · Open questions for the brainstorm
1. **Engine**: validate .NET + hots-parser port after a benchmark on real replays? (vs full
   Rust, longer for a gain not needed at "1 player" volume)
2. **DB**: SQLite or the box's Postgres?
3. **Front-end**: which SPA tech, and do we reuse the Nexus Codex design as is?
4. **Overlay**: replacement (A) or coexistence (B) with the current Node server?
5. **v1 scope**: solo-first (your stats) first, leagues/teams (SotS teams/collections)
   in v2? SotS's "league" features are rich but mostly serve organisers.
6. **Multi-player?** a single account (you) or multiple accounts/family?
7. **HeroesProfile forward-upload**: yes/no?
8. **Backfill**: real size of your replay archive on the gaming PC? (sizes the
   first import and the benchmark)
9. **Stream widget**: what granularity? (result+KDA+awards right away, or also generated Jarvis
   sentences, comparisons to your average, win streak…)

## References
- SotS: https://github.com/ebshimizu/stats-of-the-storm (local clone: /tmp/sots)
- hots-parser: https://github.com/ebshimizu/hots-parser (local clone: /tmp/hots-parser)
- Replay data reference: /tmp/sots/docs/hots-replay-data.md (1,090 lines, to archive with the spec)
- official heroprotocol: https://github.com/Blizzard/heroprotocol (2.55.15.96477, Feb. 2026)
- Heroes.StormReplayParser: https://github.com/HeroesToolChest/Heroes.StormReplayParser (v2.2.1)
- HeroesDecode / HeroesDataParser: https://github.com/HeroesToolChest
- HeroesMatchTracker (archived 2023): https://github.com/HeroesToolChest/HeroesMatchTracker
- HeroesProfile API/upload: https://api.heroesprofile.com/ · https://github.com/Heroes-Profile
- heroes-talents: https://github.com/heroespatchnotes/heroes-talents
