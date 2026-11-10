# HotS draft simulator — Implementation plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A manually driven HotS draft simulator (3 formats), with a live OBS "Timeline" overlay, in storm-codex.

**Architecture:** Draft engine in **pure Rust, TDD** (format = data list of steps); authoritative state on the server side (`Arc<RwLock>` + Postgres persistence) mutated by REST routes and broadcast via the existing WS broadcast; two React pages (console `/draft` + overlay `/draft/overlay`).

**Tech Stack:** Rust (axum, sqlx, tokio, serde), Postgres, React/Vite/TS, react-router, @tanstack/react-query.

Spec: `docs/2026-06-19-draft-simulator-design.md`. Mockups: `docs/draft-aesthetics.html`, `docs/draft-control-mockup.html`.

**Repo conventions (checked):**
- Binary crate `crates/storm-codex-server` (no lib) → **inline** unit tests `#[cfg(test)]`, run by `cargo test -p storm-codex-server`.
- Routes added in the `Router` of `crates/storm-codex-server/src/main.rs`, handlers as `async fn(State(AppState), …)`.
- WS: `state.events.send(serde_json::json!({ "type": "…" }))` (see `patch.new` in main.rs; `src/ws.rs` relays as is).
- Migrations: `crates/storm-codex-server/migrations/NNNN_name.sql` (next = `0008`).
- Hero reference data already served: `GET /api/dim/heroes` → `Record<name, { universe, role, icon }>` (see `web/src/api.ts` `useDimHeroes`).
- OBS overlays = React routes **outside `<Layout>`** (see `web/src/App.tsx`, e.g. `now-playing`).

---

## Chunk 1: Draft engine (pure Rust, TDD)

**Responsibility:** types + state machine, without any I/O. All the business risk lives here and is covered by tests.

### Task 1: Types + module

**Files:**
- Create: `crates/storm-codex-server/src/draft/mod.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (add `mod draft;` after `mod dim;`)

- [ ] **Step 1: Declare the module.** In `main.rs`, add `mod draft;` to the list of `mod`s.

- [ ] **Step 2: Write the types** in `src/draft/mod.rs`:

```rust
//! HotS draft engine — pure (no I/O). A format = an ordered list of steps;
//! the state machine "walks" over it. Serialisable for persistence + WS.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA_VERSION: i32 = 1;

// Visual side (colour/position) — INDEPENDENT of the draft order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side { Blue, Red }
impl Side { pub fn other(self) -> Side { match self { Side::Blue => Side::Red, Side::Red => Side::Blue } } }

// Order role — who drafts first. The `first_pick: Side` setting maps First→a side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order { First, Second }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action { Ban, Pick }

// A step is defined by order role (not by colour): this is what decouples first-pick and side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Step { pub order: Order, pub action: Action }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format { Standard, Normal, Fearless }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftError { Unavailable, Complete, NothingToUndo }
```

- [ ] **Step 3: Compile check.** Run: `cargo build -p storm-codex-server`. Expected: PASS ("unused" warnings tolerated).

- [ ] **Step 4: Commit.** `git add -A && git commit -m "feat(draft): draft engine types"`

### Task 2: Format sequences (data)

**Files:**
- Create: `crates/storm-codex-server/src/draft/formats.rs`
- Modify: `crates/storm-codex-server/src/draft/mod.rs` (`mod formats; pub use formats::steps_for;`)

> Standard sequence **confirmed by the operator** (HotS, first-pick = First). It is data: fixing
> it = editing this list. Written by **order role** (First/Second), so independent of the
> side: `first_pick: Side` maps First→a side at render time (see Task 3).

- [ ] **Step 1: Test of the sequence** (inline in `formats.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{Action, Format, Order, Step};

    fn counts(steps: &[Step]) -> (usize, usize, usize, usize) {
        let bf = steps.iter().filter(|s| s.action==Action::Ban  && s.order==Order::First ).count();
        let bs = steps.iter().filter(|s| s.action==Action::Ban  && s.order==Order::Second).count();
        let pf = steps.iter().filter(|s| s.action==Action::Pick && s.order==Order::First ).count();
        let ps = steps.iter().filter(|s| s.action==Action::Pick && s.order==Order::Second).count();
        (bf, bs, pf, ps)
    }

    #[test]
    fn standard_is_3_bans_5_picks_per_role_len_16() {
        let s = steps_for(Format::Standard);
        assert_eq!(counts(&s), (3, 3, 5, 5));
        assert_eq!(s.len(), 16);
    }

    #[test]
    fn standard_exact_order() {
        use Action::*; use Order::*;
        let s = steps_for(Format::Standard);
        let got: Vec<(Order, Action)> = s.iter().map(|x| (x.order, x.action)).collect();
        assert_eq!(got, vec![
            (First,Ban),(Second,Ban),(First,Ban),(Second,Ban),
            (First,Pick),(Second,Pick),(Second,Pick),(First,Pick),(First,Pick),
            (Second,Ban),(First,Ban),
            (Second,Pick),(Second,Pick),(First,Pick),(First,Pick),(Second,Pick),
        ]);
    }

    #[test]
    fn normal_has_no_bans_10_picks() {
        let s = steps_for(Format::Normal);
        assert_eq!(counts(&s), (0, 0, 5, 5));
        assert_eq!(s.len(), 10);
    }

    #[test]
    fn fearless_sequence_equals_standard() {
        assert_eq!(steps_for(Format::Fearless), steps_for(Format::Standard));
    }
}
```

- [ ] **Step 2: Run, check the failure.** Run: `cargo test -p storm-codex-server formats`. Expected: FAIL (`steps_for` not found).

- [ ] **Step 3: Implement** `formats.rs`:

```rust
use crate::draft::{Action, Format, Order, Step};
use Action::{Ban, Pick};
use Order::{First, Second};

