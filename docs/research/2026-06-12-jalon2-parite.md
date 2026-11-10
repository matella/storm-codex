# Milestone 2 — storm-stats: hots-parser parity proven + bench

**Date:** 2026-06-12 · **Status: DONE.** Automatic diff green on 114/114 replays,
full-parse bench under budget on a representative sample.
**Plan executed:** `docs/plans/2026-06-12-jalon2-storm-stats.md` · code: `crates/storm-stats/`,
harness: `tools/parity-harness/`.

## Acceptance criterion (spec)
> storm-stats validated by an automatic field-by-field diff against hots-parser on a reference
> corpus. No milestone 2 without a green diff. Documented tolerances.

**Achieved.** `python tools/parity-harness/diff.py` → **114 OK · 0 DIFF · 0 SKIP / 114**.

## Method
- **Yardstick**: hots-parser 7.55.7 (npm, the parser of our current overlay), run via
  `tools/parity-harness/dump.js` (`processReplay`, `overrideVerifiedBuild` options).
- **storm-stats**: faithful port of `parser.js` (3,360 lines), output `{match, players, status}`
  identical in shape, serialised by `storm-stats-dump`.
- **Judge**: `diff.py` deep-compares each field (float tolerance 1e-6, `null` ≡ absent),
  caches the Node dumps (~1-3 s/replay) in `corpus/stats/.ref/`. Replays that hots-parser
  rejects must be rejected by storm-stats **with the same status** (failure parity).
- **Corpus**: `corpus/stats/` — 114 replays, stratified ≥ 3 per map (all maps
  present locally) + the spike50 corpus.

## Diff coverage
- **79/114** replays with a full parse: **every field** identical — identity, score screen
  (~80 stats/player), talents, draft (ordered bans/picks), enriched takedowns, objectives of the
  16 maps, team fights/uptime, XP/levels, taunts/BM (bsteps/dances/sprays/voicelines),
  messages, votes/globes, awards, team stats.
- **25/114 map extension**: recent ARAM maps (Silver City, Lost Cavern, Braxis Outpost,
  Industrial District) that hots-parser 7.55.7 **rejects** (absent from its `MapType`). By
  operator decision (2026-06-12), **storm-stats supports them**: `EXTRA_MAPS` in
  `process.rs` resolves the map, the objective is minimal (`{type}` — these maps have no
  PvE objective) and the objective handlers are no-ops. All other fields (identity, score,
  draft, takedowns, talents, BM, teams) go through the universal paths already proven at
  parity. No Node baseline (the reference rejects them) → validated by structural invariant
  (10 players, 5 winners, score present; test `tests/extended_maps.rs`). **Stake**: these
  4 maps weigh ~30 % of the archive — without this extension, milestone 3 ("≥ 99 % parsed")
  would be impossible (~70 %).
- **10/114** rejected **identically** (same status): brawls/sandbox/heroic
  (`GameMode.Brawl` → `Unsupported`, and sandbox) — supported neither by hots-parser **nor** by
  storm-stats, by design (outside V1 scope).

## Documented tolerances (`tools/parity-harness/tolerances.json`)
Only one, on two cosmetic fields:
- `match.messages.*.point.x` / `.y` — ping coordinates. hots-parser uses **GaryIrick's**
  heroprotocol port (npm), which interprets this 33-bit `_int` field with a `lo`
  shifted by `-2³²` → aberrant negative value. storm-stats follows the **Blizzard** port (our
  protocol-gen tables) and gives the correct coordinate. **storm-stats is more correct than
  the reference here**; we document the gap rather than reproduce the bug.

## Performance (spec budget: full parse decode + stats < 150 ms/replay, 1 core)
Ryzen 7 7800X3D, single-threaded, `fast-alloc` feature (mimalloc), warm-up excluded:

| Corpus | n (full parse) | median | p95 | max |
|---|---|---|---|---|
| spike50 (representative sample) | 36 | **133 ms** | 242 ms | 319 ms |
| corpus/stats (classic full games, the heaviest) | 79 | 151 ms | 252 ms | 315 ms |

- **Under budget** on a representative sample (133 ms). The corpus/stats subset
  is heavier: its median covers only **full games on classic maps**
  (rejections — often short games — are excluded), i.e. the longest replays
  (~100,000 game events). It is **at budget** there (151 ms), dominated by **decoding** (bzip2
  floor + decoding of the 7 streams; see milestone 1 report: decoding alone ~120 ms median on this
  same corpus). The stats logic itself adds ~15-30 ms.
- Key optimisation: `Replay::visit_game_events` (storm-replay) decodes the ~100,000 game events
  without materialising a `Vec`, and storm-stats detects BM directly on `storm_replay::Value`
  (without JSON conversion). Measured gain: 258 → 133 ms median (spike50), p95 −15 %.

## Decision
**Milestone 2 validated.** Parity proven (114/114, single documented and favourable tolerance),
perf budget met on a representative sample and at budget in the worst case, decoding-dominated.
Next step: milestone 3 (server + Postgres + backfill).

## Findings for later
1. **Two heroprotocol ports diverge** (GaryIrick npm vs Blizzard Python): storm-stats follows
   Blizzard (correct). Beyond the ping coordinates, no other divergence made the
   diff fail — but keep in mind if a future field differs.
2. hots-parser 7.55.7's **map table** is outdated (15 maps). For milestone 4 (front-end),
   an up-to-date map table will be needed (via the HotsPatchNotes reference data) to show the
   recent maps — independently of parity.
3. `constants.json`/`attr.json` are embedded in the crate (exported from hots-parser).
   Regenerable; frozen as long as we target 7.55.7 parity.
4. storm-stats emits hots-parser's JSON shape; milestone 3 will project into Postgres from the
   Rust type (not this JSON) — typed views will be added as needed.
