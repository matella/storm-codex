# Companion live — Plan 1: `storm-lobby` crate (standalone parser + parity)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** produce a pure Rust crate that, from the bytes of a
`replay.server.battlelobby` alone, finds the players of a game by their **BattleTag** and infers
their team, proven at **≥ 99%** against the full parse on the archive.

> **Revised on 2026-08-27 after task 1.** The investigation established that the blob carries **no
> toon handle, no picked hero, no map and no explicit team field** — only the BattleTags,
> in clear text. Tasks 2 to 5 were rewritten accordingly; task 1 is closed and its
> findings are authoritative: `docs/research/2026-08-27-lobby-format.md`.

**Architecture:** `storm-lobby` depends on nothing (it receives `&[u8]`). Its reference truth is
`storm-stats`, which knows the real composition of each game: each archived `.StormReplay`
contains the same lobby blob as the one written live, so each replay is both a
sample and its own answer key. `storm-replay` and `storm-stats` only come in as
**dev-dependencies** — the published crate stays pure.

**Tech Stack:** Rust 2021, `thiserror`, `cargo test` integration tests, `cargo run
--example`.

## Why this plan stops at the parser

The spec also covers the server, the front and the `client-rs` watcher. They are **not** in this
plan: the battlelobby binary format is unknown to date, and writing the exact code of the
server/front tasks today would freeze details (presence of the hero, of the map) that
only task 1 can decide. Plan 2 (server + front) is written as soon as this plan is green;
plan 3 (`client-rs` watcher, Hots-Overlay repo) last, it is the only one that needs the gaming PC.

This plan alone produces a useful and testable deliverable: a proven parser, with figures.

## Global Constraints

- Rust edition 2021; workspace lints: `clippy::unwrap_used = deny`, `clippy::expect_used = warn`.
  Test files open with `#![allow(clippy::expect_used, clippy::unwrap_used)]`, like
  `crates/storm-stats/tests/extended_maps.rs`.
- Typed errors via `thiserror`. No `unwrap()` outside tests.
- `storm-lobby` is **pure**: no I/O, no runtime dependency on `storm-replay`/`storm-stats`.
- Any field whose presence is not guaranteed by the format is an `Option`. The parser never fills
  a gap with an invented value.
- Conventional commits. Co-author not required for this repo.
- Tasks 1 to 4: 100% on the Mac, no box access. Only task 5 reads the box's archive
  (Tailscale, in the evening), **read-only**.
- All of V1 happens on the `feat/companion-live` branch.

---

## Preparation

- [ ] **Step 1: Create the branch**

```bash
git checkout -b feat/companion-live
```

- [ ] **Step 2: Check that the base is green before touching anything**

Run: `cargo test --workspace`
Expected: PASS (no pre-existing failure to confuse with ours)

---

## Files

| File | Responsibility |
|---|---|
| `crates/storm-replay/examples/dump_lobby.rs` | extract the lobby blob of a `.StormReplay` to a file (inspection tool) |
| `docs/research/2026-08-27-lobby-format.md` | inspection report: what the blob contains, answers to the 4 open questions |
| `crates/storm-lobby/Cargo.toml` | manifest; minimal runtime deps, oracle in dev-deps |
| `crates/storm-lobby/src/lib.rs` | API publique (`parse`, `Lobby`, `LobbyPlayer`, `LobbyError`) |
| `crates/storm-lobby/tests/oracle.rs` | parity against `storm-stats` on the 5 committed replays |
| `crates/storm-lobby/tests/robustness.rs` | truncated / empty / random inputs → `Err`, never `panic` |
| `crates/storm-lobby/examples/parity.rs` | parity harness over an archive folder (task 5) |
| `Cargo.toml` (root) | add `crates/storm-lobby` to the workspace members |
| `docs/research/2026-08-27-lobby-parity.md` | go/no-go report with figures |

---

## Task 1: Dump tool and format inspection

