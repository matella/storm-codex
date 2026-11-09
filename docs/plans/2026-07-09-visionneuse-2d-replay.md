# 2D replay viewer — Implementation plan (MVP-1)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Replay 2D" tab to the match detail that replays hero positions on the minimap with an instant-seek scrub (alive/dead state + death markers; no HP/mana, no animation).

**Architecture:** New crate `storm-replay-viewer` (depends on `storm-replay` only) that projects the decoded events into normalised `[0,1]` geometry keyed by replay `playerId`. A server endpoint `GET /api/matches/{id}/replay2d` decodes on the fly (path + disk cache of `raw.rs`), merges player metadata from Postgres, and emits the JSON. The front loads the whole model once and scrubs 100% client-side on a `<canvas>`.

**Tech Stack:** Rust 2021 (serde, thiserror), storm-replay; axum 0.8 + sqlx (server); React 18 + TS + Vite + TanStack Query + vitest (front).

**Reference spec:** `docs/specs/2026-07-09-visionneuse-2d-replay-design.md`

**Key conventions checked in the code:**
- `storm_replay::Value`: `.field("m_x")`, `.as_int()`, `.as_str_lossy()`, `.as_array()`. Events = `Vec<Value>` of `Struct`.
- Heroes: `SUnitBornEvent` with `m_controlPlayerId` ∈ 1..=10 and `m_unitTypeName` prefixed `"Hero"` (e.g. `HeroSylvanas`, `HeroDVaMech`).
- Time base: `t = (gameloop − 610) / 16`. Extent: `SStatGameEvent{m_eventName:"GameStart"}` → `m_fixedData` `MapSizeX/MapSizeY` (fixed-point ×4096; `map_w = MapSizeX/4096`).
- Tracker coords (`m_x`, `m_items` positions) = whole tiles 0..`map_w`. Game coords (`m_data.TargetPoint.x`) = ×4096 → divide by 4096 before normalising.
- Server: `AppState { cfg: Arc<Config> (raw_cache_dir, raw_cache_max_bytes), db }`; base route `/api/matches/{id}/…` (not `/match/`); players via the table `match_players (match_id, toon_handle, name, hero, team, win)`.
- Front: helper `get<T>(path)`, route `match/:id` → `pages/MatchDetail.tsx`, helpers `mapImage(map)`, `heroIcon(hero)`, `universeColor(hero)`.

**⚠️ Before starting:** create a branch/worktree (`git switch -c feat/replay2d`). Do not work on `main`.

---

## Chunk 1 — Crate `storm-replay-viewer` (extraction → geometry)

**Responsibility:** decoded replay → `ViewerModel` (meta + heroes samples/life + deaths), normalised `[0,1]` coords, replay `playerId` key. **Zero DB / network dependency.**

**Files:**
- Modify: `Cargo.toml` (add the workspace member)
- Create: `crates/storm-replay-viewer/Cargo.toml`
- Create: `crates/storm-replay-viewer/src/lib.rs` (public API `build_model`)
- Create: `crates/storm-replay-viewer/src/model.rs` (serialisable types)
- Create: `crates/storm-replay-viewer/src/extract.rs` (extraction logic)
- Test: `crates/storm-replay-viewer/tests/extract.rs` (mini-corpus + golden)
- Test data: reuse `crates/storm-stats/tests/data/silver-city-aram.StormReplay` (copy to `crates/storm-replay-viewer/tests/data/`)

- [ ] **Step 1: Scaffold the crate and register it in the workspace**

`Cargo.toml` (root) — add to `members`:
```toml
members = ["crates/storm-replay", "crates/storm-stats", "crates/storm-codex-server", "crates/storm-replay-viewer"]
```
`crates/storm-replay-viewer/Cargo.toml`:
```toml
[package]
name = "storm-replay-viewer"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[lints]
workspace = true

[dependencies]
storm-replay = { path = "../storm-replay" }
serde = { version = "1", features = ["derive"] }
thiserror = "1"

[dev-dependencies]
serde_json = "1"
```

- [ ] **Step 2: Define the model types** — `src/model.rs`

```rust
use serde::Serialize;

pub const VIEWER_VERSION: u32 = 1;
pub const LOOP_OFFSET: i64 = 610;
pub const LOOPS_PER_SEC: f64 = 16.0;
pub const FIXED: f64 = 4096.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerModel {
    pub meta: Meta,
    pub heroes: Vec<HeroTrack>,
    pub deaths: Vec<Death>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub map_name: String,
    pub map_size: [f64; 2], // in tiles (MapSize/4096), diagnostic
    pub duration_sec: f64,
    pub loop_offset: i64,
    pub viewer_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroTrack {
    pub player_id: i64, // playerId replay (m_controlPlayerId, 1..=10)
    pub samples: Vec<Sample>,
    pub life: Vec<Interval>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample { pub t: f64, pub x: f64, pub y: f64, pub exact: bool }

#[derive(Debug, Clone, Serialize)]
pub struct Interval { pub from: f64, pub to: f64 }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Death {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub victim_player_id: i64,
    pub killer_player_id: Option<i64>,
}
```

