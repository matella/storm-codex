# Live feed spike — a matchmade game with 10 humans (2026-10-25)

> Spike required by `docs/specs/2026-10-24-feed-live-en-partie-design.md` ("Spike" section, criteria
> 1 and 3) before declaring the feature finished. Follow-up to `docs/research/2026-10-24-tempwritereplay-live.md`
> (one Sandbox game, a single human). **A single game observed**, in **ARAM** (not QM or Storm
> League): 10 humans, matchmade, but at the action pace specific to ARAM.

## The game

- Final replay: `2026-10-25 14.32.20 Braxis Outpost.StormReplay` (863,735 bytes), mode `50101`
  (ARAM), build **98348**, 16,511 loops, loss on the operator's side (`matella#21409`, Kel'Thuzad, player 9).
- Heroes: Anduin, Li-Ming, Zagara, Genji, Stitches (team 0) · Arthas, Rehgar, Azmodan, Kel'Thuzad,
  Kerrigan (team 1).

## Setup

- **Real watcher**: `client-rs` (Hots-Overlay repo, `src/live.rs`), release binary, headless mode,
  on the real `%TEMP%\Heroes of the Storm\TempWriteReplayP1\`. During the game it only sends
  up to the last 4 KB multiple of each stream, then the tail at the final flush.
- **Server**: `storm-codex-server` release (same binary as `f2feb90`) locally on `:8091`, dev
  Postgres. The box was not involved: the measured delays come from the game's writing, not from the server.
- **Recorder** (throwaway script, outside the repo): every 2 s, `GET /api/live?scope=all` + size
  of each temporary file (shared open) + path of the `HeroesOfTheStorm_x64` process; copy
  of the folder every 30 s. The watcher's log (`client.log`, `live:` lines) gives each block
  sent with its time and the `last_loop` answered by the server.

## Point 1 — are the 10 players' orders there mid-game? **Yes.**

- The temporary `game.events` contains the orders of all **10** players during the game: the `all` view
  shows ability presses for players 1 to 10, all of which appeared on the page between
  **14:16:12 and 14:16:55**, less than two minutes after loading. In loops (final replay), the
  first ability order of each `_userid` falls between **694 and 1,398**, around the gates
  opening (loop 1,206): five players before, five in the ~12 s that follow.
- Distinct `_userid`s in the final replay: `0–9` (the 10 humans) and `16` (non-player), as in
  Sandbox for the operator alone. The `allies` / `all` levels therefore do show something.
- Final counters (ability presses, `all` view): Anduin 493, Li-Ming 801, Zagara 561, Genji 255,
  Stitches 381, Arthas 239, Rehgar 248, Azmodan 429, Kel'Thuzad 662, Kerrigan 588.

## Point 2 — block cadence and delays, with 10 humans

Method: the real-time curve is the upper envelope of the `last_loop`s of the blocks received
(linear regression, then shifted onto the freshest point): **16.23 loops per wall-clock
second**. The delay of a reading = ("real-time" loop at that instant − displayed loop) / 16.23.
It is a lower bound: it takes as reference the freshest block observed, itself at most
a second or two old.

| | `tracker.events` | `game.events` (abilities) |
|---|---|---|
| Final size | 254,936 B | 1,636,416 B |
| 4 KB blocks sent | 54 (+ tail) | 396 (+ tail) |
| Interval between blocks | median **17 s**, p90 25 s, max 32 s | median **2 s**, p90 3 s, max 11 s |
| Age of a block's last event when sent | median 18.7 s, p90 27.3 s, max 41 s | median 3.4 s, p90 4.4 s, max 17.4 s |
| **Delay shown on the page** (readings / 2 s) | median **28 s**, p90 39 s, max 52 s | median **4.7 s**, p90 6.9 s, max 29 s |

**It is the opposite of the Sandbox.** With 10 humans, `game.events` fills in ~2 s: abilities
are near real time. The tracker, however, stays slow to fill its blocks: deaths, levels and talents
arrive with a **~30 s** median delay, up to ~50 s. The spec announced "~10 s for the tracker,
up to ~85 s for abilities" based on the Sandbox. Criterion 2 (< 200 ms server-side) is not
affected: these delays are the game's. The watcher holds nothing back during the game: the 491 size
readings taken during the game are **all multiples of 4,096** for both files (56 and
385 distinct sizes), so no tail was waiting for alignment.

The last partial block arrived at the end: `details` + `initData` seen at **14:32:20** (final
replay written at 14:32:20), tail sent and `POST /api/live/end` at **14:32:22** (the watcher's 2 s
stabilisation), page in `ended` at the 14:32:23 reading. No `409`, no resend during the game.

## Point 3 — source of the build mid-game: **the game process's path**

- Throughout the game, the process is
  `C:\Program Files (x86)\Heroes of the Storm\Versions\Base98348\HeroesOfTheStorm_x64.exe`: the
  `BaseNNNNN` folder of the running binary **is** the game's build (98348, confirmed by the final
  replay). This is also true from the menu, before the lobby.
- **Not in the battlelobby**: 98348 appears there neither in ASCII, nor as little/big-endian u32, nor in
  UTF-16 (checked on the battlelobby of a 98348 game).
- **The highest `Versions\BaseNNNNN` is not a safe source**: the folder keeps all the old
  builds (Base93640 … Base98348, 24 folders); the highest is the right one today, but nothing
  guarantees that a more recent downloaded build is not already running, or the reverse.
- Not implemented: the watcher does not send `X-Build` yet; the server fell back on the latest
  embedded protocol (`assumed_latest: true`) and decoded the game without error.

## Criterion 3 — final live state = projection of the final replay: **checked**

- **Bytes**: the temporary folder's `tracker.events`, `game.events` and `battlelobby` are
  **byte-identical** to the final replay's streams, and decode to the end with no remainder
  (4,433 tracker events, 140,168 game events).
- **State**: the final replay uploaded to the same server (`match_id` 2746) against the final live view,
  per hero: team, deaths, takedowns and the 7 talents (same `PurchaseName`, same order) **match
  for all 10 players**; team levels 24 / 23 on both sides. The operator is deduced from the lobby
  (`me_source: lobby`, player 9 = Kel'Thuzad), map "Braxis Outpost".

## Folder lifecycle (point 5, partial)

- The folder did not exist at 14:13 (client launched at 14:10, in the menu); it is created when the
  game loads (battlelobby read at 14:14:59). The one from the day before (game at 21:33) had disappeared by
  21:40 (cause not observed). Disappearance at the end of a game, client open: not observed.
- Reconnection mid-game: not tested.

## Still open

- **Point 4** — align the clock (loops − GatesOpen) / 16 with the clock shown in game: requires
  the time shown to the operator at a given instant, not recorded. Gates here at loop **1,206** (ARAM),
  versus 610 in Sandbox.
- **A QM or Storm League game**: the tracker cadence depends on the action pace; ARAM is
  denser in fights than a game on a large map. To re-measure to confirm the order of magnitude.
- **Point 6** (ability naming): not addressed.

## Side finding (outside the live feed)

In the post-game projection of the replay (`GET /api/matches/2746`), the heroes named **in the
death events** (`deaths[].victim.hero`, `killers[].hero`) are not those of the game: for
example player `2-Hero-1-2844614`, Kel'Thuzad in `players[].hero`, appears there as "Kharazim",
and Murky, Zarya, Medivh, Nova, Tracer, Stukov appear without having been played. `players[].hero`,
the counters and the live view are correct. Probably linked to ARAM hero selection; to analyse
on the `storm-stats` side.