**Files:**
- Create: `crates/storm-replay/examples/dump_lobby.rs`
- Create: `docs/research/2026-08-27-lobby-format.md`

**Interfaces:**
- Consumes: `storm_replay::Replay::open()` and `::battlelobby_raw()` (existing,
  `crates/storm-replay/src/lib.rs:90` and `:258`).
- Produces: the factual answers task 3 needs to write the parser, and the working
  `.bin` files.

This task is investigation, not TDD: you cannot test a hypothesis about a format
you have not read yet. Its deliverable is a **findings document**, and everything else depends on it.

- [ ] **Step 1: Write the dump tool**

```rust
//! Extracts the `replay.server.battlelobby` stream of a .StormReplay to a raw file.
//! It is the same blob as the one written live by the game in
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.
//!
//! Usage: cargo run -p storm-replay --example dump_lobby -- <replay> <output.bin>
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = || anyhow::anyhow!("usage: dump_lobby <replay.StormReplay> <output.bin>");
    let input: PathBuf = args.next().ok_or_else(usage)?.into();
    let output: PathBuf = args.next().ok_or_else(usage)?.into();

    let replay = storm_replay::Replay::open(&input)?;
    let blob = replay.battlelobby_raw()?;
    std::fs::write(&output, &blob)?;
    println!("{} bytes → {}", blob.len(), output.display());
    Ok(())
}
```

- [ ] **Step 2: Check that the tool runs**

```bash
mkdir -p /tmp/lobby && \
cargo run -p storm-replay --example dump_lobby -- \
  "crates/storm-stats/tests/data/silver-city-aram.StormReplay" /tmp/lobby/silver-city.bin
```

Expected: a line `NNNNN bytes → /tmp/lobby/silver-city.bin`, non-zero size.

- [ ] **Step 3: Dump the 5 committed replays**

```bash
for f in crates/storm-replay/tests/data/*.StormReplay crates/storm-stats/tests/data/*.StormReplay; do
  out="/tmp/lobby/$(basename "$f" .StormReplay).bin"
  cargo run -q -p storm-replay --example dump_lobby -- "$f" "$out"
done
ls -l /tmp/lobby/
```

Expected: 4 or 5 `.bin` files (a replay may appear twice if it is shared).

- [ ] **Step 4: Extract the readable strings**

```bash
strings -n 3 /tmp/lobby/silver-city.bin | head -80
```

Note: do the BattleTags (`name#1234`) appear? In what order? Are there names
that look like hero identifiers (`HeroTychus`, `Tychus`…) or map identifiers?

- [ ] **Step 5: Locate the anchors to the byte**

```bash
xxd /tmp/lobby/silver-city.bin | head -60
xxd /tmp/lobby/silver-city.bin | grep -i -m5 'tychus'
```

Goal: find out how a player record is delimited (length-prefixed? separator?) and
what surrounds each BattleTag.

- [ ] **Step 6: Compare a live blob with its archived counterpart (spec question no. 4)**

