# In-game live feed (`TempWriteReplayP1`) — design

> storm-codex spec. Written on 2026-10-24; **decisions Q1–Q5 settled by the operator on
> 2026-10-24** (§ Decisions made). Origin: the operator's question "can we read the replay
> live and see the abilities used?", then an observation session on a real game.
> The measurements are in `docs/research/2026-10-24-tempwritereplay-live.md`; the spec only repeats
> them where they decide something.
> Implementation plan: `docs/plans/2026-10-24-feed-live.md`.

## Goal

During a game, see **live, with a measured delay of about 10–14 s for the tracker and
up to ~85 s for abilities** (game events: 37–72 s behind the tracker), what is happening: team
levels, talents picked, deaths with those credited, camps and objectives, and **the abilities
you use**. Today all of this only exists after the game, when the replay comes up.

> **Re-measured with 10 humans on 2026-10-25** (ARAM, `docs/research/2026-10-25-feed-live-spike-aram.md`):
> the order is reversed. Abilities **~5 s** median delay (a block of game events every ~2 s),
> tracker **~28 s** median, up to ~50 s. The figures above apply to the Sandbox with one human.

## What the measurement established (2026-10-24)

One game observed: **Sandbox against the AI**, Volskaya Foundry, build **98348**.

1. **The game writes the replay as the game goes.** In
   `%TEMP%\Heroes of the Storm\TempWriteReplayP1\`, `replay.tracker.events` and
   `replay.game.events` grow during the game, in **full 4 KB blocks**.
2. **Latency = time to fill a block**, not a fixed delay:
   - tracker, one block every ~10–14 s;
   - game events, one block every ~15–40 s, and nothing at all during the first ~2 minutes
     (the first block was not full yet). Measured 37 to 72 s behind the tracker.
3. **It decodes with our tables**: heroes, levels, talents, deaths, camps, objective, globes, periodic
   XP, positions (every 240 loops), and ability orders (`SCmdEvent.m_abil`).
4. **The final file is the replay.** At the end of the game, the temporary files decode
   identically to the `.StormReplay`'s streams: 4,519 / 4,519 tracker events, 6,022 / 6,022 game
   events, `details` and `initData` identical, battlelobby byte-identical. A live parse carried
   through to the end yields the final parse.
5. **End of game detectable**: final flush in one go, with **appearance of `replay.details` and
   `replay.initData`** (absent during the game). The folder **is not deleted** at the end;
   it did not exist before the game, but the moment it disappears has not been observed.
6. **The build is not readable mid-game**: no MPQ header in the folder. The game
   ran on 98348, more recent than our tables (96477); the "latest known protocol" fallback
   was enough this time.

**What is NOT established** (a single game, against the AI): the presence mid-game of the
orders of the 9 other humans in a matchmade game, the latencies with 10 humans. (The 13-minute
stop observed was a pause by the operator: while paused, nothing is written.) → spike
below.

## Locked decision affected?

No. The programme spec locks nothing about real time *in game* (its "real time"
means end of game → page < 5 s). Decision no. 6 ("no pre-game mode") was already reopened
by the operator on 2026-08-27 for the companion; this feed is its extension *during* the game.

## Fair play — settled by the operator on 2026-10-24

The tracker **is not filtered by the fog of war**: `SUnitPositionsEvent` gives the position
of both teams' units every 15 s of game time; in a matchmade game, the game events probably carry
the opponents' orders (targets included). Showing live information that the client does not show
the player is an advantage, whether the source is a file or memory.
No source was consulted on Blizzard's position regarding reading these files
during a game: **the ToS risk is unknown, not zero.** The operator was informed of this risk
before deciding.

**Decision: three visibility levels, chosen by the operator on the page, "me" by default.**

| Level (`scope`) | What is shown live |
|---|---|
| `self` — **default** | **the scoreboard** (what the Tab screen shows everyone: heroes, team level, takedowns and deaths, talents of the 10 players, camps captured, objectives) + **your own abilities** |
| `allies` — option | `self` + **the abilities of your 4 allies** |
| `all` — option, assumed by the operator | `allies` + **the abilities of the 5 opponents** |

- The choice lives in the page (remembered per browser); a server with no parameter serves `self`.
- The filter is applied **server-side**, on read (`GET /api/live?scope=…`): data outside the
  level is never serialised. The WS carries **no state** (a mere invalidation signal),
  so nothing leaks through it.
- **Positions: outside V1 at all levels**, including `all` (see Out of scope) — they are not even
  kept by the reducer.

## Decisions made (operator, 2026-10-24)

| # | Question | Decision |
|---|---|---|
| Q1 | Fair play | three levels `self` (default) / `allies` / `all`, above |
| Q2 | Where to display | **both**: a **`/live`** page in the navigation, and the "in game" state of `/companion`. The view is **a single component** (`LiveGame`) mounted on `/live` right away; `/companion` (plan 3, not written yet) will embed it |
| Q3 | Name abilities in V1 | **no**: counter and timeline per ability identifier, naming = step 2 |
| Q4 | Where to parse | **server**; `client-rs` only pushes bytes |
| Q5 | Live Jarvis events | outside V1 |
| — | Order of work | **server + page first**, proven by a simulator that replays a replay in 4 KB chunks; the `client-rs` watcher next; the spike on a matchmade game is **no longer blocking for coding**, it is for declaring the feature finished (criterion 3) |

## What already exists and is reused

| Existing | Use here |
|---|---|
| `storm-replay`: `decode_tracker_events` / `decode_game_events` (`src/protocol.rs`) | the decoding itself — today private and not resumable (see contract below) |
| `storm-replay-viewer::extract`: cast detection (non-zero `m_abil`, 0.3 s dedup) | same rule for counting abilities |
| `storm-stats::process`: reading `PlayerSpawned`, `LevelUp`, `TalentChosen`, `PlayerDeath`… | semantic reference for the live reducer |
| `dim_talents.tree_id` = `TalentChosen`'s `PurchaseName` (checked: `MonkTranscendence` → Kharazim, tier 1, *Transcendence*) | live talent name, tier and icon |
| `storm-lobby::parse` + `lobby_live` (`me` = operator's index) | map and operator identification |
| `/api/upload-raw` (raw bytes + `Bearer`) | transport model |
| `/ws` broadcast channel | new `live.updated` event |
| `client-rs` watcher on `TempWriteReplayP1` (companion plan 4; **written on 2026-10-25**: `client-rs/src/live.rs` in the Hots-Overlay repo, lobby + two streams + end, proven on a real game) | same watcher, widened to both event files |

**What is missing**: the tracker names heroes by their unit (`HeroMonk`, `HeroFaerieDragon`),
not by the canonical name (`Kharazim`, `Brightwing`) used everywhere else (`dim_heroes.id`). No
mapping table exists in the repo. It is **derived from the archive** (for each replay:
player N's `PlayerSpawned.Hero` ↔ player N's hero in `details`), committed as JSON and embedded
in the server; a hero absent from the table is displayed with its unit name without `Hero`.

## Architecture

```
Gaming PC (client-rs, next step)             Box (storm-codex-server)                2nd screen
watcher TempWriteReplayP1\
  battlelobby (creation)  ── POST /api/lobby ──► (existing) lobby_live ─ lobby.detected
  tracker.events ┐ follows the size, sends
  game.events    ┘ the appended bytes
        └── POST /api/live/chunk ─────────────► live session (memory)
            X-Session, X-Stream, X-Offset         resumable decoder per stream
            + bytes                                reducer → LiveState
                                                   └─ WS live.updated (signal) ─► /live, /companion
                                                      GET /api/live?scope=&me= (filtered)
  details + initData appear (end) ─ POST /api/live/end ─► session closed; the replay comes up
                                                through the existing upload → match.parsed
