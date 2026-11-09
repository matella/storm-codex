# Live companion (pre-game / post-game) — design

> storm-codex spec. Brainstorm approved by the operator on 2026-08-27.
> Target: go/no-go spike, then `writing-plans` and TDD implementation.

## Goal

A **private companion** page (second screen / phone) that opens by itself when a game
is loading: who is in the lobby and what we know about them, your own stats on this hero and this
map, and **the build to follow**, taken from a library you have built up. When the replay
comes up, the same page switches to **debrief**: what you had planned versus what you picked.

Not a stream overlay: a dense page, for you alone.

## Reopening a locked decision

The programme spec (`docs/specs/2026-06-12-storm-codex-design.md`) locks "no pre-game mode
(`.battlelobby`) in V1" — decision no. 6 of the decisions table, repeated in "Out of V1 scope".
**The operator explicitly reopens this decision on 2026-08-27** and retains automatic
detection. Recorded here, in accordance with the repo's rule.

## Decisions made (brainstorm)

1. **Audience**: you, on a second screen. No OBS overlay, so no constraint of readability
   at a distance and no privacy question about other players' stats.
2. **Trigger: automatic from the start**, via `replay.server.battlelobby`. The risk (reverse-engineered
   format, fragile to Blizzard builds) was stated and accepted by the operator. It is
   handled by a prior spike, a quantified parity harness, and explicit degradations.
3. **Pre-game contents**: the four blocks — your suggested build, your 4 teammates, the 5
   opponents, your own stats on this hero / this map.
4. **Builds**: manual entry **plus** seeding from a played game. A build marked default
   per hero = the one proposed; the others listed below. No contextual rules engine in
   V1 (YAGNI — to be reopened if the need shows up in use).
5. **Post-game**: only the **build debrief** (planned vs picked). Session tracking already exists
   in `/queue`, the match page and the 2D viewer already exist — we do not duplicate.
6. **Pipeline**: `client-rs` pushes the **raw bytes**, the server parses. The fragile parser lives
   on the server side, so a format break is fixed by redeploying the box, without ever
   touching or redeploying the Windows binary.

## What already exists and is reused

| Existing | Use here |
|---|---|
| `storm_replay::Replay::battlelobby_raw()` (`crates/storm-replay/src/lib.rs:258`) | provides real lobby blobs for tests and parity |
| `get_battletags` (`crates/storm-stats/src/process.rs:313`) | behaviour reference (battletags regex), but insufficient on its own: it cross-references `details.m_playerList`, absent live |
| `/api/upload-raw` (raw bytes + `Bearer` + `X-Filename`) | exact model for `POST /api/lobby`; `matella-pc` token already issued |
| `/ws` broadcast channel (`match.parsed`, `draft.updated`) | we add `lobby.detected` / `lobby.updated` |
| `draft_live` + `draft/store.rs` | template for the `lobby_live` singleton |
| `dim_heroes.id = name = match_players.hero` (`dim.rs:48`) | single hero key across the whole system |
| `dim_talents.tree_id` (migration 0005) | `talentTreeId` → name/tier/icon |
| `match_players.data.talents` = `{TierNChoice: talentTreeId}` | **same shape as `builds.picks`** → trivial import and diff |
| `app_settings.operator_names` + `pickOperator`/`matchOperator` | identifies which of the 10 players is you |
| `Avatar`, vendored portraits and maps, Nexus Codex tokens | rendering |

## Go/no-go spike (prerequisite, on the Mac)

Not a single line of server or front-end code before that.

1. Extract the lobby blob from the 4 committed replays (`crates/*/tests/data/*.StormReplay`) and
   analyse it byte by byte.
2. **Settle the open question: does the picked hero appear in the battlelobby?** Unknown to
   date; do not assume it. If yes, determine the form of the name (probably an internal name
   like `HeroTychus`) and its normalisation to `dim_heroes.id`.
3. Check whether the blob carries the map and the mode.
4. Check whether the bytes of the temporary file and of the archived stream are identical (decides the
   replay↔lobby linking method, see below).
5. Run the candidate parser over the box's archive (measured: 3,322 replays, 25 builds
   2024→2026) and diff it against the complete parse, which knows the truth.

**Spike acceptance criterion: ≥ 99 % of lobbies with exact names, BattleTags and teams,
on matchmade modes** (Storm League, ARAM, Quick Match). Custom games are outside the
criterion: observers sit in the lobby there and the order does not carry the team.

The spike cannot make the feature fail — it determines whether it costs **zero clicks** (hero
present) or **one tap** (hero absent, manual picker).

## Architecture