To do when you are on the gaming PC, or to postpone to task 5 if you are not. Copy
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` right after a load,
then, once the game is over, dump the blob of the matching replay and compare:

```bash
cmp /tmp/lobby/live.bin /tmp/lobby/archive.bin && echo "IDENTIQUES" || echo "DIFFERENTS"
```

If identical → the replay↔lobby link can be made by a byte hash (more exact).
Otherwise → we keep the link by set of `toon_handle`, as planned by default.

- [ ] **Step 7: Write the findings report**

Create `docs/research/2026-08-27-lobby-format.md` answering explicitly, **with yes or no and
the supporting proof** (hex excerpt or `strings` excerpt):

1. Are the BattleTags present, and in lobby order?
2. Are the components of the toon handle (`m_region`, `m_programId`, `m_realm`, `m_id`)
   present, or does the handle have to be rebuilt another way?
3. Can team membership be inferred (explicit field, or position/order)?
4. **Is the picked hero present?** If so: in what form, and is it the hero actually
   played in ARAM (where the 3-choice shuffle pollutes the replay's attribute 4002 — see `docs/STATUS.md`)?
5. Are the map and the mode present?
6. Is the live blob bit-for-bit identical to the archived blob (step 6, or "not tested").

Any unknown answer must be written "unknown", never guessed.

- [ ] **Step 8: Commit**

```bash
git add crates/storm-replay/examples/dump_lobby.rs docs/research/2026-08-27-lobby-format.md
git commit -m "feat(storm-replay): dump_lobby example + battlelobby format inspection report"
```

---

## Task 2: `storm-lobby` crate and parity oracle (red)

> **Revision of 2026-08-27, after task 1.** The blob contains **no toon handle, no hero, no
> map and no explicit team field** (`docs/research/2026-08-27-lobby-format.md`). The only
> identity it exposes is the **BattleTag** (`name#discriminant`), in clear text, prefixed with a
> length **in UTF-8 bytes**. The oracle therefore compares BattleTags, and the public type no longer
> carries any field the format cannot fill.

**Files:**
- Create: `crates/storm-lobby/Cargo.toml`
- Create: `crates/storm-lobby/src/lib.rs`
- Create: `crates/storm-lobby/tests/oracle.rs`
- Modify: `Cargo.toml` (root, `members`)

**Interfaces:**
- Produces: `storm_lobby::parse(&[u8]) -> Result<Lobby, LobbyError>`, `Lobby { players:
  Vec<LobbyPlayer> }`, `LobbyPlayer { name: String, discriminant: String, team: Option<u8> }` and
  `LobbyPlayer::battletag() -> String`. These names are the ones plan 2 (server) will consume —
  do not rename them.
- Consumes (dev only): `storm_replay::Replay`, `storm_stats::process_replay`.

- [ ] **Step 1: Create the manifest**

```toml
[package]
name = "storm-lobby"
version = "0.1.0"
description = "Standalone parser for the Heroes of the Storm lobby file (replay.server.battlelobby)"
keywords = ["heroes-of-the-storm", "lobby", "parser", "blizzard"]
categories = ["parser-implementations", "games"]
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
regex = "1"
thiserror = "2"

# The oracle needs the full decoder and parser to know the truth of each replay.
# In dev-dependencies only: the published crate stays pure.
[dev-dependencies]
storm-replay = { path = "../storm-replay" }
storm-stats = { path = "../storm-stats" }
serde_json = "1"
anyhow = "1"

[lints]
workspace = true
```

- [ ] **Step 2: Declare the crate in the workspace**

In the root `Cargo.toml`, replace the `members` line with:

```toml
members = ["crates/storm-replay", "crates/storm-stats", "crates/storm-codex-server", "crates/storm-replay-viewer", "crates/storm-lobby"]
```

- [ ] **Step 3: Write the public API, without implementation**

`crates/storm-lobby/src/lib.rs`:

```rust
//! Standalone parser for the Heroes of the Storm lobby file.
//!
//! The game writes `replay.server.battlelobby` during the loading screen, before the replay
//! exists. This crate reads that blob **alone** — without the replay's `details` stream — to identify
//! the players of a game in progress.
//!
//! What the format really exposes, and what it does not, is recorded in
//! `docs/research/2026-08-27-lobby-format.md`: the BattleTags are in clear text, but neither the toon
//! handle, nor the picked hero, nor the map, nor an explicit team field can be found there. The public
//! type therefore only carries what is really decodable. Resolving BattleTag → application
//! identity happens downstream, on the server, against the archive of games already played.
//!
//! Pure crate: no I/O, no dependency on `storm-replay`.

use thiserror::Error;

/// A lobby player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    /// Account name, without the discriminant. May contain non-ASCII UTF-8 (see the
    /// Cyrillic case documented in the format report).
    pub name: String,
    /// Discriminant alone, the part after `#`.
    pub discriminant: String,
    /// 0 or 1. `None` when membership could not be determined — the format carries no
    /// team field, it is inferred from the order (see `parse`).
    pub team: Option<u8>,
}