- [ ] **Step 3: Write the basic invariants test (fails)** — `tests/extract.rs`

```rust
use storm_replay::Replay;
use storm_replay_viewer::build_model;

fn model() -> storm_replay_viewer::ViewerModel {
    let replay = Replay::open("tests/data/silver-city-aram.StormReplay").expect("open");
    build_model(&replay).expect("build_model")
}

#[test]
fn ten_hero_tracks_with_samples() {
    let m = model();
    assert_eq!(m.heroes.len(), 10, "10 heroes expected");
    for h in &m.heroes {
        assert!((1..=10).contains(&h.player_id));
        assert!(!h.samples.is_empty(), "player {} without samples", h.player_id);
    }
}

#[test]
fn all_coords_normalized() {
    let m = model();
    for h in &m.heroes {
        for s in &h.samples {
            assert!((0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
                "coord outside [0,1]: {:?}", s);
        }
    }
    for d in &m.deaths {
        assert!((0.0..=1.0).contains(&d.x) && (0.0..=1.0).contains(&d.y));
    }
}

#[test]
fn duration_and_meta_sane() {
    let m = model();
    assert!(m.meta.duration_sec > 60.0, "duration too short");
    assert_eq!(m.meta.viewer_version, 1);
    assert!(m.meta.map_size[0] > 0.0 && m.meta.map_size[1] > 0.0);
}

// The safety net of the user→player mapping: the EXACT samples come from the positions
// (independent of the mapping), so "10 non-empty tracks" proves NOTHING about the mapping of
// commands. These two assertions actually cover it.
#[test]
fn command_densification_lands_on_right_player() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.samples.len()).sum();
    assert!(total > 2000, "command densification missing (total {total}) — user→player mapping broken?");
    for h in &m.heroes {
        let exact = h.samples.iter().filter(|s| s.exact).count();
        let cmd = h.samples.iter().filter(|s| !s.exact).count();
        assert!(exact >= 1 && cmd >= 1, "player {}: exact={exact} cmd={cmd}", h.player_id);
        // Spatial consistency: a player clicks near where their hero is. A wrong
        // mapping (e.g. team offset) would move the clicks away from the exact positions.
        let mean = |it: &dyn Fn(&storm_replay_viewer::Sample) -> bool| {
            let v: Vec<_> = h.samples.iter().filter(|s| it(s)).collect();
            (v.iter().map(|s| s.x).sum::<f64>() / v.len() as f64,
             v.iter().map(|s| s.y).sum::<f64>() / v.len() as f64)
        };
        let (ex, ey) = mean(&|s| s.exact);
        let (cx, cy) = mean(&|s| !s.exact);
        let d = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
        assert!(d < 0.4, "player {}: clicks far from positions (d={d:.2}) — suspicious mapping", h.player_id);
    }
}
```
> This test is the real guard of the mapping (Step 3 was not). Final validation = Chunk 4 visual check.
Copy the replay: `mkdir -p crates/storm-replay-viewer/tests/data && cp crates/storm-stats/tests/data/silver-city-aram.StormReplay crates/storm-replay-viewer/tests/data/`

- [ ] **Step 4: Check the compile/test failure**

Run: `cargo test -p storm-replay-viewer`
Expected: FAIL (`build_model` / `ViewerModel` not found).

- [ ] **Step 5: Implement the extraction** — `src/extract.rs` + `src/lib.rs`

`src/lib.rs`:
```rust
mod extract;
mod model;
pub use model::*;

use storm_replay::Replay;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("replay: {0}")]
    Replay(#[from] storm_replay::Error),
    #[error("missing data: {0}")]
    Missing(&'static str),
}

/// Projects a replay into a viewer model (geometry only, replay playerId key).
pub fn build_model(replay: &Replay) -> Result<ViewerModel, Error> {
    extract::build(replay)
}

/// Authoritative mapping `playerId (tracker, 1..=10) → toon_handle` — so that the server
/// attaches the player metadata from Postgres. Derived from `SPlayerSetupEvent` (m_playerId/m_slotId)
/// crossed with `details().players[*].working_set_slot_id → toon_handle`. Reused by Chunk 2.
pub fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, Error> {
    extract::player_toons(replay)
}
```

