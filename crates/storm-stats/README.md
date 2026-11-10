# storm-stats

**Heroes of the Storm** match stats from a decoded replay (`storm-replay`) — faithful Rust
port of [hots-parser](https://github.com/ebshimizu/hots-parser).

```rust
let out = storm_stats::process_replay(std::path::Path::new("match.StormReplay"), "match.StormReplay");
if out.status == 1 {
    let json = out.to_json(); // hots-parser's { match, players } shape
}
```

## Conception

- **1:1 port of `parser.js`** (3,360 lines), bugs included: the output reproduces exactly
  hots-parser's `{match, players, status}`, including its failure statuses. Any deliberate
  deviation is a **documented tolerance**, never silent.
- **Parity proven, not claimed**: `tools/parity-harness/` runs hots-parser (Node) and
  storm-stats on the same corpus and diffs them **field by field**.
  → **114/114 replays green** (79 with an identical full parse, 35 rejected identically).
  Details: `docs/research/2026-06-12-jalon2-parite.md`.
- **Embedded constants** (`data/constants.json`, `data/attr.json`): exported from
  hots-parser, regenerable.

## Scope (full SotS parity)
Match/player identity · score screen (~80 stats/player) + awards · talents per tier · draft
(ordered picks/bans, first pick) · enriched takedowns (positions, participants, revenges) ·
objectives **of the 16 maps** (immortals, dragon, spiders, tributes, altars, skulls, ships,
punishers, Braxis with wave strength, temples, thrones, mines, seeds, gems, protector) ·
mercs/structures · periodic XP + level advantage · team fights/uptime · taunts/BM
(bsteps, dances, sprays, voicelines) · messages/pings · votes · globes · team stats.

## Accepted divergences (storm-stats > hots-parser)
- **Ping coordinates** `match.messages.*.point.x/y`: hots-parser uses GaryIrick's
  heroprotocol port, which decodes this field wrongly (signed overflow); storm-stats follows the
  Blizzard port and gives the correct value (`tools/parity-harness/tolerances.json`).
- **Recent ARAM maps** (`EXTRA_MAPS`: Silver City, Lost Cavern, Braxis Outpost, Industrial
  District): missing from the `MapType` of hots-parser 7.55.7, which rejects them. storm-stats
  parses them (minimal objective, the rest through the universal paths) — ~30% of the archive
  otherwise lost. Validated by a structural invariant (`tests/extended_maps.rs`), no Node baseline.

## Performance
Full parse (decode + stats), Ryzen 7 7800X3D single-threaded, `fast-alloc` feature:
**133 ms median** on a representative sample (spec budget: < 150 ms), dominated by
decoding. `storm-stats-dump --bench <dir>` to reproduce.

## Licence
MIT. Logic derived from hots-parser (@ebshimizu, MIT). Not affiliated with Blizzard.
