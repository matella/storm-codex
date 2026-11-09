# Milestone 1 — `storm-replay` crate: complete decoding of the 7 streams

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** a publishable Rust crate (`crates/storm-replay`) that decodes the **7 streams** of a
`.StormReplay` (header, details, initdata, attributes, messages, game events, tracker events),
with generated protocol tables (`protocol-gen`), unknown-build fallback, typed errors.
**Accept (spec): 100 % of the corpus decoded** (= the whole local archive, 2,652 replays) **and
bench < 150 ms/replay excluding stats** (7 streams included).

**Architecture:** the spike (milestone 0) validated nom-mpq + the versioned decoder interpreting
JSON typeinfos (12 ms/replay on 3 streams). Milestone 1 industrialises: root cargo workspace,
`storm-replay` crate (thiserror, no `unwrap()` outside tests, strict clippy), port of the
**BitPackedDecoder** (bit-by-bit big-endian — required by initdata/game/message events, absent from the
spike), attributes decoder (separate little-endian format), **deduplicated committed** tables
(390 builds → 32 distinct contents ≈ 0.5 MB, embedded via `include_str!`), parity proven
stream by stream against heroprotocol.

**Tech stack:** Rust 2021 (lib deps: `serde_json`, `thiserror`; dev: nothing more),
Python 3.13 for protocol-gen/cross-check (GitHub clone of Blizzard/heroprotocol — PyPI forbidden,
see milestone 0 report).

---

## Context for the implementer

- Exact reference of the decoders: `decoders.py` of the heroprotocol pip package
  (`python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`).
  The spike already ported `VersionedDecoder` (`spike/storm-decode/src/versioned.rs`) — the
  **BitPackedDecoder** (class above in the same file) remains to be ported: big-endian
  bit reading (`read_bits`: low-order bits of `_next` placed in the high-order bits of the result),
  `_int` = `bounds[0] + read_bits(bounds[1])` — **the bounds matter here** (the versioned one
  ignores them), sequential `_struct` without tags, `_choice` without skip (unknown tag = corrupted),
  `_bitarray` → (length, integer), `_blob` → byte_align then aligned bytes.
- Attributes: `decode_replay_attributes_events` in any protocol module (e.g.
  protocol91756.py:453) — BitPackedBuffer **little-endian**, scopes/attrid, 4-byte value
  reversed and stripped of `\x00`.
- Event loop: `_decode_event_stream` (protocolXXXXX.py:366) — delta svaruint32,
  userid **only** for game/message events, eventid, struct, `byte_align()` between
  events. Fields added: `_event`, `_eventid`, `_gameloop`, `_userid` (not `_bits`:
  we do not emit it, the cross-check strips it on the Python side).
- Reusable spike: `spike/export_protocols.py` (basis of protocol-gen),
  `spike/storm-decode/src/{versioned,protocol}.rs` (to adapt into a clean lib),
  corpus `corpus/spike50` + complete local archive (~2,652 replays, builds ≥ 92665 → all
  covered by the GitHub protocols, fallback validated for 97039).
- Repo conventions: thiserror in the lib (anyhow tolerated only in the binaries
  outils/tests), commits conventionnels, clippy `-D warnings`.

---

### Task 1: root workspace + crate skeleton

- Root: `Cargo.toml` workspace `members = ["crates/storm-replay"]`,
  `exclude = ["spike/storm-decode"]` (the spike keeps its lockfile, it is frozen).