`src/extract.rs` — logic (reference, the implementer adjusts as needed):
```rust
use crate::model::*;
use crate::Error;
use std::collections::HashMap;
use storm_replay::{Replay, Value};

fn loop_to_sec(gameloop: i64) -> f64 { (gameloop as f64 - LOOP_OFFSET as f64) / LOOPS_PER_SEC }

pub(crate) fn build(replay: &Replay) -> Result<ViewerModel, Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;

    // 1) Map extent via SStatGameEvent{GameStart}.MapSizeX/Y (×4096)
    let (msx, msy) = map_size(&tracker).ok_or(Error::Missing("MapSize"))?;
    let (map_w, map_h) = (msx / FIXED, msy / FIXED); // in tiles
    let norm_tile = |x: i64, y: i64| (x as f64 / map_w, y as f64 / map_h);
    let norm_world = |x: i64, y: i64| (x as f64 / msx, y as f64 / msy); // coords ×4096

    // 2) unit(tagIndex) → (player, isHero) from SUnitBornEvent (recycle: last wins)
    let mut unit_player: HashMap<i64, (i64, bool)> = HashMap::new();
    for e in &tracker {
        if event_name(e) != "SUnitBornEvent" { continue; }
        let p = field_int(e, "m_controlPlayerId").unwrap_or(0);
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let is_hero = e.field("m_unitTypeName").and_then(Value::as_str_lossy)
            .map(|n| n.starts_with("Hero")).unwrap_or(false);
        if (1..=10).contains(&p) { unit_player.insert(idx, (p, is_hero)); }
    }

    // 3) samples exacts depuis SUnitPositionsEvent (m_items = [idxDelta, x, y]*)
    let mut samples: HashMap<i64, Vec<Sample>> = HashMap::new();
    for e in &tracker {
        if event_name(e) != "SUnitPositionsEvent" { continue; }
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        let items: Vec<i64> = e.field("m_items").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_int).collect()).unwrap_or_default();
        let mut idx = 0i64;
        for tri in items.chunks_exact(3) {
            idx += tri[0]; // index cumulatif
            if let Some(&(p, true)) = unit_player.get(&idx) {
                let (x, y) = norm_tile(tri[1], tri[2]);
                samples.entry(p).or_default().push(Sample { t, x: q3(x), y: q3(y), exact: true });
            }
        }
    }

    // 4) densification via game events SCmd*/TargetPoint keyed by _userid → playerId.
    //    AUTHORITATIVE mapping via SPlayerSetupEvent (m_userId → m_playerId) — NOT the fragile
    //    "slot+1" assumption. `user_to_player` built once from the tracker.
    let user_to_player = user_to_player(&tracker); // m_userId → m_playerId (1..=10)
    let mut cmd_samples: HashMap<i64, Vec<Sample>> = HashMap::new();
    replay.visit_game_events(|ev| {
        let Some(tp) = ev.field("m_data").and_then(|d| d.field("TargetPoint")) else { return; };
        let (Some(x), Some(y)) = (tp.field("x").and_then(Value::as_int),
                                  tp.field("y").and_then(Value::as_int)) else { return; };
        let uid = ev.field("_userid").and_then(|u| u.field("m_userId")).and_then(Value::as_int);
        let Some(p) = uid.and_then(|u| user_to_player.get(&u).copied()) else { return; };
        let t = loop_to_sec(field_int(ev, "_gameloop").unwrap_or(0));
        let (nx, ny) = norm_world(x, y);
        if (0.0..=1.0).contains(&nx) && (0.0..=1.0).contains(&ny) {
            cmd_samples.entry(p).or_default().push(Sample { t, x: q3(nx), y: q3(ny), exact: false });
        }
    })?;

    // 5) merge samples (exact + command), sort by increasing t, quantise to 3 decimals (q3),
    //    dedup near-motionless points (delta < EPS from the previous kept one) to hold the
    //    payload budget (hundreds of KB). See Step 7 for the tested assembly.
    // 6) life intervals from Born(spawn)/Died/Revived of the player's hero unit
    // 7) deaths from SUnitDiedEvent of the tracked hero units
    //    x,y = norm_tile(m_x,m_y); victim = unit_player[m_unitTagIndex].0;
    //    killer = m_killerPlayerId if ∈1..10 otherwise None
    // 8) heroes sorted by player_id; warnings = vec![] (US-7 later)

    // … (assemble ViewerModel; see Steps 6-9 for the tested life & deaths)
    todo!("assemble after Steps 6-9")
}

fn event_name(e: &Value) -> String {
    e.field("_event").and_then(Value::as_str_lossy)
        .and_then(|s| s.rsplit('.').next().map(str::to_string)).unwrap_or_default()
}
fn field_int(e: &Value, k: &str) -> Option<i64> { e.field(k).and_then(Value::as_int) }

fn map_size(tracker: &[Value]) -> Option<(f64, f64)> {
    for e in tracker {
        if event_name(e) != "SStatGameEvent" { continue; }
        if e.field("m_eventName").and_then(Value::as_str_lossy).as_deref() != Some("GameStart") { continue; }
        let fixed = e.field("m_fixedData").and_then(Value::as_array)?;
        let mut sx = None; let mut sy = None;
        for kv in fixed {
            let k = kv.field("m_key").and_then(Value::as_str_lossy);
            let v = kv.field("m_value").and_then(Value::as_int);
            match k.as_deref() { Some("MapSizeX") => sx = v, Some("MapSizeY") => sy = v, _ => {} }
        }
        return Some((sx? as f64, sy? as f64));
    }
    None
}

// Quantise to 3 decimals (stabilises the golden + reduces the payload).
fn q3(v: f64) -> f64 { (v * 1000.0).round() / 1000.0 }

// AUTHORITATIVE: m_userId (from the game events' `_userid`) → tracker m_playerId (1..=10),
// from SPlayerSetupEvent (m_type == 1 = player). Replaces the "slot+1" assumption.
fn user_to_player(tracker: &[Value]) -> HashMap<i64, i64> {
    let mut m = HashMap::new();
    for e in tracker {
        if event_name(e) != "SPlayerSetupEvent" { continue; }
        let (Some(uid), Some(pid)) = (field_int(e, "m_userId"), field_int(e, "m_playerId")) else { continue; };
        if (1..=10).contains(&pid) { m.insert(uid, pid); }
    }
    m
}

// playerId (1..=10) → toon_handle: SPlayerSetupEvent (m_playerId → m_slotId) crossed with
// details().players[*].working_set_slot_id → toon_handle. Used by the server (Chunk 2).
pub(crate) fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, crate::Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;
    let mut slot_to_toon: HashMap<i64, String> = HashMap::new();
    for p in &details.players {
        if let Some(slot) = p.working_set_slot_id { slot_to_toon.insert(slot, p.toon_handle.clone()); }
    }
    let mut out = Vec::new();
    for e in &tracker {
        if event_name(e) != "SPlayerSetupEvent" { continue; }
        let (Some(pid), Some(slot)) = (field_int(e, "m_playerId"), field_int(e, "m_slotId")) else { continue; };
        if let Some(toon) = slot_to_toon.get(&slot) {
            out.push((pid, toon.clone()));
        }
    }
    out.sort_by_key(|(p, _)| *p);
    Ok(out)
}
```
> Implementation notes:
> - Check the exact fields of `ReplayDetails`/`PlayerDetails` in `crates/storm-replay/src/lib.rs` (`players`, `working_set_slot_id`, `toon_handle`).
> - If `visit_game_events` provides an owned `Value` (by value), adapt the closure signature (`|ev: Value|` + `ev.field(...)`).
> - `SPlayerSetupEvent` was observed in a real replay: `{m_playerId, m_slotId, m_userId, m_type}`, 10 entries, `m_userId == m_slotId == m_playerId-1` here — but do **not** re-hardcode this equality, read the fields (it does not always hold, e.g. observers/rejoins).