```

Until the watcher exists, the **simulator** (the server's `examples/live_sim.rs`) plays its role: it
reads a `.StormReplay`, posts its battlelobby to `/api/lobby`, then cuts `tracker.events` and
`game.events` into 4 KB blocks that it posts in game-loop order, at the game's pace multiplied
by `--speed`.

### Transport: appended chunks, indexed by offset

`POST /api/live/chunk`, headers `X-Session: <id>`, `X-Stream: tracker|game`, `X-Offset: <n>`,
`X-Build: <n>` (optional), body = raw bytes; upload `Bearer`.
- **Session**: `X-Session` = **hexadecimal SHA-256 of the battlelobby** (`client-rs` like the
  simulator). This is what links the session to the current lobby (`lobby_live.content_hash`): the
  map and the operator's slot are only read if the two match. A new identifier
  **replaces** the current session (one game at a time).
- **Idempotent**: a chunk already received (offset + length ≤ known) answers `200 duplicate`; an
  overlapping chunk is only applied for its new part; a gap (offset > known) answers
  `409` with `expected_offset`, and the client resends from there.
- Memory guards: 64 MB at most per stream and per session (`413` beyond; a real game
  produces a few hundred KB), retained feed bounded to 2,000 items, ability timeline
  limited to the served window (3 min).

### Decoding contract added to `storm-replay`

The measurement harness reached the private modules via `#[path]` and searched the decodable prefix
by brute force: **throwaway**. Public API:

