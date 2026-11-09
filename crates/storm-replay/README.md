# storm-replay

Rust decoder for **Heroes of the Storm** replays (`.StormReplay`): MPQ archive + the
7 streams of the Blizzard protocol (header, details, initdata, attributes, messages,
game events, tracker events).

```rust
let replay = storm_replay::Replay::open("match.StormReplay")?;
println!("build {} — {}", replay.header.base_build, replay.details()?.title);
for event in replay.tracker_events()? {
    // typed Value: structs/blobs/ints faithful to the protocol
}
```

## Conception

- **Generated protocol tables, not hand-written**: `tools/protocol_gen.py` exports the
  `typeinfos` of each build from a GitHub clone of
  [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol) (never the PyPI package,
  obsolete and broken on Python ≥ 3.12), deduplicates them (390 builds → 32 tables, 0.6 MB) and
  embeds them in the crate (`protocols/`). To rerun at each HotS patch.
- **Unknown builds**: fallback to the last known protocol (heroprotocol behaviour),
  flagged by `Replay::protocol_fallback()` — never silent.
- **Lazy per-stream decoding**: `Replay::open` only decodes the header; each stream is
  decoded on demand.
- **Bit-exact parity proven**: `tools/crosscheck_streams.py` deep-compares the output of the
  7 streams against heroprotocol (Python) — field-by-field equality, including ~100,000 game
  events per replay.
- Typed errors (`thiserror`), stable classes designed for the server's failure stats.

## Performances (Ryzen 7 7800X3D, mono-thread, feature `fast-alloc`)

| Measurement | Value |
|---|---|
| 7 full streams, median (corpus of 50 real replays) | **102 ms** |
| 7 full streams, p95 | 205 ms (including a ~50–115 ms incompressible bzip2 floor) |
| Hot path stats (header + details + tracker) | ~12 ms |
| Real archive (2,821 replays, 22 builds 2024→2026) | **100% decoded** |

The `fast-alloc` feature (mimalloc, used by the binaries) divides the decoding time by
~1.6 on Windows — recommended for any heavy consumer.

## Binaires

- `storm-replay-dump <replay> --stream <name>` — JSON dump of a stream (heroprotocol-normalised);
  `--bench <dir>` — full decoding bench.
- `storm-replay-verify <dir> [--csv out.csv]` — recursively decodes a whole archive,
  classifies the failures by error type.

## Licence

MIT. Format reference: heroprotocol (Blizzard, MIT). Heroes of the Storm™ is a trademark of
Blizzard Entertainment; this project is not affiliated with Blizzard.
