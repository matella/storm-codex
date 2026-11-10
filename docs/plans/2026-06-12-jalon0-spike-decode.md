# Milestone 0 — go/no-go spike: Rust decoding of `.StormReplay`

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** prove (or disprove) that a complete Rust decoding of real HotS replays stays under
**500 ms/replay**, comparing it with the .NET (Heroes.StormReplayParser) and Python
(heroprotocol) engines on a corpus of 50 local replays — documented go/no-go verdict.

**Architecture:** throwaway-but-clean code in `spike/` (the repo is self-sufficient, we are on
the gaming PC: 2,652 replays in `C:\Users\matth\Documents\Heroes of the Storm\...\Replays\Multiplayer`).
The Rust decoder invents nothing: MPQ reader = `nom-mpq` crate (the one from s2protocol-rs), "versioned"
decoder ported from `decoders.py` of the pip package `heroprotocol` (exact Blizzard reference),
protocol tables exported as JSON from the `protocolXXXXX.py` modules installed by pip and
**interpreted at runtime** (Rust code generation is milestone 1 — not the spike).

**Tech stack:** Rust 1.94 (`nom-mpq` 2, `serde_json`, `anyhow` tolerated in the spike), .NET 10
(`Heroes.StormReplayParser` 2.2.1, NuGet), Python 3.13 (`heroprotocol` 2.55.15.96477, pip).

**Acceptance criterion (spec §Milestones):** 3 real replays (builds ≥ 2024) decoded in Rust
(header + details + trackerevents, required fields present and exact vs heroprotocol) **and**
bench of 50 replays: complete Rust decoding < 500 ms/replay. Otherwise → .NET fallback recorded in the spec.

---

## Context for the implementer (zero domain knowledge assumed)

- A `.StormReplay` is an **MPQ archive** (Blizzard format). It contains:
  - a **"user data" section** at the head of the archive (outside the files): the replay's *header*,
    which gives `m_version.m_baseBuild` (build number → choice of protocol table);
  - embedded files, including `replay.details` (players, map, heroes, winner) and
    `replay.tracker.events` (stats event stream).
- All these blobs are encoded in Blizzard's **"versioned"** format: a tagged bytestream
  (struct/blob/array/int zigzag-varint…). The **exact reference** is the file `decoders.py`
  (classes `BitPackedBuffer` + `VersionedDecoder`) of the pip package `heroprotocol` — it will be on
  disk after Task 1, locate it with:
  `python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`