- [ ] **Step 6: Write the life intervals test (fails)** — add to `tests/extract.rs`

```rust
#[test]
fn life_intervals_ordered_and_bounded() {
    let m = model();
    for h in &m.heroes {
        assert!(!h.life.is_empty(), "player {} without a life interval", h.player_id);
        for iv in &h.life { assert!(iv.from <= iv.to); }
        // strictly increasing, non-overlapping intervals
        for w in h.life.windows(2) { assert!(w[0].to <= w[1].from); }
    }
}
```

- [ ] **Step 7: Complete `build` — merge samples + life + deaths (replaces the `todo!()`)**

This step assembles everything and removes the Step 5 `todo!()`:
1. **Sample merge**: for each player, concatenate `samples[p]` (exact) + `cmd_samples[p]`, sort by increasing `t`. **Dedup**: ignore a sample whose `(|dx|,|dy|)` vs the last kept one is `< EPS` (`const EPS: f64 = 0.004;`) **and** with the same `exact` — always keeps the exact samples. Final clamp `[0,1]`.
2. **Life**: interval opened at the first Born of the player's hero unit (spawn), closed at the `t` of the `SUnitDiedEvent`, reopened at the `SUnitRevivedEvent` (resolve unit → player via `unit_player`). Close the last interval at `meta.duration_sec`. Intervals sorted, non-overlapping.
3. **Deaths**: one `Death` per `SUnitDiedEvent` whose `m_unitTagIndex` ∈ tracked hero units. `x,y = q3(norm_tile(m_x,m_y))`, `victim_player_id = unit_player[idx].0`, `killer_player_id = m_killerPlayerId` if ∈ 1..=10 otherwise `None`.
4. Assemble `ViewerModel { meta, heroes (sorted by player_id), deaths (sorted by t), warnings: vec![] }`.

