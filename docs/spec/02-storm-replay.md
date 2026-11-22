# storm-replay — decoder contract

Crate: `crates/storm-replay` (public library, MIT). Showcase: its `README.md`.

## Contract

- **Input**: a `.StormReplay` file (Blizzard MPQ archive).
- **Output**: the protocol's **7 streams**, as normalised `serde_json::Value` **identical to
  heroprotocol** (bit-exact parity proven by `tools/crosscheck_streams.py`):
  `header`, `replay.details`, `replay.initdata`, `replay.attributes.events`,
  `replay.message.events`, `replay.game.events`, `replay.tracker.events`.
- **Lazy decoding**: `Replay::open` only decodes the header; each stream on demand
  (`details()`, `tracker_events()`, `message_events()`, …).
- **Typed errors** (`thiserror`), stable classes — the server uses them to classify
  upload failures (`error_class`).
- **Raw streams**: `battlelobby_raw()`, `tracker_events_raw()`, `game_events_raw()` return the
  undecoded bytes (live simulator, tests).
- **Input limits** (security review):
  - `versioned.rs` rejects struct/array counts greater than the remaining bytes and grows decoded
    collections only when values are read. `bitpacked.rs` bounds bitarray allocation by input size.
  - Skipping unknown fields (versioned) is limited to 64 nested levels (`MAX_SKIP_DEPTH`).
  - **Zip-bomb guard**: embedded files are read by `mpq.rs` (port of nom-mpq's
    `read_mpq_file_sector`, bounds-checked) and inflated through a `Read::take` budget of
    `MAX_STREAM_BYTES` = **256 MiB per stream**; a larger declared or actual decompressed size is
    `Error::MissingStream`. nom-mpq still parses the archive tables. Real streams are a few MB.

## Chunked decoding (`StreamDecoder`)

For the live feed (`docs/specs/2026-10-24-feed-live-en-partie-design.md`): HotS writes
`replay.tracker.events` and `replay.game.events` during the game, in 4 KB blocks, with no MPQ
header and no build.

- `StreamDecoder::new(StreamKind::{Tracker, Game}, build: Option<u32>)` — `None` = latest embedded
  protocol; a build without a table → fallback reported by `protocol_fallback()`.
- `feed(&[u8]) -> Result<Vec<Value>>` returns the **complete** events, identical field for field
  to `tracker_events()` / `game_events()`; a cut-off event (`Error::Truncated`) waits for the next
  chunk. Any other error is propagated and leaves the decoder in its state from before the call.
- `consumed()`, `pending()`, `gameloop()` expose the resume point.
- The whole-replay path (`tracker_events()`…) is unchanged: perf budget unchanged.

## Protocol tables

- **Generated, never hand-written**: `tools/protocol_gen.py` exports the typeinfos from
  a GitHub clone of `Blizzard/heroprotocol` (not the PyPI package, which is obsolete), deduplicates
  them (390 builds → ~32 tables) and embeds them in `protocols/` (committed).
- **Unknown build** → fallback to the latest known protocol (heroprotocol behaviour),
  reported by `Replay::protocol_fallback()` — never silent.
- **On every HotS patch**: rerun `protocol_gen.py` if the build introduces a new protocol.

## Performance

Median 102 ms for all 7 streams (p95 205 ms, bzip2 floor ~50–115 ms, incompressible); stats hot
path (header+details+tracker) ~12 ms. The `fast-alloc` feature (mimalloc) is recommended for any
heavy consumer (~×1.6 on Windows). Real archive: 2,821/2,821 decoded (22 builds
2024→2026).

## Diagnostic binaries

- `storm-replay-dump <replay> --stream <name>` — JSON dump of a stream; `--bench <dir>`.
- `storm-replay-verify <dir> [--csv]` — decodes a whole archive, classifies failures.

## Tests

`tests/mini_corpus.rs` on the committed replays in `tests/data/` (public CI without the private
corpus). The full corpus lives on the NAS/box (`fetch-corpus`). `tests/stream_decoder.rs`:
chunked decoding (4 KB, pseudo-random sizes, byte by byte) equals whole decoding;
random bytes never cause a panic. `src/mpq.rs` unit tests: byte parity with nom-mpq on every
stream of the committed replays and the output cap with a small compressed fixture.
`src/versioned.rs` tests the count and nesting boundaries directly.