```rust
pub enum StreamKind { Tracker, Game }

/// Decoder of a bare stream (without MPQ), fed in chunks.
pub struct StreamDecoder { /* protocol, kind, undecoded tail, current gameloop */ }

impl StreamDecoder {
    /// `build = None` → latest embedded protocol.
    pub fn new(kind: StreamKind, build: Option<u32>) -> Result<StreamDecoder>;
    /// `Some((requested, used))` if the build has no exact table.
    pub fn protocol_fallback(&self) -> Option<(u32, u32)>;
    /// Appends bytes, returns the COMPLETE events; keeps the cut tail for the
    /// next call. Never panics.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Value>>;
    pub fn consumed(&self) -> u64;      // bytes decoded into complete events
    pub fn pending(&self) -> usize;     // bytes waiting (cut event)
    pub fn gameloop(&self) -> i64;      // gameloop of the last complete event
}

impl Replay {
    pub fn tracker_events_raw(&self) -> Result<Vec<u8>>;  // bare stream (simulator, tests)
    pub fn game_events_raw(&self) -> Result<Vec<u8>>;
}
```

- End of buffer in the middle of an event (`Error::Truncated`) → the event waits for the next chunk.
  Any other error = real corruption or wrong protocol → `Err`, the state no longer advances.
- Public API change → **`docs/spec/02-storm-replay.md` updated in the same commit**, and a
  test on the mini-corpus: `feed` in chunks (4 KB, 1 byte, pseudo-random sizes) produces
  exactly the whole replay's `tracker_events()` / `game_events()`.

### The build, mid-game

Unknown as long as `details`/`initData` do not exist (end of game). Resolution order:
1. `X-Build` if provided. **Source found in the 2026-10-25 spike**: the path of the running
   process, `…\Versions\BaseNNNNN\HeroesOfTheStorm_x64.exe` (the battlelobby does not contain the build;
   the highest `BaseNNNNN` in the folder is not reliable, old ones remain). `client-rs` does not send it
   yet;
2. failing that, **latest embedded protocol** — what worked on 2026-10-24 — with the fallback exposed
   in the state (`protocol_fallback`).

If the protocol is wrong (a patch that changes the streams), decoding fails: the session goes to
`unreadable`, never an invented state.

### `LiveState` reducer (pure, zero I/O)

Pure server module (`src/live/state.rs`), testable without a database or network: it consumes the
decoded events and maintains the state. Semantics aligned with `storm-stats` (same fields).

- **Time base: game loops** (16 per second of game time). The displayed clock is
  `(loop − GatesOpen loop) / 16`; it is aligned with the in-game clock in the spike.