- [ ] **Step 8: Golden JSON — write the test (fails)**

```rust
#[test]
fn golden_json_stable() {
    let m = model();
    // exclude viewerVersion from the comparison (deliberate bump → regenerate the golden)
    let mut v = serde_json::to_value(&m).unwrap();
    v["meta"]["viewerVersion"] = serde_json::json!("<ignored>");
    let got = serde_json::to_string_pretty(&v).unwrap();
    let path = "tests/data/silver-city-aram.golden.json";
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(path, &got).unwrap();
    }
    let want = std::fs::read_to_string(path).expect("golden manquant — lancer UPDATE_GOLDEN=1");
    assert_eq!(got.trim(), want.trim());
}
```

- [ ] **Step 9: Generate the golden + make the whole suite pass**

Run: `UPDATE_GOLDEN=1 cargo test -p storm-replay-viewer golden_json_stable` then `cargo test -p storm-replay-viewer`
Expected: PASS (all). Then `cargo clippy -p storm-replay-viewer -- -D warnings`.
Inspect the golden by eye: 10 heroes, samples increasing in `t`, plausible coords.

- [ ] **Step 10: Commit**
```bash
git add Cargo.toml crates/storm-replay-viewer
git commit -m "feat(viewer): storm-replay-viewer crate — positional extraction → normalised JSON model"
```

---

## Chunk 2 — Endpoint serveur `/api/matches/{id}/replay2d`

**Responsibility:** decode the archived replay on the fly (+ disk cache), call `build_model`, merge the player metadata (name/hero/team/win) from Postgres by `playerId`, emit the final JSON.

**Files:**
- Modify: `crates/storm-codex-server/Cargo.toml` (dep `storm-replay-viewer`)
- Create: `crates/storm-codex-server/src/replay2d.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (`mod replay2d;` + route)

**Decisions:**
- Route: `GET /api/matches/{id}/replay2d` (consistent with `/api/matches/{id}/raw`).
- Disk cache: reuse `s.cfg.raw_cache_dir`, file `{id}-replay2d-v{VIEWER_VERSION}.json`, same `filetime_now`/`enforce_lru` as `raw.rs` (factor out if trivial, otherwise duplicate the pattern — light DRY, do not over-abstract).
- Player metadata: the geometry is keyed by tracker `playerId` (1..=10). The server maps
  `playerId → toon_handle` via the crate's **authoritative** helper `player_toons()` (crosses
  `SPlayerSetupEvent.m_slotId` with `details.working_set_slot_id`) — **NOT** "details index+1". It then
  joins `match_players` by `toon_handle` and emits `players:[{playerId, name, hero, team, win}]`. The
  universe colour and the portrait are derived **on the front** (`universeColor(hero)`, `heroIcon(hero)`),
  so **not** included in the payload.

- [ ] **Step 1: Write a server integration test (fails)** — `crates/storm-codex-server/tests/replay2d.rs` if a server test harness exists; otherwise, a documented manual test (see Step 5). First check for a test pattern in `crates/storm-codex-server/tests/`. If there is none, skip the automated server test (covered by the crate in Chunk 1 + the Chunk 4 E2E check) and tick this step as "N/A — no server harness".

- [ ] **Step 2: Implement `replay2d.rs`**

Structure (mirror of `raw.rs`) — the `todo!()` is replaced by the implementation of items 1→6:
```rust
use crate::AppState;
use axum::{extract::{Path, State}, http::{header, StatusCode}, response::{IntoResponse, Response}};
use std::collections::HashMap;
use std::path::PathBuf;
use storm_replay::Replay;
use storm_replay_viewer::{build_model, player_toons, ViewerModel, VIEWER_VERSION};