// HotS Standard sequence (confirmed). By order role; First = first-pick team.
// 1 ban F · 1 ban S · 1 ban F · 1 ban S · 1 pick F · 2 pick S · 2 pick F ·
// 1 ban S · 1 ban F · 2 pick S · 2 pick F · 1 pick S.
const STANDARD: [(Order, Action); 16] = [
    (First, Ban), (Second, Ban), (First, Ban), (Second, Ban),
    (First, Pick), (Second, Pick), (Second, Pick), (First, Pick), (First, Pick),
    (Second, Ban), (First, Ban),
    (Second, Pick), (Second, Pick), (First, Pick), (First, Pick), (Second, Pick),
];

pub fn steps_for(format: Format) -> Vec<Step> {
    STANDARD.iter()
        .filter(|(_, a)| format != Format::Normal || *a == Pick)
        .map(|&(order, action)| Step { order, action })
        .collect()
}
```

- [ ] **Step 4: Run, check the success.** Run: `cargo test -p storm-codex-server formats`. Expected: PASS.

- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): format sequences (standard/normal/fearless)"`

### Task 3: State machine (apply / undo / reset / legality)

**Files:**
- Modify: `crates/storm-codex-server/src/draft/mod.rs`

- [ ] **Step 1: Write the tests** (inline in `mod.rs`) covering: complete walk of the Standard; refusal of an already taken hero (`Unavailable`); refusal when finished (`Complete`); undo frees the last one; reset sets back to zero keeping the config; fearless seed makes unavailable; manual override; `current_step`/`active_team`/`current_phase`. Representative example:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn new_standard() -> DraftState {
        DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into())
    }

    #[test]
    fn walks_full_standard_then_completes() {
        let mut d = new_standard();
        for i in 0..16 { d.apply(&format!("hero{i}")).unwrap(); }
        assert!(d.is_complete());
        assert_eq!(d.apply("late"), Err(DraftError::Complete));
    }

    #[test]
    fn rejects_already_used_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        assert_eq!(d.apply("Muradin"), Err(DraftError::Unavailable));
    }

    #[test]
    fn undo_frees_last_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        d.undo().unwrap();
        assert_eq!(d.cursor, 0);
        d.apply("Muradin").unwrap(); // available again
    }

    #[test]
    fn fearless_seed_blocks_series_heroes() {
        let mut d = DraftState::new(Format::Fearless, Side::Blue, "Sky Temple".into());
        d.seed_series(&["Jaina".into()]);
        assert_eq!(d.apply("Jaina"), Err(DraftError::Unavailable));
    }

    #[test]
    fn manual_override_toggles_availability() {
        let mut d = new_standard();
        d.set_unavailable("Abathur", true);
        assert_eq!(d.apply("Abathur"), Err(DraftError::Unavailable));
        d.set_unavailable("Abathur", false);
        d.apply("Abathur").unwrap();
    }

    #[test]
    fn first_pick_is_independent_of_side() {
        // first_pick = Red: the First role is played by the red SIDE (the 1st step is "red side").
        let d = DraftState::new(Format::Standard, Side::Red, "Sky Temple".into());
        assert_eq!(d.active_side(), Some(Side::Red));
        let d2 = DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into());
        assert_eq!(d2.active_side(), Some(Side::Blue));
    }

    #[test]
    fn current_phase_len_groups_consecutive_same_order() {
        let mut d = new_standard();
        assert_eq!(d.current_phase_len(), 1); // step 1 = First ban, isolated
        for i in 0..5 { d.apply(&format!("h{i}")).unwrap(); } // curseur = 5
        assert_eq!(d.current_phase_len(), 2); // steps 6-7 = 2 consecutive Second picks
    }
}
```

- [ ] **Step 2: Run, check the failure.** Run: `cargo test -p storm-codex-server draft`. Expected: FAIL (`DraftState` not found).

- [ ] **Step 3: Implement** `DraftState` in `mod.rs`:

```rust
mod formats;
pub use formats::steps_for;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamInfo { pub name: String, pub players: [String; 5] }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftState {
    pub schema_version: i32,
    pub format: Format,
    pub first_pick: Side,                 // which SIDE plays the First role — setting INDEPENDENT of the naming
    pub map: String,
    pub blue: TeamInfo,                    // blue-side team (fixed colour/position)
    pub red: TeamInfo,                     // red-side team
    pub score: [u8; 2],                   // [blue, red]
    pub bo: u8,
    pub steps: Vec<Step>,                 // by order role (First/Second), independent of the side
    pub cursor: usize,
    pub assignments: Vec<Option<String>>, // parallel to steps: hero assigned to step i
    pub manual_unavailable: BTreeSet<String>,
    pub series_bans: BTreeSet<String>,    // fearless: heroes picked in previous games
}

