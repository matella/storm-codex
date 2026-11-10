# `TempWriteReplayP1` files during a game — measurements of 2026-10-24

> Raw findings from an observation session on the gaming PC, for the spec
> `docs/specs/2026-10-24-feed-live-en-partie-design.md`. **A single game observed**: a
> **"Sandbox" game against the AI** (final replay title `Sandbox (Volskaya Foundry)`; the 9 other
> players are `Player N`, toon `0-Hero-0-0`), build **98348** (2.57.0). Nothing here is generalised
> to a matchmade game before re-measurement.

## Observation protocol

- PowerShell **watcher**: lists `%TEMP%\Heroes of the Storm\TempWriteReplayP1\` every 2 s and
  logs each new, modified or vanished file. The size is read **by opening the file**
  (shared read `ReadWrite, Delete`), not from the directory listing, which can be stale
  for a file kept open by the game.
- **Copies**: snapshots of the files (same shared open) at 16:21, 16:23:02, 16:23:39 and
  16:26:48; mirror refreshed every 3 s afterwards.
- **Decoding**: throwaway harness that reuses `storm-replay`'s private modules
  (`protocol`, `versioned`, `bitpacked`…) via `#[path]`, plus `storm-lobby::parse` for the
  battlelobby. The longest decodable prefix is found by brute force (the last event
  is often cut). The harness is **not** in the repo — it is a proof of feasibility.
- **Protocol**: tables of build `96477` (latest embedded build). ⚠ The temporary folder has
  **no MPQ header**, so no `m_baseBuild`: the build was **assumed**, not read. The final
  replay revealed afterwards that the game ran on **98348** — more recent than our tables. The
  decoding worked anyway (same "latest known protocol" fallback as the server parse),
  which means the protocol did not change between the two builds for these streams, not that
  it will always be the case.

## What the folder contains

Created at **16:04:35** (all files at the same time); observed from 16:05:28.

| File | During the game |
|---|---|
| `replay.server.battlelobby` | 279,740 bytes, complete from creation, no longer changes |
| `replay.tracker.events` | **grows during the game**, in 4,096-byte blocks |
| `replay.game.events` | **grows during the game**, in 4,096-byte blocks, late start (see below) |
| `replay.attributes.events`, `replay.load.info`, `replay.message.events`, `replay.resumable.events`, `replay.smartcam.events`, `replay.sync.events`, `replay.sync.history` | **0 bytes** throughout the observation |

The battlelobby, decoded by `storm-lobby`: map `Volskaya Foundry`, **a single** player
(`matella#21409`, team `None`).

## Write cadence

Writes happen in **full 4 KB blocks**: the latency is the time to fill a block,
not a fixed delay.

**`replay.tracker.events`**
- 20,480 bytes from 16:05:47 (burst of `SUnitBornEvent` at game start: structures, minions).
- Then a block every **~10–14 s** (16:05:53 → 16:07:46, then 16:21:05 → end of observation).
- Throughput measured between two snapshots: 649 game loops for 37 s of wall clock (16:23:02 → 16:23:39),
  i.e. the game advancing in real time.

**`replay.game.events`**
- **0 bytes until 16:21:41**, even though loops 0 → ~2,000 had already been played before the pause
  (see below). The first block is only written once 4 KB have accumulated.
- Then a block every **~15–40 s** (depends on activity: camera, orders).
- Delay behind the tracker, at the same instant: **~37 s** (snapshot 16:23:39: tracker loop 5,584,
  game loop 4,998) and **~72 s** (16:26:48: tracker 8,643, game 7,482).

**13-minute stop**: no write or loop progress between **16:07:46 and 16:21:05**, on
both files, without the folder disappearing. **Cause: the operator had paused the
game** (confirmed by them on 2026-10-24). While paused, the game writes nothing and the loops do not
advance; writing resumes with the game, with no gap or catch-up.

**Block edge**: the last event of a file is generally cut; 1 to 14 bytes remained
undecodable at the end of the buffer depending on the snapshot.

## What decodes mid-game