impl LobbyPlayer {
    /// `"name#1234"` — the player's identity key. This is what the server will match against
    /// `match_players.name` + `match_players.data->>'tag'` to find their history.
    #[must_use]
    pub fn battletag(&self) -> String {
        format!("{}#{}", self.name, self.discriminant)
    }
}

/// A decoded lobby. `players` is in order of appearance in the blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
}

#[derive(Debug, Error)]
pub enum LobbyError {
    #[error("blob too short ({0} bytes) to contain a lobby")]
    TooShort(usize),
    #[error("no identifiable player in the blob")]
    NoPlayers,
    #[error("unrecognised lobby structure: {0}")]
    Unrecognized(String),
}

/// Decodes a `replay.server.battlelobby` blob.
///
/// # Errors
/// Returns [`LobbyError`] if the blob is too short or contains no identifiable player.
/// Never panics, whatever the input.
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError> {
    Err(LobbyError::Unrecognized(format!(
        "parse not implemented ({} bytes)",
        bytes.len()
    )))
}
```

- [ ] **Step 4: Write the oracle**

`crates/storm-lobby/tests/oracle.rs`:

```rust
//! Parity oracle: from the lobby blob alone, the parser must find the players that the
//! full parse of the replay identifies — by BattleTag, the only identity the format exposes.
//!
//! Each `.StormReplay` carries the same blob as the one written live by the game: each replay is
//! therefore both a sample and its own answer key.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn data(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// The replays committed in the workspace.
fn replays() -> Vec<PathBuf> {
    [
        "crates/storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay",
        "crates/storm-stats/tests/data/silver-city-aram.StormReplay",
    ]
    .iter()
    .map(|p| data(p))
    .collect()
}

/// Reference truth: `{battletag: team}` according to the full parse of the replay.
/// Players whose discriminant the full parse did not resolve are set aside: the oracle
/// cannot judge what its own reference does not know.
fn truth(path: &Path) -> BTreeMap<String, i64> {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("file name");
    let out = storm_stats::process_replay(path, filename);
    assert_eq!(out.status, 1, "{filename}: full parse rejected");
    let json = out.to_json();
    json["players"]
        .as_object()
        .expect("players")
        .values()
        .filter_map(|p| {
            let name = p["name"].as_str()?;
            let tag = p["tag"].as_i64()?;
            let team = p["team"].as_i64()?;
            Some((format!("{name}#{tag}"), team))
        })
        .collect()
}

fn lobby_of(path: &Path) -> storm_lobby::Lobby {
    let replay = storm_replay::Replay::open(path).expect("ouverture replay");
    let blob = replay.battlelobby_raw().expect("stream battlelobby");
    storm_lobby::parse(&blob).expect("parse lobby")
}

#[test]
fn parse_retrouve_les_battletags_du_replay() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let expected = truth(&path);
        assert_eq!(expected.len(), 10, "{label}: incomplete reference");

        let decoded: BTreeMap<String, Option<u8>> = lobby_of(&path)
            .players
            .iter()
            .map(|p| (p.battletag(), p.team))
            .collect();

        let attendus: Vec<&String> = expected.keys().collect();
        let obtenus: Vec<&String> = decoded.keys().collect();
        assert_eq!(obtenus, attendus, "{label}: divergent BattleTags");
    }
}

/// The format carries no team field: it is inferred from the order (first 5 / last 5).
/// This assumption was only observed on 2 samples in task 1 — this test checks it on the
/// committed corpus, and the parity harness (task 5) checks it on the full archive.
#[test]
fn les_equipes_deduites_correspondent_au_replay() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let expected = truth(&path);
        for p in &lobby_of(&path).players {
            let Some(team) = p.team else { continue };
            let attendu = expected
                .get(&p.battletag())
                .unwrap_or_else(|| panic!("{label}: unknown BattleTag \"{}\"", p.battletag()));
            assert_eq!(
                i64::from(team),
                *attendu,
                "{label}: divergent team for {}",
                p.battletag()
            );
        }
    }
}