- Each build has a module `versions/protocolXXXXX.py` containing `typeinfos` (the type
  grammar) and constants (`replay_header_typeid`, `game_details_typeid`,
  `tracker_eventid_typeid`, `tracker_event_types`…). The `main()` function of
  `heroprotocol.py` (same package) shows the canonical sequence
  (header decoded with the *latest* protocol → `baseBuild` → the build's protocol → streams).
- **Context7/doc rule:** Context7 does not index these niche libs (checked on 2026-06-12).
  The references are: the local pip code (heroprotocol), https://docs.rs/nom-mpq (crate API),
  https://github.com/sebosp/s2protocol-rs (nom-mpq usage example),
  https://github.com/HeroesToolChest/Heroes.StormReplayParser (API .NET).
- Repo conventions: conventional commits; no `unwrap()` outside tests in durable code —
  in `spike/`, `anyhow` + `?` are enough (the spike will not be published).

**Bench methodology (identical for the 3 engines, otherwise the comparison is worthless):**
single-threaded, *in-process* measurement (no process start-up cost in the measurement), decoding
header + details + trackerevents at minimum, 1 warm-up replay excluded, optimised builds
(`cargo run --release`, `dotnet run -c Release`), CSV output `file,ms` + summary
median / p95 / max on the same 50 files.

---

### Task 0: corpus of 50 replays + repo hygiene

**Files :**
- Create: `.gitignore`
- Create: `spike/sample_corpus.ps1`
- Output (not committed): `corpus/spike50/*.StormReplay`, `corpus/spike50/manifest.csv`

**Step 1 — `.gitignore` at the root:**

```gitignore
corpus/
target/
spike/protocols/
spike/bench-results/
**/bin/
**/obj/
__pycache__/
.venv/
```

**Step 2 — `spike/sample_corpus.ps1`:** sample stratified by year (builds follow
time; mtime = build proxy good enough for the spike): up to 13 replays/year 2023→2026
drawn at random + **the 3 most recent guaranteed** ("3 real replays builds ≥ 2024" criterion),
topped up with the most recent if a year is sparse, exactly 50 in total, copied to
`corpus/spike50/`, manifeste CSV `name,bytes,mtime`.

```powershell
$src = "$env:USERPROFILE\Documents\Heroes of the Storm\Accounts\*\*\Replays\Multiplayer"
$dst = Join-Path $PSScriptRoot "..\corpus\spike50"
New-Item -ItemType Directory -Force $dst | Out-Null
$all = Get-ChildItem "$src\*.StormReplay" | Sort-Object LastWriteTime
$pick = [System.Collections.Generic.List[object]]::new()
$all | Select-Object -Last 3 | ForEach-Object { $pick.Add($_) }            # 3 most recent
foreach ($y in 2023..2026) {
  $year = $all | Where-Object { $_.LastWriteTime.Year -eq $y -and $_ -notin $pick }
  $year | Get-Random -Count ([Math]::Min(13, $year.Count)) | ForEach-Object { $pick.Add($_) }
}
$rest = $all | Where-Object { $_ -notin $pick } | Sort-Object LastWriteTime -Descending
$rest | Select-Object -First ([Math]::Max(0, 50 - $pick.Count)) | ForEach-Object { $pick.Add($_) }
$pick = $pick | Select-Object -First 50
$pick | ForEach-Object { Copy-Item $_.FullName $dst }
$pick | Sort-Object LastWriteTime |
  Select-Object @{n='name';e={$_.Name}}, @{n='bytes';e={$_.Length}},
                @{n='mtime';e={$_.LastWriteTime.ToString('s')}} |
  Export-Csv (Join-Path $dst 'manifest.csv') -NoTypeInformation
"$((Get-ChildItem $dst -Filter *.StormReplay).Count) replays copied"
```

**Step 3 — run and check:**

Run: `pwsh -File spike/sample_corpus.ps1`
Expected: `50 replays copied`, manifest.csv listing mtimes spread over 2023→2026
(check that at least ~4 distinct years appear; otherwise adjust and rerun).

**Step 4 — commit :**

```bash
git add .gitignore spike/sample_corpus.ps1
git commit -m "feat(jalon0): spike corpus sampling script (50 stratified replays)"
```

---

### Task 1: Python baseline (heroprotocol)

**Files :**
- Create: `spike/bench_python.py`
- Output (not committed): `spike/bench-results/python.csv`

**Step 1 — installer :** `python -m pip install --upgrade heroprotocol mpyq`
(heroprotocol depends on mpyq; check with `python -m heroprotocol --header <a corpus replay>`
that it decodes).

**Step 2 — read the reference:** open `heroprotocol.py` and `decoders.py` in the folder given
by `python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`.
Confirm the canonical sequence (exact names to copy as is from the code read):

```python
archive = mpyq.MPQArchive(path)
contents = archive.header['user_data_header']['content']
header = <protocole latest>.decode_replay_header(contents)
base_build = header['m_version']['m_baseBuild']
protocol = <module of build base_build>
details = protocol.decode_replay_details(archive.read_file('replay.details'))
tracker = list(protocol.decode_replay_tracker_events(archive.read_file('replay.tracker.events')))
```

**Step 3 — `spike/bench_python.py`:** for each `.StormReplay` of `corpus/spike50/` (sorted
order), `time.perf_counter()` timer around the sequence above, 1st file replayed as
warm-up (measurement discarded), writes `spike/bench-results/python.csv` (`name,ms,base_build,events,ok`)
and prints `median / p95 / max ms` + number of failures (a failure does not stop the bench: `ok=0` +
message). p95 = `sorted(ms)[ceil(0.95*n)-1]`.

**Step 4 — run:**

Run: `python spike/bench_python.py`
Expected: 50 lines, ~0 failures on recent builds, median expected between ~300 ms and ~2 s
(it is the slow baseline). Note the corpus's distinct `base_build`s (will be used in Task 3).

**Step 5 — commit :**

```bash
git add spike/bench_python.py
git commit -m "feat(jalon0): bench baseline heroprotocol (Python)"
```

---

### Task 2: .NET baseline (Heroes.StormReplayParser)

**Files :**
- Create: `spike/bench-dotnet/` (console project)
- Output (not committed): `spike/bench-results/dotnet.csv`

**Step 1 — create the project:**

```bash
dotnet new console -o spike/bench-dotnet
dotnet add spike/bench-dotnet package Heroes.StormReplayParser
```

**Step 2 — `Program.cs`:** same rules as the Python bench. API (checked on the README of the
repo, v2.2.1):

```csharp
StormReplayResult result = StormReplay.Parse(path); // default options: tracker events included
// result.Status == StormReplayParseStatus.Success; result.Replay.StormPlayers;
// result.Replay.ReplayVersion (build)
```

Stopwatch per file, warm-up excluded, CSV `name,ms,build,ok` to
`spike/bench-results/dotnet.csv`, median/p95/max summary. Failure = `ok=0`, we continue.

**Step 3 — run:**

Run: `dotnet run -c Release --project spike/bench-dotnet`
Expected: median in the order of 50–300 ms/replay. It is the **target to beat** and the fallback if no-go.

**Step 4 — commit :**

```bash
git add spike/bench-dotnet
git commit -m "feat(jalon0): bench baseline Heroes.StormReplayParser (.NET)"
```

---

### Task 3: export of the protocol tables as JSON

**Files :**
- Create: `spike/export_protocols.py`
- Output (not committed): `spike/protocols/<base_build>.json` + `spike/protocols/latest.txt`

**Step 1 — `spike/export_protocols.py`:** enumerate the `protocol*.py` modules of the
`versions/` folder of the installed heroprotocol package, import them one by one, and serialise for each
a JSON `spike/protocols/{base_build}.json`:

```json
{
  "base_build": 92264,
  "typeinfos": [["_int", [[0, 7]]], ["_struct", [[["m_flags", 12, 0]]]], ...],
  "replay_header_typeid": ..., "game_details_typeid": ..., "svaruint32_typeid": ...,
  "replay_userid_typeid": ..., "tracker_eventid_typeid": ...,
  "tracker_event_types": {"0": [..., "NNet.Replay.Tracker.SPlayerStatsEvent"], ...}
}
```

`typeinfos` and the constants exist under these names in each module (check on a
module opened in the editor; `json.dumps` converts tuples to lists by itself). Also write
`latest.txt` containing the highest base_build exported (= "latest" protocol to
decode the header). Old protocols have no tracker events (< 2014): if an attribute
is missing, omit it from the JSON rather than fail.

**Step 2 — run:**

Run: `python spike/export_protocols.py`
Expected: "N protocols exported" (N ≈ 80–120), and the `base_build`s noted in Task 1
all present in `spike/protocols/`.

**Step 3 — commit :**

```bash
git add spike/export_protocols.py
git commit -m "feat(jalon0): JSON export of heroprotocol typeinfos per build"
```

---

### Task 4: Rust spike `storm-decode` — MPQ + header

**Files :**
- Create: `spike/storm-decode/` (cargo binary)
- Test: `spike/storm-decode/tests/decode.rs` (corpus paths via env `SPIKE_CORPUS`)

**Step 1 — create the crate:**

```bash
cargo new spike/storm-decode --name storm-decode
cargo add -p storm-decode nom-mpq serde_json anyhow
```

(If the root workspace does not exist, the crate lives on its own — do not create a workspace for the spike.)

**Step 2 — read the references before writing:** (a) `decoders.py` of the pip package —
`BitPackedBuffer` (big-endian bit reading) and `VersionedDecoder` (one tag byte per node:
struct / blob / array / choice / optional / bool / fourcc / bitarray / real / **vint zigzag**);
copy the semantics method by method, including `_skip_instance`. (b) docs.rs/nom-mpq +
the s2protocol-rs repo for the exact API: parse the archive from the bytes, read the **user data
section** (header) and `read_mpq_file_sector("replay.details", …)` for the embedded files.

**Step 3 — `versioned.rs` module (test first):** type `Value` (Int(i64), Blob(Vec<u8>),
Bool, Array(Vec<Value>), Struct(Vec<(String, Value)>), Optional(Option<Box<Value>>), Fourcc,
BitArray, Null) + `VersionedDecoder::instance(typeinfos_json, typeid) -> Result<Value>`.
First write `tests/decode.rs::header_smoke`: open the most recent replay of
`$env:SPIKE_CORPUS` (= `corpus/spike50`), decode the header with the `latest.txt` protocol,
assert `m_signature == "Heroes of the Storm replay 11"` and `m_version.m_baseBuild > 70000`.

Run: `cargo test -p storm-decode` → Expected: FAIL (compile error then assertions) as long as
versioned.rs + mpq.rs are not finished; iterate until PASS.

**Step 4 — binary:** `storm-decode <replay>` prints a JSON summary:
`{base_build, signature, elapsed_game_loops}` (header only at this stage).

Run: `cargo run --release -p storm-decode -- <recent replay>` → Expected: JSON consistent with
`python -m heroprotocol --header <same replay>`.

**Step 5 — commit :**

```bash
git add spike/storm-decode
git commit -m "feat(jalon0): storm-decode — MPQ + versioned decoder + header"
```

---

### Task 5: Rust spike — details + tracker events

**Files :**
- Modify: `spike/storm-decode/src/main.rs` (+ `details.rs`, `tracker.rs`)
- Test: `spike/storm-decode/tests/decode.rs` (additional cases)

**Step 1 — details (test first):** test `details_smoke`: decode `replay.details` of the same
replay with the protocol of the `base_build` read in the header (`game_details_typeid`), assert
10 players, each with non-empty `m_name` and `m_hero`, non-empty `m_title` (map).
Implement: read the embedded file + `instance(...)` + typed extraction of the fields.

**Step 2 — tracker events (test first):** test `tracker_smoke`: decode
`replay.tracker.events`, assert > 1,000 events and that at least one `SPlayerStatsEvent` and
one `SScoreResultEvent` appear. Implement the stream decoding by porting
`decode_replay_tracker_events` / `_decode_event_stream` from the Python protocol read in Task 4
(loop: delta `svaruint32` → eventid (`tracker_eventid_typeid`) → struct of
`tracker_event_types[eventid]`; **no userid for tracker events** — check this detail
in the Python code, do not infer it).

**Step 3 — complete output + cross-check:** `storm-decode <replay>` now prints
`{base_build, map, players:[{name,hero,result}], tracker_events: N, elapsed_game_loops}`.
Create `spike/crosscheck.py`: for the **3 most recent replays** of the corpus, compare this
output with the same fields extracted via heroprotocol (subprocess `--json` or import); print
`OK`/diff per field.

Run: `python spike/crosscheck.py` → Expected: `3/3 OK` (identical fields). It is the
"required fields present" half of the acceptance criterion.

**Step 4 — commit :**

```bash
git add spike/storm-decode spike/crosscheck.py
git commit -m "feat(jalon0): storm-decode — details + tracker events + cross-check heroprotocol"
```

---

### Task 6: Rust bench of 50 replays + go/no-go report

**Files :**
- Modify: `spike/storm-decode/src/main.rs` (`--bench <dir>` mode)
- Create: `docs/research/2026-06-12-jalon0-bench.md`
- Modify: `docs/STATUS.md` (and `docs/specs/2026-06-12-storm-codex-design.md` **only if no-go**)

**Step 1 — bench mode:** `storm-decode --bench corpus/spike50` — same rules as the two
other engines (single-threaded, warm-up excluded, header+details+tracker, failure → `ok=0` and we
continue), CSV `spike/bench-results/rust.csv`, median/p95/max summary.

Run: `cargo run --release -p storm-decode -- --bench corpus/spike50`

**Step 2 — report `docs/research/2026-06-12-jalon0-bench.md`:** machine (CPU), versions of the
3 toolchains, median/p95/max/failures table per engine on the same 50 files, build
coverage of the corpus, result of the 3-replay cross-check, **explicit verdict**:
- Rust median and p95 < 500 ms + cross-check 3/3 → **GO milestone 1**;
- otherwise → **NO-GO**: record the .NET fallback in the spec (§Decisions and §Risks), same milestones.

**Step 3 — update `docs/STATUS.md`:** "Where we are" section (spike done, verdict,
report link), "Next step" (milestone 1 storm-replay if go — plan to write; otherwise milestone 1
in .NET), remove the "replay sample" blocker (resolved: we are on the gaming PC).

**Step 4 — commit :**

```bash
git add docs/research/2026-06-12-jalon0-bench.md docs/STATUS.md spike/storm-decode
git commit -m "feat(jalon0): bench 3 moteurs + rapport go/no-go"
```

---

## Known pitfalls (to reread before Task 4–5)

- **BitPackedBuffer reads bits big-endian** and the versioned `vint` is a **zigzag varint
  in whole bytes** (bit 7 = continuation, bit 6 of the 1st byte = sign) — do not confuse the
  two paths; everything is in `decoders.py`, port it, do not reinvent it.
- Versioned structs are **tagged per field** (tag = 3rd element of the typeinfos entries) and
  tolerate unknown fields (`_skip_instance`) — essential for multi-build compat.
- `replay.details` ↔ protocol of the **header's base_build**, not the latest; the header itself
  decodes with the **latest** (heroprotocol behaviour, including the unknown-build fallback).
- Some very old replays (< 2015, alpha builds) may fail everywhere: a failure common
  to the 3 engines is not a Rust no-go, it is a data point for the report.
- mpyq/nom-mpq: embedded files are compressed per sector — let the lib handle it;
  never reimplement decompression.
