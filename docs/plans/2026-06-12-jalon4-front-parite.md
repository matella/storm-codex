# Milestone 4 — front-end parity (React SPA, Nexus Codex design)

> **For Claude:** REQUIRED SUB-SKILL: superpowers:executing-plans. Context7 before React/Vite/
> TanStack/uPlot if in doubt about an API.

**Goal:** React + Vite + TS SPA served by `storm-codex-server`, consuming its REST API + WS,
with the **Nexus Codex** design (tokens from the mockup), covering the data-backed SotS pages.
**Accept (spec):** every SotS feature can be found (per page), API p95 < 100 ms (already
met on the server side), real time (new match at the top without F5).

**Design (approved mockup `docs/specs/2026-06-12-storm-codex-mockup.html`):** background #06070b,
surfaces #0e1016/#16181f, hairlines #1a1d2a/#232636, accent #7F77DD, win #5DCAA5,
loss #E24B4A/#F7C1C1, blue team #85B7EB / red #F09595, universe rings
(Warcraft #EF9F27, StarCraft #378ADD, Diablo #E24B4A, Overwatch #D85A30, Nexus #AFA9EC),
Inter + JetBrains Mono, `.bdg` badges, pills, kickers, initial avatars.

**Stack:** Vite + React 18 + TS, TanStack Query (cache + WS invalidation), react-router,
uPlot (timelines). Static build served by the binary (tower-http ServeDir, index.html fallback).

---

### Task 1: Vite scaffold + design tokens + layout + API service
- `web/`: Vite React-TS, `package.json`, `vite.config.ts` (build → `web/dist`, proxy `/api`+`/ws`
  to :8088 in dev). `src/theme.css`: Nexus Codex tokens + components (.bdg/.pill/.av/.kick…).
- `src/api.ts`: typed fetch (`/api/matches`, `/matches/{id}`, `/players/{toon}`, `/heroes`) +
  WS hook (`/ws` → invalidates the matches queries). Layout (header nav + live-dot).
- Build OK, styled blank page. Commit.

### Task 2: Matches (filterable real-time list) + session Dashboard
- Matches page: list from `/api/matches` (mode/map/hero filters), rows in the mockup style
  (time, mode badge, hero avatar with universe ring, KDA, W/L), pagination, **WS → new
  match at the top** (toast "replay received"). Dashboard: last game + session stats.
- Checked against the (real) backfilled DB. Commit.

### Task 3: Match detail
- `/match/{id}` from `/api/matches/{id}`: header (map, mode, date, build), draft
  (ordered bans/picks), 10-player score (2 teams, ~80 stats via the JSONB), timelines
  XP/levels + objectives (uPlot), taunts/BM, chat. Commit.

### Task 4: Player + Heroes + Maps
- `/player/{toon}` (summary, hero pool, winrate); `/heroes` (aggregated games/wins/winrate table,
  sort) + `/hero/{name}`; `/maps` (winrate per map). Sortable TanStack tables. Commit.

### Task 5: server-side static serving + integrated build + STATUS + push
- `storm-codex-server` serves `web/dist` (ServeDir + `index.html` fallback for SPA routing).
  `npm run build` → bundle; the binary serves it on `/`. End-to-end check (server + DB +
  built front-end). Per-page parity checklist. STATUS, commit, **push**.

---

## Pitfalls
- Universe rings: the hero's universe is needed → `dim_heroes` (milestone 4: populate from
  the box's HotsPatchNotes API :5001 at server startup; if unavailable, accent colour fallback).
- The score screen (~80 stats) is in `match_players.data.gameStats` (JSONB) — show a
  readable subset + a collapsible full table.
- SPA routing: the server must return index.html on unknown front-end routes (not 404).
- Real time: the WS pushes `{type:match.parsed, match_id, map}` → invalidate the matches query.
