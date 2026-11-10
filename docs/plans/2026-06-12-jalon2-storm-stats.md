# Milestone 2 — `storm-stats` crate: hots-parser port + parity harness

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** a `crates/storm-stats` crate that turns a `storm_replay::Replay` into
`MatchStats` (= hots-parser's `{match, players}` output), with an **automatic field-by-field
diff against hots-parser 7.55.7 (Node)** on a corpus covering the 16+ maps.
**Accept (spec): green diff on the corpus, documented tolerances.**

**Architecture:** parser.js (3,360 lines) is the **functional spec** — we port its
semantics phase by phase, bugs included (a deliberate gap = documented tolerance, never
silent). The Rust output serialises (serde) exactly hots-parser's JSON shape to
make the diff trivial; milestone 3's Postgres projection will be done from the Rust type, not
from this JSON. The harness (`tools/parity-harness/`): `dump.js` (Node ref., **done**, writes
to a file because pino pollutes stdout) + `storm-stats-dump` (Rust) + `diff.py` (deep-compare,
exclusion list = THE tolerances, cache of the reference dumps — Node takes ~1-3 s/replay).

**Refs:** `%TEMP%\hots-parser\parser.js` (read the phase BEFORE porting it),
`constants.js` (446 lines: units per map, awards, talents…, to transpose into a generated or
hand-written `constants.rs`), `docs/research/hots-replay-data-reference.md` (the format's
bible), real output: 33 `match` keys, ~40 `player` keys (noted during the session).

---

### Task 1: stats corpus per map + crate skeleton + diff harness

- `spike/sample_stats_corpus.ps1`: from the local archive, ≥ 3 replays per distinct map
  (map name = end of the file name) + the spike50 corpus → `corpus/stats/` (~100 files).
- `crates/storm-stats` (workspace): deps `storm-replay` (path), `serde`/`serde_json`,
  `thiserror`; bin `storm-stats-dump <replay> <out.json>`.
- `tools/parity-harness/diff.py`: `--corpus <dir>` → for each replay, Node dump (cached in
  `corpus/stats/.ref/`) + Rust dump, deep-compare with: float tolerance 1e-6, `null` ≡ absent,
  exclusions read from `tools/parity-harness/tolerances.json` (JSON paths + reason);
  `--only <prefixes>` for the progressive diff during the port.
- Commit.

### Task 2: match + player identity (phase 1 of processReplay)

Port: header/details/initdata/attributes → `match.{version,type,loopLength,filename,mode,
map,date,rawDate,region,playerIDs,heroes,loopGameStart,length}` + `players.{hero,name,uuid,
region,realm,ToonHandle,tag,team,build,mode,version,map,date,rawDate,length,win,
internalHeroName,heroLevel,skin,announcer,mount,silenced,voiceSilenced}`.
Careful: `attr.js` (attributes → mode mapping), date = winFileTimeToDate (offset -610 s,
see the bible), tag from battletags (getBattletags reads the initdata). `--only` diff green. Commit.

### Task 3: score screen + talents + draft

- `processScoreArray` (SScoreResultEvent → `gameStats` ~80 stats + awards), talents per tier
  (game events SHeroTalentTreeSelectionEvent + names via tracker), `globes`, `votes`.
- Draft: `match.bans` (SHeroBannedEvent), `match.picks` (ordered SHeroPickedEvent),
  `firstPickWin`, `turn`. Diff green. Commit.

### Task 4: tracker unit pipeline (the big piece)

UnitBorn/UnitDied/UnitOwnerChange… loop: enriched takedowns (positions, participants,
chain kills, revenges), `deaths`, `lifespan`, `mercs` (camps), `structures`,
`levelTimes`, `XPBreakdown` (PeriodicXPBreakdown), `computeLevelDiff`/`analyzeLevelAdv` →
`levelAdvTimeline`, `firstFort/firstKeep(+Win)`, `team0/1Takedowns`. Diff green. Commit.

### Task 5: per-map objectives (16 branches)

processReplay's switch per map → normalised `match.objective` (immortals+duration, dragon,
spiders, tributes, altars, skulls, ships, punishers, **braxisWaveStrength**, temples,
thrones, mines, seeds, gems, protector) + `getFirstObjectiveTeam` → `firstObjective(Win)`.
Each map validated by diff on its corpus replays. Commit (possibly several).

### Task 6: game events — taunts/BM, messages, with/against

`processTauntData`: bsteps (frame sequences, BSTEP_FRAME_THRESHOLD=8), dances, sprays,
voicelines, taunts + context (near an enemy, kill nearby); `match.messages` (chat/pings);
`with`/`against`; `units`. Diff green. Commit.

### Task 7: team stats + uptime

`collectTeamStats` → `match.teams`, `analyzeUptime`/`analyzeTeamPlayerUptime`/
`timeWithHeroAdv`. Diff green. Commit.

### Task 8: full diff + bench + finishing touches

- `diff.py --corpus corpus/stats` **without `--only`**: green (exclusions = tolerances
  documented in tolerances.json with a reason; copy them into the crate's README).
- Bench: decode + stats on corpus/spike50, single-threaded — **spec budget < 150 ms/replay
  median** (decode alone = 102 ms; stats must fit in ~45 ms).
- Crate README, STATUS.md (milestone 2 done), commit + **push**.

---

## Known pitfalls

- hots-parser freezes `version: 7` (PARSER_VERSION) and legacy fields — reproduce as is.
- JS `undefined` → serialised as `null` by dump.js: the diff treats null ≡ absent.
- JS iteration orders (objects = insertion): arrays must match exactly,
  objects are compared by keys.
- Floats: positions /4096 (fixedData), percentages — tolerance 1e-6, no more.
- ARAM/brawl: some maps have no objective logic in hots-parser — the output
  must be identically empty, not absent.
- Node: always `dump.js` to a file (pino writes to stdout).