pub async fn get_replay2d(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    // 1) archived_path: same query as raw.rs (uploads WHERE match_id=$1 AND status='parsed').
    // 2) cache: raw_cache_dir/{id}-replay2d-v{VIEWER_VERSION}.json → hit: filetime_now + serve.
    // 3) miss: spawn_blocking that returns BOTH the geometry AND the playerId→toon mapping
    //    (the Replay is consumed in the closure, so we take out everything the async part needs):
    //      let r = Replay::open(&archived)?;
    //      let model = build_model(&r)?;              // meta/heroes/deaths/warnings
    //      let toons = player_toons(&r)?;             // Vec<(playerId, toon_handle)>
    //      (model, toons)
    // 4) async match_players query → HashMap<toon, (name,hero,team,win)>:
    //      SELECT toon_handle, name, hero, team, win FROM match_players WHERE match_id = $1
    // 5) players[] = for each (playerId, toon) of `toons`, join the HashMap metadata:
    //      { playerId, name, hero, team, win }  (universeColor/portrait derived on the front)
    // 6) final JSON = merge of the model + players via serde_json (see the helper below),
    //    serialise, write cache, enforce_lru (raw.rs pattern), serve.
    todo!("implement items 1→6")
}
```
Details:
- **Player order/identity = authoritative**, NOT "details index+1". The crate provides `player_toons()`
  (tracker playerId → toon via `SPlayerSetupEvent.m_slotId` × `details.working_set_slot_id`). The server
  joins `match_players` by `toon_handle`.
- `build_model` + `player_toons` run in the **same** `spawn_blocking` (a single decode, ~130 ms);
  the `match_players` query stays async, outside the blocking part.
- **JSON merge**: `ViewerModel` serialises `{meta, heroes, deaths, warnings}`; the server inserts
  `players` into it. Helper:
  ```rust
  fn merge(model: &ViewerModel, players: serde_json::Value) -> serde_json::Value {
      let mut v = serde_json::to_value(model).unwrap_or_default();
      if let Some(o) = v.as_object_mut() { o.insert("players".into(), players); }
      v
  }
  ```
- JSON final servi = `{ meta, players, heroes, deaths, warnings }`.

- [ ] **Step 3: Wire the route** — `main.rs`
```rust
mod replay2d;
// … in the Router:
.route("/api/matches/{id}/replay2d", get(replay2d::get_replay2d))
```
Add the dep in `crates/storm-codex-server/Cargo.toml`:
```toml
storm-replay-viewer = { path = "../storm-replay-viewer" }
```

- [ ] **Step 4: Compile + clippy**
Run: `cargo build -p storm-codex-server && cargo clippy -p storm-codex-server -- -D warnings`
Expected: OK.

- [ ] **Step 5: Manual check against the backfilled DB** (the box runs in the evening, or local Docker Postgres `docker-compose.dev.yml`)
```bash
# get an existing match id
curl -s localhost:5102/api/matches | python3 -c "import sys,json;print(json.load(sys.stdin)[0]['id'])"
curl -s localhost:5102/api/matches/<ID>/replay2d | python3 -m json.tool | head -40
```
Expected: correct `meta.mapName`, `players` 10 entries with `hero`, `heroes` 10 tracks, non-empty `deaths`. Second call instant (cache hit — check the file in `raw_cache_dir`).

- [ ] **Step 6: Commit**
```bash
git add crates/storm-codex-server
git commit -m "feat(server): /api/matches/{id}/replay2d endpoint (on-demand decoding + cache + player meta)"
```

---

## Chunk 3 — Onglet front « Replay 2D » (canvas + scrub)

**Responsibility:** load the model, show the minimap, 10 hero markers, scrub bar, client `seek(t)` (lerp + life), dimming of the dead + death markers. Tab in `MatchDetail`.

**Files:**
- Modify: `web/package.json` (+ vitest); `web/vite.config.ts` (test config)
- Create: `web/src/replay2d.ts` (types + pure `seek(t)` + `sampleAt`)
- Create: `web/src/replay2d.test.ts` (vitest)
- Create: `web/src/components/Replay2D.tsx` (canvas + scrub)
- Modify: `web/src/api.ts` (`fetchReplay2d` + types)
- Modify: `web/src/pages/MatchDetail.tsx` (tab)

- [ ] **Step 1: Add vitest** — `web/package.json`
```json
"scripts": { "dev": "vite", "build": "tsc -b && vite build", "preview": "vite preview", "test": "vitest run" },
"devDependencies": { "vitest": "^2.1.0", ...existant }
```
`web/vite.config.ts`: add `test: { environment: 'node' }` (import from `vitest/config` if needed). Then `cd web && npm install`.

- [ ] **Step 2: Write the `seek` test (fails)** — `web/src/replay2d.test.ts`
```ts
import { describe, it, expect } from "vitest";
import { sampleAt, type HeroTrack } from "./replay2d";