Snapshot of 16:26:48 (loop 8,643 on the tracker side, 7,482 on the game side; gates at loop 610).

**Tracker** — 4,103 events: `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitOwnerChangeEvent`,
`SUnitPositionsEvent`, `SUnitRevivedEvent`, `SUnitTypeChangeEvent`, `SUpgradeEvent`,
`SPlayerSetupEvent` (×10), `SStatGameEvent`.
- **Heroes** of the 10 players via `PlayerSpawned` (e.g. P1 `HeroMonk`, P8 `HeroChromie`).
- **Levels** (`LevelUp`, per player), **talents** (`TalentChosen` with `PurchaseName`, e.g.
  `ChromieSandBlastOnceAgainTheFirstTime`), **deaths** with all those credited (`PlayerDeath`:
  `PlayerID` + `KillingPlayer`×n), **camps** (`JungleCampCapture`, `CampType`), **map
  objective** (`VolskayaCapturePointSpawned/Complete`), **globes** (`RegenGlobePickedUp`), **periodic
  XP** (`PeriodicXPBreakdown`).
- **Positions**: `SUnitPositionsEvent` every **240 loops** (loops 960, 1,200, 1,440…), for
  the tracked units **of both teams** — the tracker is not filtered by the fog of war.

**Game events** — 4,927 events: `SCameraUpdateEvent`, `SCmdEvent`,
`SCmdUpdateTargetPointEvent`, `SCommandManagerStateEvent`, `STriggerKeyPressedEvent`,
`SHeroTalentTreeSelectedEvent`…
- **A single `_userid` (0)** emits orders (≈ 2 events under `_userid` 16, non-player).
  `SPlayerSetupEvent`: userId 0 → player 1 (Kharazim).
- **Abilities**: `SCmdEvent` with non-zero `m_abil` — at 7:47 of loops, P1 has 123 ability
  orders spread over 13 `(m_abilLink, m_abilCmdIndex)` pairs (e.g. `26/0` ×43, `491/0` ×16).
  These are **key presses**, not confirmed casts. **Which `m_abilLink` matches
  which key is not known**: the identifier points into the build's ability catalogue.

## End of game

- Final replay written at **16:27:41**:
  `Replays/Multiplayer/2026-10-24 16.27.41 Sandbox (Volskaya Foundry).StormReplay` (141,625
  bytes, 9,736 loops, win).
- At **16:27:43**, final flush in one go in the temporary folder: last partial block of
  `tracker.events` (241,687 bytes, no longer a multiple of 4 KB) and of `game.events` (64,264), and
  **appearance** of `replay.details` (1,408) and `replay.initData` (17,073); `attributes.events`,
  `load.info`, `resumable.events`, `smartcam.events`, `sync.events` go from 0 to a few
  hundred bytes. `message.events` and `sync.history` stay empty (no chat).
- **The folder is not deleted** at the end of the game (still present at 16:29). It
  did not exist before the game (checked around 16:00) and appeared at 16:04:35, all files
  created at the same time. When it disappears (client closed? next game
  launched?) **has not been observed**.
- **Identity with the final replay**, decoded with the final build's tables: tracker **4,519 / 4,519
  identical events**, game events **6,022 / 6,022 identical**, `details` and `initData`
  identical, `battlelobby` **byte-identical**. The event streams were compared
  **decoded**, not byte by byte: they decode identically, so a live parse carried through to the
  end yields the final parse.
- The final replay also contains only `_userid`s 0 and 16: in Sandbox, the AIs do not emit
  game events. This is not a peculiarity of the temporary file.

## Limits

1. **A Sandbox game against the AI**: a single human, so a single order emitter (the 13-min
   stop is a deliberate pause, see Write cadence). In a matchmade game, the volume of orders (10 humans) changes the
   block fill time, hence the latencies; the presence of the orders **of the 9 others**
   in the temporary file mid-game remains to be checked.
2. **Build not readable mid-game** (see Observation protocol): only learned at the end, from
   the final replay or `replay.initData`/`details`, which only appear at the final flush.
