# Milestone 0 — go/no-go bench: `.StormReplay` decoding Rust vs .NET vs Python

**Date:** 2026-06-12 · **Verdict: GO** (spec criterion: complete decoding < 500 ms/replay and
required fields present — both are met by a wide margin).
**Plan executed:** `docs/plans/2026-06-12-jalon0-spike-decode.md` · code: `spike/`.

## Machine & toolchains

Windows 11 gaming PC — AMD Ryzen 7 7800X3D. Rust 1.94.0 · .NET SDK 10.0.301 ·
Python 3.13.5. Single-threaded, in-process measurements, warm-up excluded, optimised builds
(`--release` / `-c Release`).

## Corpus

50 real replays drawn from the local archive (2,652 files), stratified 17/17/16 over
2024/2025/2026 (the archive contains nothing before 2024 — the spec said "2023→2026",
adjusted to reality). 13 distinct base builds: 92665 → 97039.
Script: `spike/sample_corpus.ps1` → `corpus/spike50/` (not committed) + `manifest.csv`.

## Measured scope (identical for the 3 engines)

File read + MPQ opening + decoding of **header + replay.details + replay.tracker.events**
(no game/message events — not required for storm-stats on the hot path).

| Engine | n | failures | median | p95 | max |
|---|---|---|---|---|---|
| **Rust `storm-decode` (spike)** | 50 | **0** | **12 ms** | **21 ms** | **24 ms** |
| .NET Heroes.StormReplayParser 2.2.1 | 50 | 1 | 37 ms | 60 ms | 72 ms |
| Python heroprotocol 2.55 (+mpyq) | 50 | 0 | 161 ms | 244 ms | 318 ms |

Raw CSVs: `spike/bench-results/{rust,dotnet,python}.csv` (not committed, reproducible).

- The go/no-go threshold (500 ms) is beaten by a factor of ~40 at the median; the **milestone 1 budget**
  ("< 150 ms excluding stats") is already met by a factor of ~7, and even the spec's *full parse
  decode + stats* budget (150 ms) has ~138 ms of margin for storm-stats.
- Backfill extrapolation (3,000 replays, 8 cores, ~21 ms p95): **< 10 s of decoding** —
  the "3-year backfill < 5 min" budget will be dominated by upload/Postgres, not the parse.
- .NET fails on 1 replay (`2025-11-03 19.58.34 Lost Cavern.StormReplay`, status=Exception)
  that Rust and Python decode without error — the .NET fallback would therefore also have a
  completeness cost, not only a perf one.

## Field accuracy ("required fields present" criterion)

`spike/crosscheck.py`: Rust output compared field by field with heroprotocol on the 3 most
recent replays (2026-06-09, build 97039) — **3/3 identical**: base_build,
elapsed_game_loops, map, 10 players (name/hero/result), exact number of tracker events.
`cargo test` smoke tests: header (signature + build), details (10 players, 5 winners),
tracker (> 1,000 events, SStatGameEvent + SScoreResultEvent present).

## Spike architecture (what worked)

- **MPQ:** `nom-mpq` 2.0.8 crate (the one from s2protocol-rs) — nothing to report, handles sectors/compression.
- **Versioned decoder:** direct port of `VersionedDecoder`/`decoders.py` (heroprotocol, MIT).
  The format is 100 % byte-aligned → a byte cursor is enough (no bit reader).
- **Protocol tables:** exported as JSON from a GitHub clone of Blizzard/heroprotocol
  (`spike/export_protocols.py`, 390 builds) and **interpreted at runtime** — already fast
  enough; Rust code generation (protocol-gen, milestone 1) is an optimisation, not a
  prerequisite.
- **"Latest known protocol" fallback validated for real:** corpus build 97039 has no
  published protocol (latest = 96477) and decodes perfectly with it; same for the 50 files
  on the Python side (pip stops at 91756 → the whole corpus went through the fallback, 0 failures).

## Findings to keep for later

1. **PyPI heroprotocol is undead**: pip version 2.55.4.91756 (broken on Python ≥ 3.12,
   `import imp`) vs GitHub 2.55.15.96477. → milestone 1's protocol-gen must generate from the
   **GitHub clone**, never from pip. (importlib workaround in `spike/bench_python.py`.)
2. The local archive's 2024+ replays cover 13 builds; milestone 1's multi-build reference
   corpus will have to widen (NAS/box + free replays for the CI mini-corpus).
3. .NET parses game events by default (~5× slower: 202 ms vs 37 ms median) — any
   future comparison must fix the scope via `ParseOptions`.
4. The .NET failure on Lost Cavern (ARAM map) is not investigated — irrelevant going forward
   (engine ruled out), but to keep in mind if we re-bench.

## Decision

**GO milestone 1 (storm-replay in Rust).** The .NET fallback is definitively ruled out: Rust is
~3× faster than .NET on this scope, decodes 50/50 files, and the output is
identical to the Blizzard reference. Next step: milestone 1 plan (publishable `storm-replay`
crate: 7 streams, protocol-gen from the GitHub clone, multi-build corpus, < 150 ms).