- `crates/storm-replay`: lib `src/lib.rs` + `src/error.rs` (`thiserror`:
  `Error::{Io, Mpq, Truncated, Corrupted, UnknownType, MissingStream, Protocol}` — classes
  stable error codes, they will be used for the server's failure stats in milestone 3).
- `cargo clippy --workspace -- -D warnings` green. Commit.

### Task 2: protocol-gen (deduplicated committed tables)

- `tools/protocol_gen.py`: from a GitHub clone (automatic clone into `%TEMP%/heroprotocol`
  if absent, `--clone-dir` option), exports per build the **complete** JSON (typeinfos + the 6
  typeid constants + `tracker_event_types` + `game_event_types` + `message_event_types` +
  `game_eventid_typeid`, `message_eventid_typeid`), deduplicates by content hash
  (excluding `base_build`), writes:
  - `crates/storm-replay/protocols/tables/<hash12>.json` (~32 files),
  - `crates/storm-replay/protocols/index.json` (`{"builds": {"92665": "<hash12>", …},
    "latest": 96477}`),
  - generated `crates/storm-replay/protocols/embed.rs`: `static TABLES: &[(&str, &str)] =
    &[("<hash12>", include_str!("tables/<hash12>.json")), …];` + index builds.
- Check: 390 builds in the index, ~32 tables, total size < 1 MB. Commit (tables included —
  rerunnable at each HotS patch).

### Task 3: decoders (ported versioned + new bitpacked)

- `src/value.rs`: the spike's `Value` + `BitArrayInt(u64 bits, u64 value)` (bitpacked variant).
- `src/typeinfo.rs`: `TypeInfo` with **bounds kept** (`Int{lo, bits}`, `Choice{lo, bits,
  fields}`, `Array{lo, bits, typeid}`, `Blob{lo, bits}`, `BitArray{lo, bits}`) — parsed from the
  JSON; the versioned decoder ignores them, the bitpacked one uses them.
- `src/versioned.rs`: port of the spike, adapted (typed `Error` instead of anyhow).
- `src/bitpacked.rs`: big-endian `BitReader` faithful to `BitPackedBuffer.read_bits` + little
  endian for attributes; `BitPackedDecoder::instance`. Unit tests: vint (0, 1, -1,
  0x3F, large numbers), big-endian read_bits on a known pattern (compare with a Python computation
  done by hand in the test's comment).
- Commit.

### Task 4: attributes + embedded tables + event loops

- `src/attributes.rs`: port of `decode_replay_attributes_events` (typed struct
  `Attributes{source, map_namespace, scopes: HashMap<u8, HashMap<u32, Vec<AttributeValue>>>}`).
- `src/protocol.rs`: `Protocol` loaded from the embedded tables (`OnceLock`, JSON parsed
  once per table and per process); `for_build(build) -> (protocol, fallback: bool)`;
  `decode_header/details/initdata`; `decode_tracker_events` (versioned, without userid);
  `decode_game_events`/`decode_message_events` (bitpacked, with userid, `byte_align` between
  events).
- Commit.

### Task 5: public `Replay` API + mini-corpus + integration tests

- `src/lib.rs`: `Replay::open(path)` / `Replay::from_bytes` → MPQ parsed + header decoded at
  opening (typed build, duration, version: `ReplayHeader`); **lazy** streams:
  `details()` (typed view `ReplayDetails{title, players: Vec<PlayerDetails{name, hero, team,
  result, toon…}>, timestamp}` + `details_raw()` Value), `initdata_raw()`, `attributes()`,
  `message_events()`, `game_events()`, `tracker_events()` (Value + event name);
  `protocol_fallback() -> Option<(requested, used)>` (warning up to the caller).
- CI mini-corpus: copy 2 recent replays + 1 from 2024 from the local archive into
  `crates/storm-replay/tests/data/` (~3 MB committed — the operator's replays; flagged in the
  session summary for a possible veto). Integration tests: the 7 streams of each replay
  of the mini-corpus decode, invariants (10 players, 5 winners, > 1,000 tracker events,
  non-empty attributes, game events > tracker events).
- `cargo test --workspace` + clippy green. Commit.

### Task 6: stream-by-stream parity vs heroprotocol

- Bin `src/bin/storm-replay-dump.rs`: `storm-replay-dump <replay> --stream <name>` → JSON
  lines normalised "like heroprotocol" (blobs as latin-1, structs = objects, reals = `[f]`,
  bitarrays according to the decoder, `_event/_eventid/_gameloop/_userid`).
- `tools/crosscheck_streams.py`: on the **3 most recent replays** of the spike50 corpus +
  the 2024 replay of the mini-corpus, for each of the 7 streams: decode via heroprotocol (milestone 0
  importlib loader) and via storm-replay-dump, normalise (strip `_bits`, tuples→lists,
  bytes→latin-1), **deep-compare**. Expected: exact equality everywhere (documented tolerance
  only if a real case requires it — then document it in the report).
- Commit.

### Task 7: archive sweep (100 % criterion) + bench (< 150 ms criterion)

- Bin `src/bin/storm-replay-verify.rs`: walks a directory recursively, decodes the
  7 streams of each `.StormReplay`, classifies failures by `Error`; CSV output + summary.
- Run on the complete local archive (`Documents/Heroes of the Storm/.../Multiplayer`).
  **Accept: 100 % decoded** (otherwise: fix the decoder — the local builds are all
  ≥ 92665, no "alpha builds" excuse).
- Bench: `storm-replay-dump --bench corpus/spike50` (7 streams, single-threaded, warm-up excluded,
  median/p95/max → `spike/bench-results/rust-jalon1.csv`). **Accept: median and p95 < 150 ms.**
- Commit.

### Task 8: finishing touches + STATUS + push

- Crate README (usage, protocol-gen, fallback, measured perf), rustdoc on the public
  API, `cargo fmt` + clippy + tests one last time.
- `docs/STATUS.md`: milestone 1 finished (sweep + bench figures), next step milestone 2.
- Commit + **git push** (operator authorisation of 2026-06-12).

---

## Known pitfalls

- Big-endian `read_bits`: we consume the **low-order** bits of the current byte but
  place them in the **high-order** bits of the result — port the loop as is, do not "simplify".
- Bitpacked `_struct` has **no** tags or skip: every field decodes sequentially;
  an unsuitable protocol ⇒ `Corrupted`, that is the expected behaviour (no tolerance).
- Versioned `_bitarray` = aligned bytes; bitpacked = integer of `length` bits. Two variants
  of `Value`, do not merge.
- Attributes: **little-endian** buffer (the only use), 4-byte values reversed
  (`[::-1]`) then `strip(b'\x00')` — strip on both sides, like Python.
- game events: some types contain exotic bitarrays/fourcc — if an eventid or
  a field breaks on an old replay of the sweep, first compare with a
  `python -m`-style heroprotocol dump of the same file before touching the decoder.
- Reuse the heroprotocol clone in `%TEMP%` if it exists (otherwise re-clone --depth 1).