impl DraftState {
    pub fn new(format: Format, first_pick: Side, map: String) -> Self {
        let steps = steps_for(format);
        let n = steps.len();
        Self {
            schema_version: SCHEMA_VERSION, format, first_pick, map,
            blue: TeamInfo { name: "Blue".into(), players: Default::default() },
            red:  TeamInfo { name: "Red".into(),  players: Default::default() },
            score: [0, 0], bo: 5, steps, cursor: 0,
            assignments: vec![None; n],
            manual_unavailable: BTreeSet::new(),
            series_bans: BTreeSet::new(),
        }
    }

    /// Resolves an order role to a visual side according to the `first_pick` setting (the decoupling).
    pub fn side_of(&self, order: Order) -> Side {
        if order == Order::First { self.first_pick } else { self.first_pick.other() }
    }
    pub fn current_step(&self) -> Option<Step> { self.steps.get(self.cursor).copied() }
    pub fn active_side(&self) -> Option<Side> { self.current_step().map(|s| self.side_of(s.order)) }
    pub fn is_complete(&self) -> bool { self.cursor >= self.steps.len() }

    fn is_used(&self, hero: &str) -> bool {
        self.assignments.iter().flatten().any(|h| h == hero)
    }
    pub fn is_unavailable(&self, hero: &str) -> bool {
        self.manual_unavailable.contains(hero) || self.series_bans.contains(hero) || self.is_used(hero)
    }