- **Players**: `SPlayerSetupEvent` (userId → playerId, slotId); hero via `PlayerSpawned`;
  team: players 1–5 = team 0, 6–10 = team 1 (tracker convention).
- **Scoreboard**: level per team (`LevelUp`), talents per player (`TalentChosen`), deaths
  (`PlayerDeath`: `PlayerID` + `KillingPlayer`×n → one death for the victim, one takedown for
  each credited player), camps (`JungleCampCapture`), objectives (other map `SStatGameEvent`s, kept as
  is in the feed).
- **Abilities**: per player, per `(m_abilLink, m_abilCmdIndex)` pair: counter + gameloops
  (0.3 s dedup per pair, the viewer's rule).
- **Operator (`me`)** — a wrong "me" would show another player's abilities under `self`, an
  undetermined "me" shows none; hence, in order: explicit `?me=` (the page's
  selector); operator's slot in **this session's** lobby (hash = id) → playerId via
  `SPlayerSetupEvent.m_slotId` (lobby order = slot, checked on a 10-player game); if that lobby
  has several humans, nothing; otherwise the only human player who emitted a game event, once
  the game events cover at least one minute (game against the AI). Test: at no block of a
  10-human game does this last rule trigger.
- `last_loop` per stream and timestamp of the last chunk received, exposed as is: the page
  shows the delay rather than suggesting real time.

### Abilities

An ability order is an `SCmdEvent` with `m_abil` = `(m_abilLink, m_abilCmdIndex)`. Two limits
shown on the page:
- it is a **key press**, not a successful cast (range, cooldown, cancellation unknown);
- `m_abilLink` is an index into the **build's** ability catalogue: its name is not in the
  replay. On 2026-10-24, Kharazim produced 13 distinct pairs in 10 minutes.

V1 (**Q3**): counter and timeline per pair, without a name. Naming is a step 2 with two avenues
to evaluate: extraction from the game data (CASC, heavy, to redo at each patch) or a table
calibrated per hero (press each key in try mode, once per patch).

## API and events

| Route / event | Role |
|---|---|
| `POST /api/live/chunk` 🔒 | appended bytes of a stream (`X-Session`, `X-Stream`, `X-Offset`, `X-Build`?), upload `Bearer`. `200` `{status: applied\|duplicate, consumed, last_loop}`; `409` `{expected_offset}`; `400` invalid header |
| `POST /api/live/end` 🔒 | `{session}` — end of game signalled by the client; the session goes to `ended` |
| `GET /api/live?scope=self\|allies\|all&me=<playerId>` | filtered view of the current session; `204` if none |
| `DELETE /api/live` 🔒 | closes the session (`ADMIN_TOKEN` guard of the management routes) |
| WS `live.updated` | `{type, session}` after each chunk that changes the state or status change; **no state in the event** — the front-end invalidates `["live"]` |

No table: the live state is **in memory** and disposable. The archived replay remains the source of
truth (tier 1); the Postgres projection comes from the final parse, unchanged. Consistent with the rule
"never any mass pre-decoding".

## Front-end

**`LiveGame`** component (complete view of a game in progress), mounted on the **`/live`** page
("Live" navigation entry) and intended for the "in game" state of `/companion` when that page
exists. Driven by `live.updated`:
- **banner**: map, game clock, data delay (tracker / abilities / last chunk
  received), status (`live`, `ended`, `unreadable`, feed interrupted);
- **visibility**: selector `Me + scoreboard` / `+ allies` / `All` (warning shown
  on `All`), and an "I am" selector (10 heroes) when the operator is not deduced;
- **scoreboard**: two teams, hero (portrait), team level, takedowns / deaths, talents per tier;
- **abilities**: per visible player, one row per identifier (counter + timeline of the last 3
  minutes), with the mention "key presses, not named";
- **feed**: deaths, talents, camps, objectives, most recent first;
- **idle**: "waiting for a game" — never an empty page.

## Degradations (required behaviours)

| What breaks | Behaviour |
|---|---|
| Files of an already finished game found when `client-rs` starts (`replay.details` present) | game over → **no live session opened**; the replay comes up through the existing upload (same trap as the companion's leftover battlelobby) |
| Wrong protocol (patch) | session `unreadable` ("unreadable feed, assumed build X"); no state changes any more; the rest of the app works |
| Game events still empty (start of game) | abilities section "waiting for the first data", not a zero |
| No chunk for > 60 s while the session is not `ended` | "feed interrupted (pause or disconnection?)" — a 13-min pause observed on 2026-10-24 produces exactly this silence; the page cannot tell pause from disconnection |
| Hero absent from the unit table | unit name without `Hero` (e.g. `Monk`), never an empty cell |
| Box unreachable | the client keeps its offset and catches up on return (`409` → resend); the page shows the previous state marked stale |
| `client-rs` absent or stopped | nothing changes for post-game: the replay comes up as today |

## Spike (on the gaming PC — required before declaring the feature finished, no longer before coding)

> **Done on 2026-10-25** on a 10-human **ARAM** game (not QM/SL), with the real `client-rs`
> watcher: `docs/research/2026-10-25-feed-live-spike-aram.md`. Points **1** (yes, all 10), **2**
> (abilities ~5 s, tracker ~28 s median) and **3** (process path) settled; **4** (clock) and
> **5** (lifecycle, partial) remain open; **6** not addressed.

1. **A matchmade game** (QM or Storm League): are the orders of the 10 players in the temporary
   `game.events` **mid-game**? (Decides whether `allies`/`all` actually show
   anything.)
2. Re-measure both latencies and the block cadence with 10 humans.
3. **Source of the build mid-game**: look for the build in the battlelobby; failing that, check
   reading `Versions\BaseNNNNN` in the install folder.
4. Align the clock (loops − GatesOpen) with the clock shown in game.
5. Folder lifecycle: when does it disappear (client closed, next game)? On a
   reconnection mid-game, is it recreated, do the files start from zero?
6. Abilities, step 2: assess the feasibility of the two naming avenues (without implementing them).

## Tests

- **`storm-replay`**: `StreamDecoder::feed` in chunks (4 KB, 1 byte, pseudo-random sizes)
  = decoding of the whole replay, on the mini-corpus; never panics on truncated or
  random input.
- **Reducer**: replay a committed replay chunk by chunk; the final state matches the
  `storm-stats` parse of the same replay (heroes, team levels, talents, deaths).
- **Visibility filter**: tests that fail if an out-of-level ability (ally under `self`,
  opponent under `allies`) appears in the serialised view.
- **Server**: idempotence of `/api/live/chunk` (duplicate ignored, overlap, gap → `409`),
  session replacement.
- **E2E replayable without a game**: `live_sim` on a replay → the `/live` page goes through the
  expected states (checked in the browser).

## Acceptance criteria

1. Spike: points 1 to 4 settled, figures recorded in `docs/research/`. **As of 2026-10-25: 1, 2
   and 3 settled; 4 (clock alignment) remains to do** → criterion not yet met.
2. Chunk received → `live.updated` emitted **< 200 ms** (the dominant delay is the game's, ~10 s
   for the tracker, up to ~85 s for abilities, outside our control). **Measured on
   2026-10-24** (`POST /api/live/chunk` round trip, which answers after the broadcast; local release
   server): p95 1.7 ms over 76 blocks, 1.6 ms over 259 blocks → **met**.
3. At the end of the game, the final live state matches the projection of the final replay (heroes, levels,
   talents, deaths), checked on a real game. **Met on 2026-10-25** (ARAM, 10 players: heroes,
   teams, deaths, takedowns, 7 talents and team levels identical; byte-identical streams).
4. No out-of-level ability in the served view (automated test).

## Out of V1 scope

- OBS overlay of the live feed.
- **Live positions and minimap, at all visibility levels** (possible next step,
  behind the `all` level).
- Live Jarvis events.
- Ability naming (step 2).
- Reading the game's memory or any source other than the files of the temporary folder.