const h: HeroTrack = {
  playerId: 1,
  samples: [ {t:0,x:0,y:0,exact:true}, {t:10,x:1,y:1,exact:false} ],
  life: [ {from:0,to:6}, {from:8,to:10} ],
};

describe("sampleAt", () => {
  it("lerps between two alive samples", () => {
    const p = sampleAt(h, 5); // vivant (0..6)
    expect(p).not.toBeNull();
    expect(p!.x).toBeCloseTo(0.5); expect(p!.alive).toBe(true);
  });
  it("freezes the position during the dead interval (no lerp across)", () => {
    const p = sampleAt(h, 7); // mort (6..8)
    expect(p!.alive).toBe(false);
    expect(p!.x).toBeCloseTo(0.6); // last alive position at t=6, not 0.7
  });
  it("clamps before the first / after the last sample", () => {
    expect(sampleAt(h, -1)!.x).toBeCloseTo(0);
    expect(sampleAt(h, 99)!.x).toBeCloseTo(1);
  });
});
```

- [ ] **Step 3: Check the failure**
Run: `cd web && npm run test`
Expected: FAIL (`sampleAt` not found).

- [ ] **Step 4: Implement `replay2d.ts`**
```ts
export interface Sample { t: number; x: number; y: number; exact: boolean }
export interface Interval { from: number; to: number }
export interface HeroTrack { playerId: number; samples: Sample[]; life: Interval[] }
export interface PlayerMeta { playerId: number; name: string | null; hero: string | null; team: number | null; win: boolean | null }
export interface Death { t: number; x: number; y: number; victimPlayerId: number; killerPlayerId: number | null }
export interface Replay2D {
  meta: { mapName: string; mapSize: [number, number]; durationSec: number; loopOffset: number; viewerVersion: number };
  players: PlayerMeta[]; heroes: HeroTrack[]; deaths: Death[]; warnings: string[];
}

const aliveAt = (life: Interval[], t: number) => life.some((iv) => t >= iv.from && t <= iv.to);

/** Most recent end of life before/at t (= instant of the death). null if none (before spawn). */
function lastAliveEnd(life: Interval[], t: number): number | null {
  let best: number | null = null;
  for (const iv of life) if (iv.to <= t && (best === null || iv.to > best)) best = iv.to;
  return best;
}

/** Linear interpolation of the position at instant `t` (clamped to the 1st/last sample). */
function interp(s: Sample[], t: number): { x: number; y: number } {
  const hi0 = s.length - 1;
  if (t <= s[0].t) return { x: s[0].x, y: s[0].y };
  if (t >= s[hi0].t) return { x: s[hi0].x, y: s[hi0].y };
  let lo = 0, hi = hi0, i = 0;
  while (lo <= hi) { const mid = (lo + hi) >> 1; if (s[mid].t <= t) { i = mid; lo = mid + 1; } else hi = mid - 1; }
  const a = s[i], b = s[i + 1] ?? a;
  const f = b.t === a.t ? 0 : (t - a.t) / (b.t - a.t);
  return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
}

/** Position + alive/dead state of a hero at instant t (pure). null if no samples.
 *  Dead: the position is FROZEN at the instant of the death (end of the last alive interval) — no
 *  lerp across the dead→respawn gap (the respawn is at the base, far from the place of death). */
export function sampleAt(h: HeroTrack, t: number): { x: number; y: number; alive: boolean } | null {
  const s = h.samples;
  if (!s.length) return null;
  const alive = aliveAt(h.life, t);
  const et = alive ? t : (lastAliveEnd(h.life, t) ?? t); // effective time: dead → instant of death
  const p = interp(s, et);
  return { x: p.x, y: p.y, alive };
}