```
Gaming PC (client-rs)                    Box (storm-codex-server)            You (2nd screen)
watcher %TEMP%\…\TempWriteReplayP1\
  replay.server.battlelobby
      │ write detected, debounce,
      │ dedup by content hash
      └─ POST /api/lobby (raw bytes) ────► storm_lobby::parse(&[u8])
                                           ├─ Postgres enrichment
                                           ├─ UPSERT lobby_live (singleton)
                                           └─ WS lobby.detected ────────────► /companion

  … game played …
  replay written → existing upload ──────► parse → projection → match.parsed ─► debrief
```

### `crates/storm-lobby` crate (pure, zero I/O)

Does not depend on `storm-replay`: it receives bytes. Consistent with `storm-replay-viewer`
(problem geometry isolated, testable on its own, publishable at milestone 6).

> **Updated on 2026-08-27 after running the spike.** Measured findings
> (`docs/research/2026-08-27-lobby-format.md`): the BattleTags are **present in clear text**, prefixed
> by a length in UTF-8 bytes; the **toon handle, the picked hero, the map and the mode are
> absent**; the team has **no explicit field** and is derived from the order (5+5). The public type
> therefore only carries what is actually decodable — a field that is always `None` would be dead weight.

```rust
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError>;

pub struct Lobby {
    pub players: Vec<LobbyPlayer>,   // order of appearance in the blob
}
pub struct LobbyPlayer {
    pub name: String,                // may contain non-ASCII UTF-8
    pub discriminant: String,        // part after '#'
    pub team: Option<u8>,            // derived from the order, only if exactly 10 players
}
impl LobbyPlayer {
    pub fn battletag(&self) -> String;  // "name#1234" — the identity key
}
```

Typed errors (`thiserror`), no `unwrap()` outside tests.

**Identity resolution.** The blob does not give the toon handle: the server finds it by
matching `name#discriminant` against `match_players.name` + `match_players.data->>'tag'`, that is
**against the archive itself**. A player absent from the archive stays unresolved — with no consequence,
since they have no history to display anyway.

### `lobby.rs` server module

Endpoint, enrichment, singleton. Modelled on `draft/store.rs`. The current lobby overwrites the
previous one; no lobby history (the archived replay remains the source of truth, tier 1).

## Data model — migration `0009_companion.sql`