    pub fn apply(&mut self, hero: &str) -> Result<(), DraftError> {
        if self.is_complete() { return Err(DraftError::Complete); }
        if self.is_unavailable(hero) { return Err(DraftError::Unavailable); }
        self.assignments[self.cursor] = Some(hero.to_string());
        self.cursor += 1;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), DraftError> {
        if self.cursor == 0 { return Err(DraftError::NothingToUndo); }
        self.cursor -= 1;
        self.assignments[self.cursor] = None;
        Ok(())
    }
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.assignments = vec![None; self.steps.len()];
    }
    pub fn set_unavailable(&mut self, hero: &str, value: bool) {
        if value { self.manual_unavailable.insert(hero.to_string()); }
        else { self.manual_unavailable.remove(hero); }
    }
    pub fn seed_series(&mut self, heroes: &[String]) {
        self.series_bans = heroes.iter().cloned().collect();
    }
    /// Length of the block of consecutive steps of the same order role (= same side) from the cursor,
    /// for the **per-phase** timer (the countdown covers the whole block, not each pick).
    pub fn current_phase_len(&self) -> usize {
        let Some(step) = self.current_step() else { return 0 };
        self.steps[self.cursor..].iter().take_while(|s| s.order == step.order).count()
    }
}
```

- [ ] **Step 4: Run, check the success.** Run: `cargo test -p storm-codex-server draft`. Expected: PASS. Complete the missing tests (phase, reset keeps the config) until green.

- [ ] **Step 5: Clippy.** Run: `cargo clippy -p storm-codex-server -- -D warnings`. Expected: PASS.

- [ ] **Step 6: Commit.** `git add -A && git commit -m "feat(draft): state machine (apply/undo/reset/legality/fearless)"`

---

## Chunk 2: Server state, persistence & API

**Responsibility:** expose the engine via REST + WS, with a persistent shared state.

### Task 4: Migration (draft + series persistence)

**Files:**
- Create: `crates/storm-codex-server/migrations/0008_draft.sql`

- [ ] **Step 1: Write the migration:**

```sql
-- "Live" draft state (singleton: a single row id=1) + series history (fearless).
CREATE TABLE draft_live (
    id          INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state       JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE draft_series (
    id          BIGSERIAL PRIMARY KEY,
    format      TEXT NOT NULL,
    bo          INT  NOT NULL,
    team_names  JSONB NOT NULL,
    score       JSONB NOT NULL,
    games       JSONB NOT NULL DEFAULT '[]',  -- [[hero,…] per finished game]
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- [ ] **Step 2: Check it applies** at startup (automatic migrations, see `main.rs`). Run: `cargo build -p storm-codex-server` then start the server locally (or `sqlx migrate run`). Expected: migration `0008` applied without error.

- [ ] **Step 3: Commit.** `git add -A && git commit -m "feat(draft): draft_live + draft_series migration"`

### Task 5: Shared state in AppState + loading

**Files:**
- Modify: `crates/storm-codex-server/src/main.rs` (`draft` field in `AppState`, init in `run`)
- Create: `crates/storm-codex-server/src/draft/store.rs` (Postgres `load`/`save`; `mod store;` in `draft/mod.rs`)

- [ ] **Step 1:** Add to `AppState`: `pub draft: std::sync::Arc<tokio::sync::RwLock<crate::draft::DraftState>>`.
- [ ] **Step 2:** In `run()`, after the migrations: load from `draft_live` (otherwise `DraftState::new(Format::Standard, Team::Blue, "Sky Temple")`) and build the `Arc<RwLock<…>>`.
- [ ] **Step 3:** `store.rs`: `async fn load(db) -> Option<DraftState>` (SELECT state FROM draft_live WHERE id=1) and `async fn save(db, &DraftState)` (INSERT … ON CONFLICT (id) DO UPDATE). Serialisation via `serde_json`.
- [ ] **Step 4: Build.** Run: `cargo build -p storm-codex-server`. Expected: PASS.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): shared state + persistence (load/save)"`

### Task 6: REST routes + WS broadcast

**Files:**
- Create: `crates/storm-codex-server/src/draft/api.rs` (handlers; `mod api;` in `draft/mod.rs`)
- Modify: `crates/storm-codex-server/src/main.rs` (register the routes)

Routes (all mutate the state under a write lock, persist via `store::save`, then `state.events.send(json!({"type":"draft.updated"}))`):
- `GET  /api/draft` → the current state (JSON).
- `POST /api/draft/config` `{format, map, first_pick (side "blue"|"red"), blue:{name,players[5]}, red:{name,players[5]}, bo}` → recreates the state. `first_pick` is **independent** of the blue/red naming.
- `POST /api/draft/action` `{hero}` → `apply`; 409 if `Unavailable`/`Complete`.
- `POST /api/draft/undo`, `POST /api/draft/reset`.
- `POST /api/draft/unavailable` `{hero, value}` → `set_unavailable`.
- `POST /api/draft/score` `{blue, red}`.
- `POST /api/draft/series/next` → pushes the current game's picks into `draft_series.games`, recreates a pre-filled draft (`seed_series` = union of the games); `POST /api/draft/series/new` → empties the series.

- [ ] **Step 1:** Implement `api.rs` (one handler per route; helper `broadcast_and_save(state)`).
- [ ] **Step 2:** Register the routes in the `Router` of `main.rs` (`.route("/api/draft", …)` block etc.).
- [ ] **Step 3: Build + clippy.** Run: `cargo build -p storm-codex-server && cargo clippy -p storm-codex-server -- -D warnings`. Expected: PASS.
- [ ] **Step 4: Manual test** (local server): `curl -s localhost:PORT/api/draft | jq` then `curl -XPOST …/api/draft/action -d '{"hero":"Muradin"}'` → the state advances; a 2nd `/ws` tab receives `draft.updated`.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): REST routes + WS broadcast draft.updated"`

---

## Chunk 3: Web — API layer + `/draft` console