/** "Recent" deaths at t (marker that persists for ~4 s of scrub). */
export function deathsNear(deaths: Death[], t: number, window = 4): Death[] {
  return deaths.filter((d) => t >= d.t && t <= d.t + window);
}
```

- [ ] **Step 5: Check that the tests pass**
Run: `cd web && npm run test`
Expected: PASS.

- [ ] **Step 6: Add `fetchReplay2d`** — `web/src/api.ts`
```ts
import type { Replay2D } from "./replay2d";
export const fetchReplay2d = (id: string | number) => get<Replay2D>(`/api/matches/${id}/replay2d`);
```
(reuses the existing `get<T>`.)

- [ ] **Step 7: Canvas component** — `web/src/components/Replay2D.tsx`

Elements:
- `useQuery(["replay2d", id], () => fetchReplay2d(id))`.
- State `t` (seconds) driven by an `<input type="range" min=0 max=durationSec step=0.1>`.
- Background: `<img>` via `mapImage(meta.mapName)`; if `null`, gradient background (existing class). Draw the image in the canvas (or set it as a CSS background under the transparent canvas).
- Draw loop in a `useEffect([t, data])`: for each hero, `sampleAt(track, t)` → px `cx = x*W`, `cy = (1 - y)*H` (**Y flip**: the world goes up, the canvas goes down). Marker filled with the team colour (team 0 = blue, team 1 = red — reuse the existing team tokens/classes), ring `universeColor(hero)`, portrait `heroIcon(hero)` clipped to a circle (initials fallback). Dead hero → reduced `globalAlpha` + cross.
- Death markers: `deathsNear(deaths, t)` → small ✕ at `(x, 1-y)`.
- Player legend (name + hero) alongside, team colour.

**2D canvas** rendering (not SVG). Resizing: responsive square canvas (container max-width), map coords ×W/H.

> ⚠️ **Y flip**: if the heroes appear vertically inverted during the visual check (Chunk 4), that is the sign of the flip — adjust `cy = y*H` vs `(1-y)*H` once.

- [ ] **Step 8: Integrate the tab** — `web/src/pages/MatchDetail.tsx`
- `const [tab, setTab] = useState<"score" | "replay2d">("score")` (the file already imports `useState`).
- Two tab buttons (existing `bdg`/card classes): "Score" / "Replay 2D".
- `{tab === "replay2d" ? <Replay2D id={id} /> : <…existing score…>}`. Do not break the current score display.

- [ ] **Step 9: Front build**
Run: `cd web && npm run build`
Expected: `✓ built` (⚠️ a `tsc` failure fails silently — check the `✓ built` line, see STATUS.md).

- [ ] **Step 10: Commit**
```bash
git add web
git commit -m "feat(web): Replay 2D tab — canvas, scrub, client seek(t), alive/dead + death markers"
```

---

## Chunk 4 — E2E visual check + minimap calibration

**Responsibility:** prove on a real match that the heroes are placed correctly on the right map and that the scrub is instant; fix orientation/cropping if needed. **MVP-1 acceptance criterion.**

- [ ] **Step 1: Start the stack** (backfilled Postgres + server + front). Use the `preview_*` tools (never Bash for the server). Create `.claude/launch.json` if missing (Rust server + `WEB_DIR`, or `vite` proxy to the API). Open a real match → "Replay 2D" tab.

- [ ] **Step 2: Visual checks** (preview_screenshot + preview_console_logs + preview_network):
  - The right minimap shows (or the gradient fallback for ARAM maps without an image — expected).
  - 10 visible markers, split into 2 teams, within the map bounds.
  - Scrub from 0 → end: the markers move coherently; at a known hero's death, the marker dims and a ✕ appears at the right place.
  - Instant seek (no perceptible latency; `preview_network`: a single GET `/replay2d`, no request per cursor move).

- [ ] **Step 3: Fix the calibration if needed**
  - Vertical inversion → toggle the Y flip (Chunk 3 Step 7).
  - Heroes offset/squashed at the edge → `MapSize` includes an unplayable border: add a per-map cropping constant (margin as a fraction) applied to the normalisation in the crate or to the drawing in the front. Document the value. Do **not** over-tune: aim for "clearly readable", not pixel-perfect.

- [ ] **Step 4: Non-regression check**: the existing "Score" tab still works; the other pages unchanged.

- [ ] **Step 5: Update `docs/STATUS.md`** — add a section "2D viewer (MVP-1): shipped + verified" (date, what works, screenshots, HP/mana explicitly out of scope, remaining fast-follows: play/pause animation, structures, kill-feed).

- [ ] **Step 6: Final commit + propose branch finalisation**
```bash
git add docs/STATUS.md
git commit -m "docs(status): 2D viewer MVP-1 shipped + verified E2E"
```
Then invoke superpowers:finishing-a-development-branch (merge/PR).

---

## Notes transverses
- **DRY / YAGNI:** no snapshots, no animation, no structures/kill-feed in MVP-1 (fast-follows). The cache reuses the `raw.rs` pattern, do not create a 4th tier.
- **Perf:** compute budget = one parse (~130 ms) on 1st access, then cache. Seek < 50 ms guaranteed (100% client). Payload: keep only the movement `TargetPoint`s + quantise the floats (~3 decimals) to stay in hundreds of KB.
- **Reuse:** hero names/portraits/colours come from the Postgres projection (ARAM shuffle correction included) and the front helpers — the viewer never re-resolves hero identity.
- **Main risk (calibration):** de-risked from Chunk 1 (test `all_coords_normalized`) and confirmed visually in Chunk 4.