/// Regression. A Cyrillic BattleTag is present in this replay as multi-byte UTF-8; `strings`
/// does not see it. A parser that assumed ASCII would silently lose one player out of ten
/// in European games — without failing, which is worse. See the format report, Q1.
#[test]
fn battletag_non_ascii_est_decode() {
    let path = data("crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
    let lobby = lobby_of(&path);
    assert!(
        lobby
            .players
            .iter()
            .any(|p| p.name == "ЛовкийЭльф" && p.discriminant == "215346"),
        "Cyrillic BattleTag missing — decoded: {:?}",
        lobby.players.iter().map(LobbyPlayer::battletag).collect::<Vec<_>>()
    );
}
```

Note: the last `assert!` references `LobbyPlayer`; add `use storm_lobby::LobbyPlayer;` at the
top of the file if the compiler asks for it, or replace it with `|p| p.battletag()`.

- [ ] **Step 5: Run the oracle and check that it fails for the right reason**

Run: `cargo test -p storm-lobby --test oracle`
Expected: the 3 tests FAIL on `parse lobby` → `Unrecognized("parse not implemented (… bytes)")`.
A failure on anything else (compilation, replay not found, `status != 1`, `incomplete reference`)
must be fixed before task 3 — otherwise the oracle proves nothing.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/storm-lobby
git commit -m "test(storm-lobby): crate skeleton + BattleTag parity oracle (red)"
```

---

## Task 3: Implement the parser until green

**Files:**
- Modify: `crates/storm-lobby/src/lib.rs`

**Interfaces:**
- Consumes: `docs/research/2026-08-27-lobby-format.md` (task 1 findings).
- Produces: a working `parse()`. Signature and types **unchanged** compared with task 2.

- [ ] **Step 1: Extract the BattleTags**

Reuse the regex already proven in production in `crates/storm-stats/src/process.rs:313`:

```rust
r"[\p{L}\d]{3,24}#\d{4,10}[zØ]?"
```

applied to `String::from_utf8_lossy(bytes)`. `\p{L}` covers Cyrillic, and `from_utf8_lossy`
decodes multi-byte UTF-8 correctly — it was `strings` that did not see them in task 1, not
this approach. The task 2 test `battletag_non_ascii_est_decode` is the judge of this precise
point: do not disable or loosen it.

Split each occurrence at the first `#`: the left part is `name`, the right one `discriminant`.
Keep the order of appearance and **deduplicate** — the blob may contain the same string several
times; the first occurrence of each BattleTag is authoritative for the order.

Run: `cargo test -p storm-lobby --test oracle`
Expected: `parse_retrouve_les_battletags_du_replay` and `battletag_non_ascii_est_decode` PASS;
`les_equipes_deduites_correspondent_au_replay` PASS trivially as long as `team` is `None`.

- [ ] **Step 2: Infer the teams from the order**

The format carries no team field (format report, Q3). Rule, and nothing more
ambitious: **if and only if exactly 10 players were decoded**, the first 5 get
`Some(0)` and the last 5 `Some(1)`. In every other case — 9 players, 11, a non-standard
mode — leave `None` for all.

This is deliberately conservative: a wrong team is worse than a missing team, since it
would show an opponent as an ally. The task 5 harness will measure whether the order assumption
holds on the archive; as long as it is not measured, it only applies to exactly 10 players.

```rust
if players.len() == 10 {
    for (i, p) in players.iter_mut().enumerate() {
        p.team = Some(u8::from(i >= 5));
    }
}
```

Run: `cargo test -p storm-lobby --test oracle`
Expected: the 3 tests PASS.

- [ ] **Step 3: Handle the degenerate cases**

`parse` must return `LobbyError::TooShort(len)` on a blob obviously too short to
contain a lobby, and `LobbyError::NoPlayers` when no BattleTag is found. Never
return an empty `Lobby` as `Ok`: plan 2's server distinguishes "unreadable lobby" from "lobby
read" to choose what it shows.

- [ ] **Step 4: Check green and cleanliness**

Run: `cargo test -p storm-lobby`
Expected: PASS

Run: `cargo clippy -p storm-lobby --all-targets -- -D warnings`
Expected: no warning (`unwrap_used` is `deny` at the workspace level).

- [ ] **Step 5: Commit**

```bash
git add crates/storm-lobby
git commit -m "feat(storm-lobby): BattleTag extraction and team inference — oracle green"
```

---

## Task 4: Robustness to malformed inputs

**Files:**
- Create: `crates/storm-lobby/tests/robustness.rs`
- Modify: `crates/storm-lobby/src/lib.rs` (if a test reveals a panic)

**Interfaces:**
- Consumes: `storm_lobby::parse`, `storm_lobby::LobbyError`.
- Produces: the "never panics" guarantee, which plan 2's server endpoint depends on — a
  `panic!` in an axum handler would kill the request at every Blizzard build change.

- [ ] **Step 1: Write the robustness tests**

```rust
//! `parse` consumes potentially malformed binary (new Blizzard build, file read
//! while the game is writing it). It must ALWAYS return, never panic.
//!
//! In the last two tests, **the absence of panic IS the assertion**: the test harness
//! fails by itself if `parse` panics. The result is deliberately ignored, since no return
//! value is "correct" on a corrupted input — only the absence of panic is.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

fn blob_reel() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/storm-stats/tests/data/silver-city-aram.StormReplay");
    let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
    replay.battlelobby_raw().expect("stream battlelobby")
}

#[test]
fn entree_vide_est_une_erreur() {
    assert!(storm_lobby::parse(&[]).is_err(), "empty blob accepted");
}

#[test]
fn entree_aleatoire_est_une_erreur_pas_une_panique() {
    // Deterministic generator (xorshift): no dependency, reproducible result.
    let mut state: u32 = 0x1234_5678;
    let mut bytes = Vec::with_capacity(4096);
    for _ in 0..4096 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        bytes.push((state & 0xff) as u8);
    }
    assert!(
        storm_lobby::parse(&bytes).is_err(),
        "random noise was accepted as a lobby"
    );
}

#[test]
fn blob_tronque_a_toute_longueur_ne_panique_jamais() {
    let blob = blob_reel();
    // A file read while being written is a prefix of the complete file: we exercise exactly
    // that case, at every truncation point (step of 97 to stay fast).
    for n in (0..blob.len()).step_by(97) {
        let _ = storm_lobby::parse(&blob[..n]);
    }
}

#[test]
fn blob_corrompu_en_son_milieu_ne_panique_jamais() {
    let mut blob = blob_reel();
    let milieu = blob.len() / 2;
    let fin = (milieu + 64).min(blob.len());
    for b in &mut blob[milieu..fin] {
        *b = 0xff;
    }
    let _ = storm_lobby::parse(&blob);
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p storm-lobby --test robustness`
Expected: PASS. A panic (`index out of bounds`, `byte index … is not a char boundary`) is a
real defect to fix in `parse` — replace direct indexing with `get()`/`get(..n)` and
propagate `LobbyError::TooShort`.

- [ ] **Step 3: Check that nothing regressed**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/storm-lobby
git commit -m "test(storm-lobby): robustness — empty, truncated, corrupted, random inputs"
```

---

## Task 5: Parity harness on the archive and go/no-go report

**Files:**
- Create: `crates/storm-lobby/examples/parity.rs`
- Create: `docs/research/2026-08-27-lobby-parity.md`

**Interfaces:**
- Consumes: `storm_lobby::parse`, `storm_replay::Replay`, `storm_stats::process_replay`.
- Produces: the two figures that decide the go/no-go — the rate of lobbies whose **10
  BattleTags** are exact (criterion: **≥ 99%**), and the rate whose **teams inferred from
  the order** are exact, which decides whether we show two teams or a flat list of 10.

- [ ] **Step 1: Write the harness**

```rust
//! Parity of the standalone parser against the full parse, over a folder of replays.
//!
//! Usage: cargo run --release -p storm-lobby --example parity -- <folder> [max]
//!
//! Two independent measurements, not to be confused:
//!   - `exact battletags`: the parser finds exactly the game's 10 BattleTags.
//!   - `exact teams`     : among those, the inference from the order (5+5) matches the truth.
//! The first decides the go/no-go; the second decides whether the companion shows two teams.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Tally {
    total: usize,
    stats_rejete: usize,
    lobby_err: usize,
    battletags_exacts: usize,
    equipes_exactes: usize,
    equipes_evaluables: usize,
    divergents: Vec<(String, String)>,
    par_build: BTreeMap<u32, (usize, usize)>, // build → (evaluated, exact battletags)
}