### Task 7: Typed API layer

**Files:**
- Modify: `web/src/api.ts` (types `DraftState`, helpers `fetchDraft`, `draftAction`, etc.; hook `useDraft` that refetches on `draft.updated` via the existing WS `useLiveEvents`/equivalent)

- [ ] **Step 1:** Types mirroring the server JSON (`Team`, `Action`, `Step`, `DraftState`).
- [ ] **Step 2:** `useDraft()`: `useQuery(["draft"], fetchDraft)` + invalidation on the WS event `draft.updated` (extend the WS handler of `api.ts` that already handles `match.parsed`/`patch.new`).
- [ ] **Step 3 :** POST helpers (`draftAction(hero)`, `draftUndo()`, `draftConfig(...)`, …).
- [ ] **Step 4: Web build.** Run: `cd web && npm run build`. Expected: PASS (strict tsc).
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): web API layer + useDraft hook (live WS)"`

### Task 8: `/draft` console page

**Files:**
- Create: `web/src/pages/Draft.tsx`
- Modify: `web/src/App.tsx` (route `<Route path="draft" element={<Draft/>} />` **inside** `<Layout>`)

Follow `docs/draft-control-mockup.html` faithfully: config bar (format/map/first-pick/Bo/score + Undo/Reset/Next game/New series), current phase banner + timer, series bans ribbon (fearless), Blue/Red columns (team name + 5 editable nicknames + bans + highlighted current slot), central picker (search + role tabs from `dim_heroes.role` + grid; unavailable greyed out; click = `draftAction`). Reuse `Avatar`, `useDimHeroes`.

- [ ] **Step 1:** Implement the page (read via `useDraft`, writes via the Task 7 helpers).
- [ ] **Step 2:** Register the route.
- [ ] **Step 3: Web build.** Run: `cd web && npm run build`. Expected: PASS.
- [ ] **Step 4: Preview check**: start the server + front-end, open `/draft`, draft a complete Standard by clicking; check step highlight + unavailable + undo. (Preview workflow, no manual request to the user.)
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): /draft control console"`

---

## Chunk 4: Web — `/draft/overlay` Timeline overlay

### Task 9: Skins (CSS variables)

**Files:**
- Create: `web/src/pages/draft-overlay.css` (the 4 skins = `[data-skin=…]` blocks; copy the tokens from `docs/draft-aesthetics.html`)

- [ ] **Step 1:** Port the variables of the 4 skins (nexus/glass/tactical/mono) + load the web fonts (Oswald, JetBrains Mono, Cinzel) via `@import`/`<link>`.
- [ ] **Step 2: Commit.** `git add -A && git commit -m "feat(draft): overlay skins (4 CSS themes)"`

### Task 10: Timeline overlay

**Files:**
- Create: `web/src/pages/DraftOverlay.tsx`
- Modify: `web/src/App.tsx` (route `<Route path="draft/overlay" element={<DraftOverlay/>} />` **outside** `<Layout>`, like `now-playing`)

Port the **Timeline** view of `docs/draft-aesthetics.html`: header (map, names + blinking dot on `active_team`, score, phase timer), series bans ribbon (if fearless), picks in a chronological grid (columns = pick steps in order, blue top / red bottom, portrait + hero + nickname, active tile = gold glow), central ban strip. Data via `useDraft` (live WS). Skin via `?skin=` (default `nexus`). The timer resets when `active_team` changes (per-phase timer).

- [ ] **Step 1:** Implement the overlay (pure presentation from `DraftState`).
- [ ] **Step 2:** Register the route (outside Layout).
- [ ] **Step 3: Web build.** Run: `cd web && npm run build`. Expected: PASS.
- [ ] **Step 4: Preview check** (1920×1080): draft from `/draft` in one tab; `/draft/overlay` updates **live**; test `?skin=glass|tactical|mono`. Screenshot as evidence.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(draft): live Timeline overlay (4 skins)"`

---

## Acceptance criteria (spec reminder)

1. Engine: 3 sequences end to end, unavailable refusal, undo/reset — tests green (`cargo test -p storm-codex-server`).
2. `/draft` builds a Standard by clicking; `/draft/overlay` live via WS; `?skin=` changes the aesthetics.
3. Fearless: game 2 → heroes from game 1 unavailable (ribbon); manual override OK.
4. Team names + nicknames + score editable and reflected in the overlay.

## Out of scope (YAGNI)

Bot/AI, Jarvis suggestions, talents, multi-operator, hard time constraint, dedicated "player view" overlay.