```sql
-- Build library. `picks` has EXACTLY the shape written by the parser into
-- match_players.data.talents: {"Tier1Choice": "<talentTreeId>", ...}.
CREATE TABLE builds (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hero_id         TEXT NOT NULL,          -- = dim_heroes.id = match_players.hero
    name            TEXT NOT NULL,
    picks           JSONB NOT NULL,
    notes           TEXT,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    source_match_id BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX builds_hero_idx ON builds(hero_id);
-- Invariant held by the database: at most one default build per hero.
CREATE UNIQUE INDEX builds_one_default_per_hero ON builds(hero_id) WHERE is_default;

-- Current lobby. Singleton, modelled on draft_live.
CREATE TABLE lobby_live (
    id         INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

(The applied migration file keeps its original French comments; it is never edited.)

### Enriched state (`lobby_live.state`, carries a `schema_version`)

The server does not return the raw lobby but the lobby joined with the archive:

- **per player**: `known`; games with you / against you and winrates; last encounter; their 3
  most-played heroes; their stats on the current hero.
- **for you** (via `operator_names`): winrate on this hero, on this map, on the
  hero×map pair, your last games on this hero.
- **build**: the hero's default + alternatives, talents resolved via `dim_talents.tree_id`.
- `match_id`: filled in at linking → switch to debrief.

SQL aggregates over `match_players` ⋈ `matches` (~20,000 rows, existing indexes on `toon_handle`,
`hero`, `match_id`). Budget: see criterion 3 below, amended after measurement.

### Replay ↔ lobby linking

**Method chosen: the set of the 10 BattleTags**, plus a time window of a few
hours. The complete parse rebuilds the same BattleTags from the blob embedded in the replay
(`get_battletags`, `process.rs:313`), so both sides carry the same key.

More exact alternative — hash the blob's bytes and compare them with the one extracted from the replay — retained
**only if point 4 of the spike confirms** bit-for-bit identity. Otherwise the handles are kept.

## API

| Route | Role |
|---|---|
| `POST /api/lobby` | raw bytes, upload `Bearer` — parses, enriches, upserts, broadcasts. Idempotent. |
| `GET /api/lobby` | current enriched state; `204` if none |
| `POST /api/lobby/hero` | `{hero}` — 1-tap fallback if the blob does not carry the hero |
| `DELETE /api/lobby` | closes the companion |
| `GET/POST/PUT/DELETE /api/builds` | library CRUD (same admin guard as teams/collections) |
| `POST /api/builds/from-match` | `{match_id, toon_handle, name}` → seeding |

WS: `lobby.detected` on receipt of a new lobby; `lobby.updated` on any later mutation
of the same lobby (hero entered by hand, linking to the replay, closing). Existing channel,
consumed like `match.parsed` — TanStack Query invalidation, no state push.

## Front-end

### `/companion` — three states on one route, driven by the WS

- **Idle**: "waiting for a lobby", last debrief, access to the library. Never empty.
- **Pre-game**: map · mode · your hero banner. **The build dominates the page** (7 tiers, large) —
  it is what gets consumed during the ~45 s of loading. The 10 players in two team
  columns (known/unknown badge, W-L with or against you, current hero). Your hero / map /
  hero×map stats below the build.
- **Debrief**: tier-by-tier diff, planned vs picked, marked ✓/✗, plus the result. Links to the
  match page and the 2D viewer — which exist, and which we do not re-implement.

### `/builds` — library

List per hero, 7-tier editor fed by `dim_talents`, default marking. The
seeding button "save as build" lives **in the match page**, where you are when you
notice that a game went well.

## Degradations (required behaviours)

| What breaks | Behaviour |
|---|---|
| Blizzard changes the format | `POST /api/lobby` answers 200 with a classified `parse_failed`, build logged in clear. The page shows "unreadable lobby (build X)" **and** the hero picker → the suggested build is still shown. Loss of the 9 players, not of the main function. |
| Hero absent from the blob (**confirmed** by the spike) | picker, one tap |
| Player never met | "never met" written as is — never a 50 % fabricated from zero games |
| Map absent from the blob (**confirmed** by the spike) | picker, one tap; failing that, map stats at debrief |
| No build for this hero | "no build" + shortcut "import from your best game on this hero" (data already in the database) |
| Box unreachable | previous state shown and marked stale — no infinite spinner |

## Tests

- **`storm-lobby`**: no golden — the oracle (`tests/oracle.rs`) diffs against the complete parse on
  the workspace's 4 committed replays; **never panics** on truncated, empty or
  random input (`Err`, not `panic`, `tests/robustness.rs`).
- **`crates/storm-lobby/examples/parity.rs`**: standalone parser over the box's archive, diffed against the
  complete parse, broken down by game mode. **Criterion: ≥ 99 % (names, BattleTags, teams) on
  matchmade modes** — see the measured verdict in `docs/research/2026-08-27-lobby-parity.md`.
- **Server**: default build invariant (violation refused by the database); upsert
  idempotence; enrichment aggregates on a seeded database; linking by set of handles.
- **Front-end**: vitest on the pure parts — build diff, known/unknown classification. Same
  approach as `replay2d.ts`, no heavy DOM test.
- **E2E on the Mac**: local Postgres + server + committed replay → extract its blob → `POST
  /api/lobby` → page filled → upload the replay → debrief. Reproducible without the gaming PC or a game
  in progress.

## Acceptance criteria

1. Spike: lobby parity **≥ 99 % on matchmade modes** (custom games outside the criterion);
   hero presence settled by yes or no. → **achieved: 100 %** on 2,710 games.
2. Lobby detection → page filled **< 2 s**.
3. `/api/lobby`: **`GET` p95 < 100 ms** (it serves the state from memory — measured at ~1 ms);
   **`POST` p95 < 500 ms**.

   **Amendment of 2026-08-28, operator decision, backed by measurement.** The initial criterion was
   "`/api/lobby` p95 < 100 ms", inherited from the programme spec's general API contract. Measured by
   `crates/storm-codex-server/backfill_bench.py`: the `POST` holds **18 ms warm**, but
   **165 ms cold** — reproducible over 3 restarts (164.6 / 167.6 / 169.3 ms). Yet the warm
   regime does not exist in use: there is **one `POST` per game**, server idle in between, so
   the cold path **is** the nominal path.

   The 100 ms contract is therefore not met, and claiming otherwise would be false. It is amended
   rather than worked around, because the real need does not justify it: this `POST` runs during
   a **45-second** loading screen, where 165 ms is imperceptible. The 500 ms threshold
   leaves a 3× margin on the measurement and would remain invisible in use.

   What is **not** decided: the cause of the ~150 ms cold cost (pool connection establishment,
   SQL planning, other) has not been investigated. If this threshold were to break, it is the first
   place to look — a `min_connections` on the sqlx pool is the most likely hypothesis.
4. Scripted, replayable E2E on the Mac.
5. Each degradation in the table above exercised by a test or a recorded manual check.

## Deliverable outside this repo

The watcher lives in **`client-rs` (Hots-Overlay repo)**: watch
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`, handle the file
locked during writing, **deduplicate by content hash** (the file survives between two
games — without that, the previous game's lobby would reopen at each launch).

It is the only piece that requires the gaming PC, and the last in the sequence: everything else is
developed and proven from the Mac.

## Out of V1 scope

- Companion OBS overlay (decision: private page only).
- Contextual rules engine to choose the build (map, matchup, composition).
- External stats source for unknown players (HeroesProfile API) — unknown players stay
  displayed as unknown.
- Debrief extended to teammates' performance.
- Lobby history.