/// `{battletag: team}` according to the full parse, or `None` if the replay is rejected.
fn verite(path: &Path) -> Option<BTreeMap<String, i64>> {
    let filename = path.file_name()?.to_str()?;
    let out = storm_stats::process_replay(path, filename);
    if out.status != 1 {
        return None;
    }
    let json = out.to_json();
    Some(
        json["players"]
            .as_object()?
            .values()
            .filter_map(|p| {
                let name = p["name"].as_str()?;
                let tag = p["tag"].as_i64()?;
                let team = p["team"].as_i64()?;
                Some((format!("{name}#{tag}"), team))
            })
            .collect(),
    )
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: parity <dossier> [max]"))?
        .into();
    let max: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

    let mut t = Tally::default();
    for entry in std::fs::read_dir(&dir)? {
        if t.total >= max {
            break;
        }
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("StormReplay") {
            continue;
        }
        t.total += 1;
        let label = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();

        let Ok(replay) = storm_replay::Replay::open(&path) else {
            t.divergents.push((label, "ouverture impossible".into()));
            continue;
        };
        let build = replay.header.base_build;
        let Ok(blob) = replay.battlelobby_raw() else {
            t.divergents.push((label, "blob absent".into()));
            continue;
        };
        let Some(attendu) = verite(&path) else {
            t.stats_rejete += 1;
            continue;
        };
        if attendu.len() != 10 {
            t.stats_rejete += 1;
            continue;
        }

        let compteur = t.par_build.entry(build).or_default();
        compteur.0 += 1;

        match storm_lobby::parse(&blob) {
            Err(e) => {
                t.lobby_err += 1;
                t.divergents.push((label, format!("parse: {e}")));
            }
            Ok(lobby) => {
                let tags_ok = lobby.players.len() == attendu.len()
                    && lobby.players.iter().all(|p| attendu.contains_key(&p.battletag()));
                if !tags_ok {
                    t.divergents
                        .push((label, format!("{} battletags decoded", lobby.players.len())));
                    continue;
                }
                t.battletags_exacts += 1;
                compteur.1 += 1;

                if lobby.players.iter().any(|p| p.team.is_some()) {
                    t.equipes_evaluables += 1;
                    if lobby.players.iter().all(|p| match p.team {
                        None => true,
                        Some(team) => attendu.get(&p.battletag()) == Some(&i64::from(team)),
                    }) {
                        t.equipes_exactes += 1;
                    } else {
                        t.divergents.push((label, "divergent teams".into()));
                    }
                }
            }
        }
    }

    let base = t.total - t.stats_rejete;
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };
    println!("replays seen         : {}", t.total);
    println!("rejected (stats)     : {}", t.stats_rejete);
    println!("comparison base      : {base}");
    println!(
        "exact battletags     : {} ({:.2} %)",
        t.battletags_exacts,
        pct(t.battletags_exacts, base)
    );
    println!("parse errors         : {}", t.lobby_err);
    println!(
        "exact teams          : {} / {} evaluable ({:.2} %)",
        t.equipes_exactes,
        t.equipes_evaluables,
        pct(t.equipes_exactes, t.equipes_evaluables)
    );
    println!("\nper build (build: exact battletags/evaluated)");
    for (build, (evalues, exacts)) in &t.par_build {
        println!("  {build}: {exacts}/{evalues}");
    }
    println!("\nfirst 20 divergences");
    for (f, why) in t.divergents.iter().take(20) {
        println!("  {f} — {why}");
    }
    Ok(())
}
```

- [ ] **Step 2: Trial run on the local corpus**

```bash
cargo run --release -p storm-lobby --example parity -- crates/storm-replay/tests/data
```

Expected: `exact battletags : 3 (100.00 %)`. A gap here points to a harness bug, not a
parser bug — the task 2 oracle already covers these replays.

- [ ] **Step 3: Fetch the box's archive (read-only)**

The box runs in the evening. Read-only approach, like the runbook
`docs/runbooks/2026-07-09-visionneuse-2d-verif-box.md`:

```bash
mkdir -p /tmp/archive && \
rsync -av --include='*.StormReplay' --exclude='*' \
  matella@192.168.129.85:~/apps/storm-codex/data/archive/ /tmp/archive/ | tail -3
