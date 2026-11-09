# storm-stats — stats contract & parity

Crate: `crates/storm-stats` (public library, MIT). A **1:1** Rust port of
[hots-parser](https://github.com/ebshimizu/hots-parser) (`parser.js`, 3,360 lines), bugs included.

## Code layout (`src/process/`)

`mod.rs` (`process_replay`, loading, main phases) and six submodules, split out of the single
`process.rs` file (4,700 lines) on 2026-11-07 with no logic change: `js.rs`
(JavaScript semantics: dates, numbers, `for…in`, JSON access), `teams.rs` (team aggregates,
hero presence), `messages.rs` (chat, pings, BM, taunts), `draft.rs` (bans, picks, score
screen), `objectives.rs` (map objectives), `events.rs` (XP, deaths, camps, levels, units,
talents). Proof of the split: `storm-stats-dump` outputs **byte-for-byte identical**
before/after on the 114 replays of the local corpus, and the parity diff green again (114/114).

## Contract

- **Input**: path to a `.StormReplay` + file name (used by some heuristics).
- **Output**: `Output { match_, players, status }` → `to_json()` reproduces exactly the
  `{match, players, status}` shape of hots-parser. `status == 1` = complete parse; the other
  statuses reproduce the reference's rejections (see `reject_class` on the server side).
- **Scope** (full SotS parity): match/player identity · score screen (~80 stats/player) +
  awards · talents per tier · ordered draft (picks/bans, first pick) · enriched takedowns ·
  objectives of the 16 maps · mercs/structures · periodic XP + level advantage · team fights ·
  taunts/BM · **messages/pings/announcements** (`match.messages`) · votes · globes · team stats.

## Parity: the rule

**storm-codex extensions outside parity**: `Output.actions` (`toon → number of SCmdEvent` after the start of the game, basis of APM, absent from `to_json`); `Output.hotkey_profiles` (`toon → m_hotkeyProfile`, `SUserOptionsEvent`), absent from `to_json` and therefore from the Node diff.

**Proven, not claimed.** The `tools/parity-harness/` harness (`dump.js` on the Node side, `diff.py`,
`tolerances.json`) diffs field by field against hots-parser 7.55.7 on the reference corpus:
**114/114 green** (79 identical complete parses, 25 ARAM extension, 10 identical brawl rejections).
Any change to `src/process/` reruns the diff before merging. Run `diff.py` with the
**absolute path** of the corpus if the `.ref/` references were produced that way (otherwise only
`match.filename` differs). Last run: 2026-11-07, 114/114 (`diff.py` finds the
`storm-stats-dump` binary on Linux as on Windows).

## Accepted divergences (the ONLY ones — any new divergence is documented here + tolerances.json)

1. **Ping coordinates** (`match.messages[].point.x/y`): hots-parser decodes via the
   GaryIrick port (signed overflow); storm-stats follows the Blizzard decoder — correct value.
2. **Recent ARAM maps** (`EXTRA_MAPS`: Silver City, Lost Cavern, Braxis Outpost, Industrial
   District): rejected by hots-parser 7.55.7's `MapType` (~30 % of the archive). storm-stats
   parses them (minimal objective, universal paths otherwise). Validated by a structural invariant
   (`tests/extended_maps.rs`), no Node baseline possible.

## Embedded constants

`data/constants.json` + `data/attr.json`, exported from hots-parser (regenerable). They notably
hold `MessageType` (Chat 0 / Ping 1 / LoadingProgress 2 / PlayerAnnounce 5),
`MessageTarget` (All 0 / Allies 1 / Observers 4), `GameMode`, `MapType`, `ScoreEventNames`…
The front-end replicates the values it displays (see [08-frontend.md](08-frontend.md)).

## Reading guide — cast summary (`cast/summary.rs`)

The control room's post-game breakdown (`POST /api/cast/games/{n}/replay`) reads these paths, checked on
≈ 70 real Custom replays (2026-08 → 10) and cross-checked against a second path. **No metric
added to storm-stats**; a missing path gives `None` / empty and the screen drops the row.
Team 0 = Blue, 1 = Red. Numbers may be written as floats (`28.0`).

| Summary value | storm-stats path | Cross-check / note |
|---|---|---|
| `winner` (required) | `match.winner` (0/1) | any other value → `incomplete` |
| `duration_secs` (required) | `match.length`, truncated | = the match page's `fmtDur` (truncates) |
| `takedowns` (required) | `match.team0Takedowns` / `team1Takedowns` | match page header; **not** `match.takedowns` (list of events); ≠ sum of `SoloKill` on a few games |
| `team_level` | `match.teams[t].level` | = max of the keys of `match.levelTimes[t]` (the match page's source) over the whole corpus |
| `merc_camps` | `match.teams[t].stats.mercCaptures` | = number of the team's `match.mercs.captures`. The match page's advanced "Mercs" column sums `gameStats.MercCampCaptures` (participation, ≈ 2×): **not used** |
| `structures` (forts · keeps) | `teams[t].stats.structures.Fort.destroyed + .Keep.destroyed` | buildings only (no towers or wells); a map with no `Fort`/`Keep` subkey at all (in either team) or `structures` missing → `None` (row dropped, never 0–0); otherwise a missing subkey = 0; = number of Fort/Keep `match.structures` destroyed on the opponent's side |
| `objectives` | **allow-list** by `MapType` key (`rt_str("MapType", …)` compared to `match.map`); **timed** events only (non-null `time`, like the match page's `objectiveEvents`) | see the list below; any other map → `None` (row dropped) |
| `bans` | `match.bans[t][].hero` (attribute code) → `attr.json` `heroAttribute` | unknown code ignored |
| `players[]` | key of `players` (= `ToonHandle`) → `toon`; `name`, `hero`, `team`; `gameStats.{SoloKill→kills, Takedowns, Deaths, Assists, HeroDamage, SiegeDamage, Healing}` | `players[].deaths` / `.takedowns` (root) are **lists** of events, not counters; never derive one counter from another (`Takedowns ≠ SoloKill + Assists` on 7 rows of the corpus). Order: Blue then Red, in `match.picks[t]` order |
| `xp_lead` | `match.XPBreakdown[]` matched by `loop`: Σ numeric fields of `breakdown` excluding `*Time*` (like the match page's "Team XP" curve), Blue − Red, `(time truncated, rounded)` | present on 100 % of the Custom corpus; no `levelAdvTimeline` fallback (different unit: levels) |

**Objective allow-list (R17)** — each entry: the storm-stats event (process.rs) that
feeds `objective[t].events`, and the cross-check on the Custom corpus:
- `TowersOfDoom` — `AltarCaptured`: one event per captured altar (count = events, 10–5, 4–9…).
- `CursedHollow` — `TributeCollected`: one event per tribute picked up by the team (≤ `tributes` spawned: 3–0 of 4, 4–8 of 12).
- `DragonShire` — `DragonKnightActivated`: one event per activated Dragon Knight.
- `HauntedWoods` (Garden of Terror) — `GardenTerrorActivated`: one terror activated per event.
- `Crypts` (Tomb of the Spider Queen) — `SpidersSpawned`: one wave of Webweavers (gems turned in) per event.
- `Volskaya` — spawn of the `Triglav` controlled by the team: one protector captured per event.
- `BlackheartsBay` — `GhostShipCaptured`: one ship captured per event (thin corpus: a single game, in the database, 3–2).
- `BattlefieldOfEternity` — `objective.results[].winner` (`ImmortalDefeated`): immortal won.

**Excluded** (row dropped): `ControlPoints` (Sky Temple, one event per temple **shot**, ≈ 241 per
game); `AlteracPass` (one cavalry **unit** per event); `Shrines` (Infernal Shrines:
`PunisherKilled` = punishers **killed** — undercounts shrines won, `shrines[].team` ≥ events on
the corpus, e.g. 3 vs 2); `Warhead Junction` (nukes **launched**, not landed); `HauntedMines` (lists
of golems, no per-team events); `BraxisHoldout`, `Hanamura` (no per-team events);
ARAM maps. When in doubt, a map stays off the list.

**Orientation (R16, `Series::orient`)**: team 0 of the replay is not necessarily the series' Blue.
The `players[]` rows are attached to the series' players (identical linked toon, then exact
name): clear evidence of a swap → `GameSummary::swapped()` (pairs and bans swapped, winner
and sides inverted, `xp_lead` negated); without evidence, team 0 = Blue, unless a contrary
entered winner → `409 side_mismatch`. `?swap=1` forces the swap.

Team comparisons displayed (K/D/A, hero damage, siege damage) = sums of the player rows,
like the match page's "Team total" row.

## Performance

133 ms median full parse (decode + stats, single-threaded, `fast-alloc`) — budget < 150 ms.
Bench: `storm-stats-dump --bench <dir>` (separates complete parses and rejections).