```

If the archive path differs, find it without changing anything:

```bash
ssh matella@192.168.129.85 "docker inspect storm-codex-server --format '{{range .Config.Env}}{{println .}}{{end}}' | grep -i -E 'archive|data'"
```

If the box is unreachable, **stop there and report it** — do not invent a verdict from the
local corpus of 4 replays alone, which is already covered by the oracle and proves nothing at scale.

- [ ] **Step 4: Run the parity on the full archive**

```bash
cargo run --release -p storm-lobby --example parity -- /tmp/archive 2>&1 | tee /tmp/lobby-parity.txt
```

- [ ] **Step 5: Write the go/no-go report**

`docs/research/2026-08-27-lobby-parity.md`: the harness output, then the verdict.

- **BattleTags ≥ 99 % → GO.**
- **BattleTags < 99% → analyse the divergences per build** before any verdict. If the failure is
  concentrated on a few old builds, it is acceptable and to be documented (the companion serves the
  present, not 2024) — then give the rate **over the current year's builds**, which is the
  figure that matters. If the failure is uniform, it is a no-go: report it to the operator, do not
  adjust the threshold.
- **Teams**: give the rate separately, without mixing it with the previous one. Below ~99%, the
  recommendation is to return `team: None` in all circumstances and show a flat list
  of 10 players rather than two teams of which a part would be wrong. Write the recommendation;
  the decision belongs to the operator.

- [ ] **Step 6: Clean up**

```bash
rm -rf /tmp/archive /tmp/lobby
```

- [ ] **Step 7: Commit**

```bash
git add crates/storm-lobby/examples/parity.rs docs/research/2026-08-27-lobby-parity.md
git commit -m "test(storm-lobby): parity harness on the archive + go/no-go report"
```

---

## End of plan

- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` green
- [ ] `docs/research/2026-08-27-lobby-format.md` answers the 6 questions, without any "maybe"
- [ ] `docs/research/2026-08-27-lobby-parity.md` carries two figures and a verdict
- [ ] `docs/STATUS.md` updated (state + next step), as required by `CLAUDE.md`

Then: write plan 2 (migration `0009`, module `lobby.rs`, routes, `/companion`, `/builds`),
knowing now that the hero and the map will be entered by hand, and that player identity
is resolved by BattleTag against the archive.
