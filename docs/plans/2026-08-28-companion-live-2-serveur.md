# Companion live — Plan 2: server (lobby, builds, enrichment)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** expose on the server everything the companion page will need — ingest a live lobby,
enrich it with the archive's history, manage a build library, and link the finished
game to the lobby for the debrief.

**Architecture:** `storm-lobby` (plan 1, merged) provides the BattleTags and the teams. The server
resolves these BattleTags to application identities **against the archive itself**
(`match_players.name` + `data->>'tag'` → `toon_handle`), then aggregates the history in a single
`jsonb_build_object` query, as `read::get_player` already does. The current lobby state is a
persisted singleton, modelled on `draft_live`.

**Tech Stack:** Rust 2021, axum 0.8, sqlx 0.8 (runtime queries, no checked macros),
PostgreSQL, `tokio::sync::broadcast` for the WS.

## Scope

**This plan covers the server only.** The front (`/companion`, `/builds`) is plan 3; the
`client-rs` watcher (Hots-Overlay repo) is plan 4 and needs the gaming PC. Each route shipped
here is testable with curl without a single line of front.

## Global Constraints

- Rust 2021; workspace lints: `clippy::unwrap_used` = deny, `clippy::expect_used` = warn.
  No `unwrap()` outside tests; test files open with
  `#![allow(clippy::expect_used, clippy::unwrap_used)]`.
- **sqlx with runtime queries** (`sqlx::query`, `query_scalar`), never the compile-time checked
  macros — that is the choice already made everywhere in `crates/storm-codex-server`.
- Never any string interpolation in SQL: only `$1`, `$2`… binds.
- `storm-lobby` stays **pure**: no I/O, no runtime dependency on `storm-replay`/`storm-stats`.
- Budget: `/api/lobby` — `GET` p95 < 100 ms, `POST` p95 < 500 ms (criterion **amended on
  2026-08-28 after measurement**, see `docs/specs/2026-08-27-companion-live-design.md` §Criteria).
- Lobby state mutations broadcast on the existing `/ws` channel, like `draft.updated`.
- Conventional commits. Branch: `feat/companion-live-serveur`.
- **No change to the box** in this plan: everything is developed against the dev Postgres
  (`docker-compose.dev.yml`).

## Product decisions recorded by the operator (2026-08-28)

1. **Per-player team reassignment** (not a simple swap button): plan 1's measurement
   shows that the swap only fixes 5.3% of the wrong cases, against 94.7% of orders that carry
   no team information. The API must therefore allow setting the team **of each player**.
2. **`.s2ma hash → map` table derived from the archive**, to remove the manual entry of the
   map. That is task 1.

## Files

| File | Responsibility |
|---|---|
| `crates/storm-lobby/src/lib.rs` | + extraction of the `.s2ma` hashes, + map resolution via an embedded table |
| `crates/storm-lobby/src/maps.rs` | `hash → map name` table, generated, committed |
| `crates/storm-lobby/examples/derive_maps.rs` | derives `maps.rs` from a folder of replays |
| `crates/storm-codex-server/migrations/0009_companion.sql` | `builds`, `lobby_live` tables, resolution index |
| `crates/storm-codex-server/src/builds.rs` | library CRUD + import from a match |
| `crates/storm-codex-server/src/lobby/mod.rs` | enriched state type + singleton |
| `crates/storm-codex-server/src/lobby/store.rs` | `lobby_live` persistence (modelled on `draft/store.rs`) |
| `crates/storm-codex-server/src/lobby/enrich.rs` | BattleTag→identity resolution + archive aggregates |
| `crates/storm-codex-server/src/lobby/api.rs` | routes `/api/lobby*` |
| `crates/storm-codex-server/src/main.rs` | route declarations + `lobby` field in `AppState` |

---

## Task 1: Map inferred from the `.s2ma` hashes

**Files:**
- Modify: `crates/storm-lobby/src/lib.rs`
- Create: `crates/storm-lobby/src/maps.rs`
- Create: `crates/storm-lobby/examples/derive_maps.rs`
- Test: `crates/storm-lobby/tests/oracle.rs` (one test added)

**Interfaces:**
- Produces: `Lobby { players: Vec<LobbyPlayer>, map: Option<String> }` — the `map` field is **added**
  (`Lobby` is `#[non_exhaustive]`, so it is non-breaking). `map` carries the map name as
  `matches.map` stores it, or `None` if no known hash is found.
- Consumes: nothing new.

The format report (`docs/research/2026-08-27-lobby-format.md`, Q5) establishes that the blob contains
cache paths to `.s2ma` files identified by hash, but no map name. Each
archived replay carries both these hashes **and** its known map: the mapping can therefore be derived
from the archive, once and for all.

- [ ] **Step 1: Write the hash extraction test (red)**

Add to `crates/storm-lobby/tests/oracle.rs`:

```rust
/// The blob contains Battle.net cache paths to `.s2ma` files (the maps),
/// identified by a 32-character hexadecimal hash. They are the format's only map
/// clue (format report, Q5).
#[test]
fn les_hashes_de_carte_sont_extraits() {
    let path = data("crates/storm-stats/tests/data/silver-city-aram.StormReplay");
    let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
    let blob = replay.battlelobby_raw().expect("stream battlelobby");
    let hashes = storm_lobby::map_hashes(&blob);
    assert!(
        !hashes.is_empty(),
        "no .s2ma hash extracted — the format report documents 9"
    );
    assert!(
        hashes.iter().all(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit())),
        "malformed hash: {hashes:?}"
    );
}
```

- [ ] **Step 2: Run the test, check the failure**

Run: `cargo test -p storm-lobby --test oracle les_hashes`
Expected: FAIL at compile time — `map_hashes` does not exist.

- [ ] **Step 3: Implement the extraction**

In `crates/storm-lobby/src/lib.rs`, add:

```rust
/// Hashes of the `.s2ma` files (maps) referenced by the blob, in order of appearance,
/// deduplicated. The blob never names the map: these hashes are the only clue, and their
/// mapping to a name lives in [`maps`] (derived from the archive).
#[must_use]
pub fn map_hashes(bytes: &[u8]) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // Same pattern as `parse()`: the literal is checked by the tests, so a regex compile
    // failure is impossible in normal use — but no bare `unwrap()`.
    let re = RE.get_or_init(|| match regex::Regex::new(r"([0-9a-f]{32})\.s2ma") {
        Ok(r) => r,
        Err(e) => unreachable!("invalid .s2ma regex: {e}"),
    });
    let text = String::from_utf8_lossy(bytes);
    let mut out: Vec<String> = Vec::new();
    for c in re.captures_iter(&text) {
        if let Some(h) = c.get(1) {
            let h = h.as_str().to_string();
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}
```

Note: `unreachable!` is the same escape hatch `parse()` already uses for its own regex —
it satisfies `clippy::unwrap_used = deny` without hiding a real error, since the pattern is a
literal covered by the tests.

- [ ] **Step 4: Check green**

Run: `cargo test -p storm-lobby --test oracle les_hashes`
Expected: PASS

- [ ] **Step 5: Write the derivation tool**

`crates/storm-lobby/examples/derive_maps.rs`:

```rust
//! Derives the `.s2ma hash → map name` table from a folder of archived replays.
//!
//! Principle: each replay gives (map known via the full parse, hashes of the blob). A hash is
//! kept for a map if it appears in ALL the replays of that map and in NO replay
//! of another map. Hashes common to several maps (shared assets) are therefore set aside.
//!
//! Usage: cargo run --release -p storm-lobby --example derive_maps -- <folder> > src/maps.rs

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: derive_maps <dossier>"))?
        .into();

    // map → number of replays, hash → maps where it appears, and (map,hash) → occurrences
    let mut replays_par_carte: BTreeMap<String, usize> = BTreeMap::new();
    let mut cartes_par_hash: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut occurrences: BTreeMap<(String, String), usize> = BTreeMap::new();

    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("StormReplay") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else { continue };
        let Ok(replay) = storm_replay::Replay::open(&path) else { continue };
        let Ok(blob) = replay.battlelobby_raw() else { continue };
        let out = storm_stats::process_replay(&path, filename);
        if out.status != 1 {
            continue;
        }
        let json = out.to_json();
        let Some(carte) = json["match"]["map"].as_str() else { continue };
        let carte = carte.to_string();

        *replays_par_carte.entry(carte.clone()).or_default() += 1;
        for h in storm_lobby::map_hashes(&blob) {
            cartes_par_hash.entry(h.clone()).or_default().insert(carte.clone());
            *occurrences.entry((carte.clone(), h)).or_default() += 1;
        }
    }

    // A hash is discriminating if it appears on only one map, and on ALL its replays.
    let mut table: Vec<(String, String)> = Vec::new();
    for (hash, cartes) in &cartes_par_hash {
        if cartes.len() != 1 {
            continue;
        }
        let Some(carte) = cartes.iter().next() else { continue };
        let vus = occurrences.get(&(carte.clone(), hash.clone())).copied().unwrap_or(0);
        let total = replays_par_carte.get(carte).copied().unwrap_or(0);
        if total > 0 && vus == total {
            table.push((hash.clone(), carte.clone()));
        }
    }
    table.sort();

    println!("//! `.s2ma hash → map name` table, GENERATED — do not edit by hand.");
    println!("//!");
    println!("//! Produced by `cargo run --release -p storm-lobby --example derive_maps -- <folder>`");
    println!("//! on the box's archive. A hash is kept only if it appears on a single map");
    println!("//! AND on all the replays of that map.");
    println!();
    println!("/// `(hash, map name)`, sorted by hash — the names are the ones `matches.map` stores.");
    println!("pub(crate) const MAP_BY_HASH: &[(&str, &str)] = &[");
    for (h, c) in &table {
        println!("    ({h:?}, {c:?}),");
    }
    println!("];");

    eprintln!("maps covered : {}",
        table.iter().map(|(_, c)| c).collect::<BTreeSet<_>>().len());
    eprintln!("hashes kept  : {}", table.len());
    eprintln!("maps seen    : {}", replays_par_carte.len());
    Ok(())
}
```

- [ ] **Step 6: Generate the table from the archive**

The archive lives in a Docker volume on the box. Fetch it **read-only** (the pattern of the
runbook `docs/runbooks/2026-07-09-visionneuse-2d-verif-box.md`):

```bash
mkdir -p /tmp/lobby-archive && \
ssh matella@192.168.129.85 "docker cp storm-codex-server:/data/archive -" | tar -xf - -C /tmp/lobby-archive && \
ls /tmp/lobby-archive/archive | wc -l
```

Expected: ~3300 files. Then (several minutes — background or high timeout):

```bash
cargo run --release -p storm-lobby --example derive_maps -- /tmp/lobby-archive/archive \
  > crates/storm-lobby/src/maps.rs
```

Look at the error output: "maps covered" should approach "maps seen". If fewer than
half of the maps are covered, **do not force it**: record the result and report it to
the operator — the map will stay a manual entry, which plan 3 already handles.

- [ ] **Step 7: Wire the table into `parse()`**

In `crates/storm-lobby/src/lib.rs`:

```rust
mod maps;

// … in the construction of the Lobby, after the players:
let map = map_hashes(bytes)
    .iter()
    .find_map(|h| {
        maps::MAP_BY_HASH
            .iter()
            .find(|(hash, _)| hash == h)
            .map(|(_, nom)| (*nom).to_string())
    });
```

and add the field to the structure:

```rust
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Map, inferred from the `.s2ma` hashes via a table derived from the archive. `None` if no
    /// known hash — the format never names the map.
    pub map: Option<String>,
}
```

- [ ] **Step 8: End-to-end map test**

Add to `crates/storm-lobby/tests/oracle.rs`:

```rust
/// The map inferred from the hashes must match the one from the full parse, when it is inferred.
/// A `None` map is acceptable (hash unknown to the table); a WRONG map is not.
#[test]
fn la_carte_deduite_ne_ment_jamais() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let filename = label;
        let out = storm_stats::process_replay(&path, filename);
        assert_eq!(out.status, 1, "{label}: full parse rejected");
        let json = out.to_json();
        let attendue = json["match"]["map"].as_str().unwrap_or_default().to_string();

        if let Some(deduite) = lobby_of(&path).map {
            assert_eq!(deduite, attendue, "{label}: wrong inferred map");
        }
    }
}
```

Run: `cargo test -p storm-lobby`
Expected: PASS (the 4 previous tests + the 2 new ones).

- [ ] **Step 9: Clean up and commit**

```bash
rm -rf /tmp/lobby-archive
cargo clippy -p storm-lobby --all-targets -- -D warnings
git add crates/storm-lobby
git commit -m "feat(storm-lobby): infer the map from the .s2ma hashes via a table derived from the archive"
```

---

## Task 2: Migration `0009` and build library

**Files:**
- Create: `crates/storm-codex-server/migrations/0009_companion.sql`
- Create: `crates/storm-codex-server/src/builds.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (module declaration + routes)

**Interfaces:**
- Produces: the routes `GET /api/builds`, `POST /api/builds`, `PUT /api/builds/{id}`,
  `DELETE /api/builds/{id}`, `POST /api/builds/from-match`. Task 4 will read the `builds` table to
  attach the default build to the lobby; task 6 will compare `builds.picks` with the talents played.
- Consumes: `AppState { db: PgPool, … }` (`main.rs:31`).

- [ ] **Step 1: Write the migration**

`crates/storm-codex-server/migrations/0009_companion.sql`:

```sql
-- Build library. `picks` has EXACTLY the shape written by the parser in
-- match_players.data.talents: {"Tier1Choice": "<talentTreeId>", ...}. That is what makes importing
-- from a match and the post-game diff simple object comparisons, with no mapping to invent.
CREATE TABLE builds (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hero_id         TEXT NOT NULL,          -- = dim_heroes.id = match_players.hero
    name            TEXT NOT NULL,
    picks           JSONB NOT NULL,
    notes           TEXT,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    source_match_id BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX builds_hero_idx ON builds (hero_id);
-- Invariant held by the database, not by the code: at most one default build per hero.
CREATE UNIQUE INDEX builds_one_default_per_hero ON builds (hero_id) WHERE is_default;

-- Current lobby. Singleton, exact copy of draft_live: the whole state in the JSON, overwritten at each
-- new lobby. No history — the archived replay stays the source of truth.
CREATE TABLE lobby_live (
    id         INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- BattleTag → toon_handle resolution: the lobby blob only carries "name#discriminant", and
-- the archive is the only mapping table available (see the companion-live spec).
CREATE INDEX match_players_name_tag_idx
    ON match_players (lower(name), (data ->> 'tag'));
```

- [ ] **Step 2: Check that the migration applies**

```bash
docker compose -f docker-compose.dev.yml up -d
cargo run -p storm-codex-server 2>&1 | head -20
```

Expected: the server starts without a migration error. Check:

```bash
docker compose -f docker-compose.dev.yml exec -T postgres \
  psql -U postgres -d storm_codex -c '\d builds' | head -15
```

Expected: the 9 columns of `builds`.

- [ ] **Step 3: Write the builds module**

`crates/storm-codex-server/src/builds.rs`:

```rust
//! Talent build library. `picks` reuses the parser's shape
//! (`{TierNChoice: talentTreeId}`), which allows importing a build from a played game and
//! diffing "planned vs taken" without conversion. The uniqueness of the default build is held by a
//! partial unique index in the database, not by this code.
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value as J};

use crate::AppState;

type Resp = Result<Json<J>, (StatusCode, Json<J>)>;

fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

/// Library mutations follow the same guard as teams/collections: open if
/// no `ADMIN_TOKEN` is configured (the suite spec's default local mode), protected otherwise.
/// Reading always stays open, like `list_teams`.
fn refus_admin() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin token requis" })),
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Optional filter on the hero (key `dim_heroes.id`).
    pub hero: Option<String>,
}

/// `GET /api/builds` — the library, optionally filtered by hero.
pub async fn list(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(b ORDER BY b.hero_id, b.is_default DESC, b.name), '[]'::jsonb)
         FROM (
            SELECT id, hero_id, name, picks, notes, is_default, source_match_id, updated_at
            FROM builds
            WHERE $1::text IS NULL OR hero_id = $1
         ) b",
    )
    .bind(q.hero)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct BuildBody {
    pub hero_id: String,
    pub name: String,
    pub picks: J,
    pub notes: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// `POST /api/builds` — create a build. Marking `is_default` first removes the existing default of the
/// same hero: without that, the partial unique index would reject the insertion.
pub async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<BuildBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let mut tx = s.db.begin().await.map_err(db_err)?;
    if b.is_default {
        sqlx::query("UPDATE builds SET is_default = false WHERE hero_id = $1 AND is_default")
            .bind(&b.hero_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO builds (hero_id, name, picks, notes, is_default)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&b.hero_id)
    .bind(&b.name)
    .bind(&b.picks)
    .bind(&b.notes)
    .bind(b.is_default)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(json!({ "id": id })))
}

/// `PUT /api/builds/{id}` — replace a build. Same precaution on the default as `create`.
pub async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<BuildBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let mut tx = s.db.begin().await.map_err(db_err)?;
    if b.is_default {
        sqlx::query("UPDATE builds SET is_default = false WHERE hero_id = $1 AND is_default AND id <> $2")
            .bind(&b.hero_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    let n = sqlx::query(
        "UPDATE builds SET hero_id = $2, name = $3, picks = $4, notes = $5, is_default = $6,
                updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&b.hero_id)
    .bind(&b.name)
    .bind(&b.picks)
    .bind(&b.notes)
    .bind(b.is_default)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    tx.commit().await.map_err(db_err)?;
    if n == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "build inconnu" }))));
    }
    Ok(Json(json!({ "ok": true })))
}

/// `DELETE /api/builds/{id}`.
pub async fn delete(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let n = sqlx::query("DELETE FROM builds WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "build inconnu" }))));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct FromMatchBody {
    pub match_id: i64,
    pub toon_handle: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
}

/// `POST /api/builds/from-match` — seed a build from a played game. This is what avoids
/// entering 90 heroes by hand: the archive's talents already have the right shape.
pub async fn from_match(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<FromMatchBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let row: Option<(String, J)> = sqlx::query_as(
        "SELECT hero, COALESCE(data -> 'talents', '{}'::jsonb)
         FROM match_players WHERE match_id = $1 AND toon_handle = $2",
    )
    .bind(b.match_id)
    .bind(&b.toon_handle)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;

    let Some((hero_id, picks)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "joueur absent de ce match" })),
        ));
    };
    if picks.as_object().is_none_or(serde_json::Map::is_empty) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "aucun talent enregistré pour ce joueur" })),
        ));
    }

    create(
        State(s),
        headers,
        Json(BuildBody {
            hero_id,
            name: b.name,
            picks,
            notes: None,
            is_default: b.is_default,
        }),
    )
    .await
}
```

- [ ] **Step 4: Declare the module and the routes**

In `crates/storm-codex-server/src/manage.rs`, make the guard reusable — it is
currently private:

```rust
pub(crate) fn is_admin(h: &axum::http::HeaderMap, s: &AppState) -> bool {
```

(only the keyword changes; the body stays identical.)

In `crates/storm-codex-server/src/main.rs`, next to the other `mod`s:

```rust
mod builds;
```

and in the `Router::new()`, after the `/api/collections` routes:

```rust
        .route("/api/builds", get(builds::list).post(builds::create))
        .route(
            "/api/builds/{id}",
            axum::routing::put(builds::update).delete(builds::delete),
        )
        .route("/api/builds/from-match", post(builds::from_match))
```

- [ ] **Step 5: Check with curl, including the database invariant**

Start the server, then:

```bash
# In dev without ADMIN_TOKEN configured, the guard lets through; with a token, add
# -H "authorization: Bearer $ADMIN_TOKEN" to the three mutations.
curl -s -XPOST localhost:8080/api/builds -H 'content-type: application/json' \
  -d '{"hero_id":"Tychus","name":"Anti-heal","picks":{"Tier1Choice":"TychusMasterAssassin"},"is_default":true}'
curl -s -XPOST localhost:8080/api/builds -H 'content-type: application/json' \
  -d '{"hero_id":"Tychus","name":"Poke","picks":{"Tier1Choice":"TychusOverdrive"},"is_default":true}'
curl -s 'localhost:8080/api/builds?hero=Tychus'
```

Expected: two `{"id":N}`, then a list of **two** builds of which **only one** has `is_default: true`
(the second — the first was unmarked by the transaction). If both come out `true`, or if the
second insertion fails with a 500, the `create` transaction is faulty.

Then check that the index really protects against a bypass:

```bash
docker compose -f docker-compose.dev.yml exec -T postgres psql -U postgres -d storm_codex \
  -c "UPDATE builds SET is_default = true WHERE hero_id = 'Tychus';"
```

Expected: `ERROR: duplicate key value violates unique constraint "builds_one_default_per_hero"`.
This is the intended behaviour — the invariant holds even if future code forgets it.

- [ ] **Step 6: Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server/migrations/0009_companion.sql crates/storm-codex-server/src/builds.rs crates/storm-codex-server/src/main.rs
git commit -m "feat(server): migration 0009 + build library (CRUD, import from a match)"
```

---

## Task 3: Live lobby ingestion

**Files:**
- Create: `crates/storm-codex-server/src/lobby/mod.rs`
- Create: `crates/storm-codex-server/src/lobby/store.rs`
- Create: `crates/storm-codex-server/src/lobby/api.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (`lobby` field in `AppState`, loading at
  startup, routes)
- Modify: `crates/storm-codex-server/Cargo.toml` (`storm-lobby` dependency)

**Interfaces:**
- Produces: `LobbyState` (serialisable, carries `schema_version`), the routes `POST /api/lobby`,
  `GET /api/lobby`, `DELETE /api/lobby`, and the WS event `lobby.detected`. Task 4 will fill the
  `players[].history` field; task 5 will mutate `hero`, `map` and the teams; task 6 will fill
  `match_id`.
- Consumes: `storm_lobby::{parse, Lobby, LobbyPlayer}` (plan 1), `crate::AppState`.

- [ ] **Step 1: Add the dependency**

In `crates/storm-codex-server/Cargo.toml`, `[dependencies]` section:

```toml
storm-lobby = { path = "../storm-lobby" }
```

- [ ] **Step 2: Define the state**

`crates/storm-codex-server/src/lobby/mod.rs`:

```rust
//! Live lobby: current state of a game being loaded. Persisted singleton (modelled on
//! `draft_live`) — the current lobby overwrites the previous one, there is no lobby history: the
//! archived replay stays the source of truth (tier 1 of the three data tiers).
pub mod api;
pub mod enrich;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

/// Schema version of `lobby_live.state` — a state of an unknown version is ignored at
/// startup rather than deserialised wrongly.
pub const LOBBY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayerState {
    pub name: String,
    pub discriminant: String,
    /// `"name#1234"` — the identity key from the blob.
    pub battletag: String,
    /// Team: the one inferred by `storm-lobby`, or the one set by hand (task 5).
    pub team: Option<u8>,
    /// `true` if the team was corrected by hand — the front shows it differently.
    #[serde(default)]
    pub team_manual: bool,
    /// Application identity, resolved against the archive. `None` = player never met.
    pub toon_handle: Option<String>,
    /// Archive aggregates (task 4). `null` until enriched or if the player is unknown.
    #[serde(default)]
    pub history: Option<J>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyState {
    pub schema_version: u32,
    pub detected_at: String,
    pub players: Vec<LobbyPlayerState>,
    /// Map: inferred from the `.s2ma` hashes, or entered by hand.
    pub map: Option<String>,
    #[serde(default)]
    pub map_manual: bool,
    /// The operator's hero — never in the blob, always entered (task 5).
    pub hero: Option<String>,
    /// Index in `players` of the player identified as the operator, if found.
    pub me: Option<usize>,
    /// Suggested build + alternatives (task 4).
    #[serde(default)]
    pub build: Option<J>,
    /// The operator's stats on this hero / this map (task 4).
    #[serde(default)]
    pub me_stats: Option<J>,
    /// Filled when the matching replay is parsed (task 6) → switches to debrief.
    #[serde(default)]
    pub match_id: Option<i64>,
    /// `parse_failed` when the blob is unreadable: the page then shows the manual selector
    /// rather than an empty page.
    #[serde(default)]
    pub status: Option<String>,
}

impl LobbyState {
    /// Builds the raw state from a decoded lobby, before enrichment.
    #[must_use]
    pub fn from_lobby(lobby: &storm_lobby::Lobby, detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: lobby
                .players
                .iter()
                .map(|p| LobbyPlayerState {
                    name: p.name.clone(),
                    discriminant: p.discriminant.clone(),
                    battletag: p.battletag(),
                    team: p.team,
                    team_manual: false,
                    toon_handle: None,
                    history: None,
                })
                .collect(),
            map: lobby.map.clone(),
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: None,
        }
    }

    /// Degraded state when the blob could not be decoded (new Blizzard build). The page stays
    /// usable: the operator enters their hero, the build shows.
    #[must_use]
    pub fn unreadable(detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: Vec::new(),
            map: None,
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: Some("parse_failed".into()),
        }
    }
}
```

- [ ] **Step 3: Persistence (modelled on `draft/store.rs`)**

`crates/storm-codex-server/src/lobby/store.rs`:

```rust
//! Persistence of the current lobby (singleton `lobby_live`, id=1). Runtime queries, like the rest
//! of the server. Deliberate copy of the `draft/store.rs` pattern: two independent singletons, each
//! with its own life cycle; factoring them would cost more in indirection than it would bring.
use crate::lobby::{LobbyState, LOBBY_SCHEMA_VERSION};
use sqlx::PgPool;

/// Loads the persisted state. A state of another schema version is ignored (→ no lobby).
pub async fn load(db: &PgPool) -> Option<LobbyState> {
    let row: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT state FROM lobby_live WHERE id = 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    let state: LobbyState = row.and_then(|j| serde_json::from_value(j).ok())?;
    (state.schema_version == LOBBY_SCHEMA_VERSION).then_some(state)
}

/// Writes the state (upsert on the single row).
pub async fn save(db: &PgPool, state: &LobbyState) -> Result<(), sqlx::Error> {
    let v = serde_json::to_value(state).unwrap_or_else(|_| serde_json::json!({}));
    sqlx::query(
        "INSERT INTO lobby_live (id, state, updated_at) VALUES (1, $1, now())
         ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
    )
    .bind(&v)
    .execute(db)
    .await
    .map(|_| ())
}

/// Clears the current lobby.
pub async fn clear(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_live WHERE id = 1")
        .execute(db)
        .await
        .map(|_| ())
}
```

- [ ] **Step 4: The ingestion routes**

`crates/storm-codex-server/src/lobby/api.rs`:

```rust
//! Live lobby routes. `POST /api/lobby` receives the raw bytes of the lobby file (same
//! contract as `/api/upload-raw`: `Bearer` + binary body), decodes it, enriches it, persists it and
//! broadcasts `lobby.detected`. With the parser living on the server, a Blizzard format break is
//! fixed by a box redeployment, without ever touching the Windows binary.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value as J};

use crate::lobby::{store, LobbyState};
use crate::AppState;

/// Idempotence: two POSTs of the same file (the watcher restarts, the game rewrites) must not
/// overwrite an already enriched state nor rebroadcast an event. We compare the set of BattleTags.
fn meme_lobby(a: &LobbyState, b: &LobbyState) -> bool {
    if a.players.len() != b.players.len() || a.players.is_empty() {
        return false;
    }
    let mut x: Vec<&str> = a.players.iter().map(|p| p.battletag.as_str()).collect();
    let mut y: Vec<&str> = b.players.iter().map(|p| p.battletag.as_str()).collect();
    x.sort_unstable();
    y.sort_unstable();
    x == y
}

/// `POST /api/lobby` — raw bytes, upload `Bearer`.
pub async fn ingest(
    State(s): State<AppState>,
    headers: HeaderMap,
    bytes: axum::body::Bytes,
) -> (StatusCode, Json<J>) {
    if !crate::upload::token_valide(&s.db, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        );
    }

    let detected_at = chrono::Utc::now().to_rfc3339();
    let mut state = match storm_lobby::parse(&bytes) {
        Ok(lobby) => LobbyState::from_lobby(&lobby, detected_at),
        Err(e) => {
            // Not an HTTP error: the page must open anyway, with the manual selector.
            tracing::warn!("unreadable lobby ({} bytes): {e}", bytes.len());
            LobbyState::unreadable(detected_at)
        }
    };

    {
        let courant = s.lobby.read().await;
        if let Some(prec) = courant.as_ref() {
            if meme_lobby(prec, &state) {
                return (StatusCode::OK, Json(json!({ "status": "unchanged" })));
            }
        }
    }

    crate::lobby::enrich::enrich(&s.db, &mut state).await;
    let _ = store::save(&s.db, &state).await;
    let joueurs = state.players.len();
    *s.lobby.write().await = Some(state);
    let _ = s.events.send(json!({ "type": "lobby.detected" }));

    (
        StatusCode::ACCEPTED,
        Json(json!({ "status": "ok", "players": joueurs })),
    )
}

/// `GET /api/lobby` — the current enriched state, `204` if there is no lobby.
pub async fn get(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    match s.lobby.read().await.as_ref() {
        None => (StatusCode::NO_CONTENT, Json(J::Null)),
        Some(st) => (
            StatusCode::OK,
            Json(serde_json::to_value(st).unwrap_or(J::Null)),
        ),
    }
}

/// `DELETE /api/lobby` — close the companion.
pub async fn clear(State(s): State<AppState>) -> Json<J> {
    let _ = store::clear(&s.db).await;
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
}
```

- [ ] **Step 5: Extract the token validation to reuse it**

In `crates/storm-codex-server/src/upload.rs`, the validation currently lives inline in `upload`
(lines ~73-96). Extract it into a public function and call it from both places:

```rust
/// True if the header carries a `Bearer` matching a non-revoked upload token.
/// Shared with `lobby::api::ingest`: both endpoints have the same authentication contract.
pub async fn token_valide(db: &sqlx::PgPool, headers: &axum::http::HeaderMap) -> bool {
    token_id(db, headers).await.is_some()
}

/// The token's id, when it is valid (the upload needs it to record `uploads.token_id`).
pub async fn token_id(db: &sqlx::PgPool, headers: &axum::http::HeaderMap) -> Option<i64> {
    let tok = bearer(headers)?;
    let h = sha256_hex(tok.as_bytes());
    sqlx::query_scalar("SELECT id FROM upload_tokens WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(&h)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}
```

Then replace the inline block of `upload` with `let Some(token_id) = upload::token_id(&state.db,
&headers).await else { … }`, keeping its existing 401 response identical.

- [ ] **Step 6: Wire the state into `AppState`**

In `crates/storm-codex-server/src/main.rs`, add the module and the field:

```rust
mod lobby;
```

```rust
pub struct AppState {
    // … existing fields …
    /// Current live lobby (singleton, persisted in `lobby_live`). `None` = no lobby.
    pub lobby: Arc<RwLock<Option<lobby::LobbyState>>>,
}
```

When building the state, after loading the draft:

```rust
    let lobby_initial = lobby::store::load(&db).await;
```

and in the `AppState { … }` literal:

```rust
        lobby: Arc::new(RwLock::new(lobby_initial)),
```

Routes, after the draft ones:

```rust
        .route("/api/lobby", get(lobby::api::get).post(lobby::api::ingest))
        .route("/api/lobby", axum::routing::delete(lobby::api::clear))
```

⚠️ axum refuses two `.route()` calls on the same path: **merge** them into a single call —
`.route("/api/lobby", get(lobby::api::get).post(lobby::api::ingest).delete(lobby::api::clear))`.

- [ ] **Step 7: Check end to end**

Create a token, extract a real blob, post it:

```bash
TOKEN=$(curl -s -XPOST localhost:8080/api/admin/tokens -H "authorization: Bearer $ADMIN_TOKEN" \
  -H 'content-type: application/json' -d '{"name":"test-lobby"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
cargo run -q -p storm-replay --example dump_lobby -- \
  "crates/storm-stats/tests/data/silver-city-aram.StormReplay" /tmp/lobby.bin
curl -s -XPOST localhost:8080/api/lobby -H "authorization: Bearer $TOKEN" \
  --data-binary @/tmp/lobby.bin
curl -s localhost:8080/api/lobby | head -c 400
```

Expected: `{"status":"ok","players":10}` then a JSON state with 10 players and their BattleTags.
Posting the same file again must answer `{"status":"unchanged"}`. Without `Bearer`: 401.

- [ ] **Step 8: Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server
git commit -m "feat(server): live lobby ingestion (POST/GET/DELETE /api/lobby + WS lobby.detected)"
```

---

## Task 4: Enrichment against the archive

**Files:**
- Create: `crates/storm-codex-server/src/lobby/enrich.rs`

**Interfaces:**
- Produces: `pub async fn enrich(db: &PgPool, state: &mut LobbyState)` — fills `toon_handle`,
  `history`, `me`, `build` and `me_stats`. Called by `lobby::api::ingest` (task 3) and after each
  manual mutation (task 5).
- Consumes: `LobbyState`, `LobbyPlayerState` (task 3); the `builds` table (task 2).

This is the heart of the companion's value: what the blob gives (BattleTags) becomes what you
want to see (who these people are, and how it went with them).

- [ ] **Step 1: Write the identity resolution**

```rust
//! Enrichment of the lobby against the archive. The blob only carries `name#discriminant`: the
//! `toon_handle` is found in `match_players` (name + `data->>'tag'`), that is, in the
//! games already played. A player never met stays unresolved — with no consequence, since they
//! have no history to show anyway.
use serde_json::Value as J;
use sqlx::PgPool;

use crate::lobby::LobbyState;

/// `name#tag` → `toon_handle`, according to the archive. `None` if never met.
async fn resoudre(db: &PgPool, name: &str, discriminant: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT toon_handle FROM match_players
         WHERE lower(name) = lower($1) AND data ->> 'tag' = $2
         ORDER BY match_id DESC LIMIT 1",
    )
    .bind(name)
    .bind(discriminant)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}
```

- [ ] **Step 2: Write the per-player aggregate**

```rust
/// History of a player from the operator's point of view: games together, games against,
/// win rates, favourite heroes. A single query — the spec's p95 < 100 ms contract applies to
/// `/api/lobby`, and one round trip per player would blow it.
async fn historique(db: &PgPool, toon: &str) -> Option<J> {
    sqlx::query_scalar(
        "WITH moi AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         ),
         mes_parties AS (
            SELECT mp.match_id, mp.team
            FROM match_players mp
            WHERE lower(mp.name) IN (SELECT name FROM moi)
         ),
         croisements AS (
            SELECT mp.match_id,
                   (mp.team = mes.team) AS ensemble,
                   mp.win
            FROM match_players mp
            JOIN mes_parties mes ON mes.match_id = mp.match_id
            WHERE mp.toon_handle = $1
         )
         SELECT jsonb_build_object(
            'toon', $1::text,
            'games_with', (SELECT count(*) FROM croisements WHERE ensemble),
            'wins_with', (SELECT count(*) FROM croisements WHERE ensemble AND win),
            'games_against', (SELECT count(*) FROM croisements WHERE NOT ensemble),
            'wins_against', (SELECT count(*) FROM croisements WHERE NOT ensemble AND win),
            'last_seen', (SELECT max(m.played_at) FROM croisements c
                          JOIN matches m ON m.id = c.match_id),
            'top_heroes', COALESCE((SELECT jsonb_agg(h ORDER BY h.games DESC) FROM (
                SELECT hero, count(*) AS games, count(*) FILTER (WHERE win) AS wins
                FROM match_players
                WHERE toon_handle = $1 AND hero IS NOT NULL
                GROUP BY hero ORDER BY count(*) DESC LIMIT 3) h), '[]'::jsonb))",
    )
    .bind(toon)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}
```

- [ ] **Step 3: Write the operator's stats and the build**

```rust
/// The operator's stats on this hero, this map, and the pair. `hero` can be `None` as long as
/// the operator has not entered it: the map stats can still be computed.
async fn stats_operateur(db: &PgPool, hero: Option<&str>, map: Option<&str>) -> Option<J> {
    sqlx::query_scalar(
        "WITH moi AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         ),
         miennes AS (
            SELECT mp.hero, mp.win, m.map
            FROM match_players mp JOIN matches m ON m.id = mp.match_id
            WHERE lower(mp.name) IN (SELECT name FROM moi)
         )
         SELECT jsonb_build_object(
            'hero_games',     (SELECT count(*) FROM miennes WHERE hero = $1),
            'hero_wins',      (SELECT count(*) FROM miennes WHERE hero = $1 AND win),
            'map_games',      (SELECT count(*) FROM miennes WHERE map = $2),
            'map_wins',       (SELECT count(*) FROM miennes WHERE map = $2 AND win),
            'hero_map_games', (SELECT count(*) FROM miennes WHERE hero = $1 AND map = $2),
            'hero_map_wins',  (SELECT count(*) FROM miennes WHERE hero = $1 AND map = $2 AND win))",
    )
    .bind(hero)
    .bind(map)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Default build of the hero + alternatives. `None` if no build is recorded: the front then offers
/// the import from the best game on this hero.
async fn build_du_heros(db: &PgPool, hero: &str) -> Option<J> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'default', (SELECT to_jsonb(b) FROM (
                SELECT id, name, picks, notes FROM builds
                WHERE hero_id = $1 AND is_default LIMIT 1) b),
            'alternates', COALESCE((SELECT jsonb_agg(a ORDER BY a.name) FROM (
                SELECT id, name, picks, notes FROM builds
                WHERE hero_id = $1 AND NOT is_default) a), '[]'::jsonb))",
    )
    .bind(hero)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}
```

- [ ] **Step 4: Assemble**

```rust
/// Fills the state with everything the archive knows. Best-effort: a failing query leaves the
/// field at `None` rather than failing the ingestion — a half-enriched lobby is better
/// than an empty page.
pub async fn enrich(db: &PgPool, state: &mut LobbyState) {
    let noms_operateur: Vec<String> = sqlx::query_scalar(
        "SELECT lower(jsonb_array_elements_text(value)) FROM app_settings WHERE key = 'operator_names'",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    for (i, p) in state.players.iter_mut().enumerate() {
        p.toon_handle = resoudre(db, &p.name, &p.discriminant).await;
        if let Some(t) = p.toon_handle.as_deref() {
            p.history = historique(db, t).await;
        }
        if noms_operateur.iter().any(|n| n == &p.name.to_lowercase()) {
            state.me = Some(i);
        }
    }

    state.me_stats = stats_operateur(db, state.hero.as_deref(), state.map.as_deref()).await;
    state.build = match state.hero.as_deref() {
        Some(h) => build_du_heros(db, h).await,
        None => None,
    };
}
```

The `serde_json::json` import is not used by this module: only import it if a change
requires it — `cargo clippy -D warnings` would flag a dead import.

- [ ] **Step 5: Check against a populated database**

Data is needed: upload a few replays of the committed corpus into the dev database, then
post the matching lobby blob again.

```bash
for f in crates/storm-replay/tests/data/*.StormReplay crates/storm-stats/tests/data/*.StormReplay; do
  curl -s -XPOST localhost:8080/api/upload -H "authorization: Bearer $TOKEN" \
    -H "X-Filename: $(basename "$f")" --data-binary @"$f" > /dev/null
done
curl -s -XPUT localhost:8080/api/admin/settings -H "authorization: Bearer $ADMIN_TOKEN" \
  -H 'content-type: application/json' -d '{"operator_names":["matella"]}'
curl -s -XDELETE localhost:8080/api/lobby
curl -s -XPOST localhost:8080/api/lobby -H "authorization: Bearer $TOKEN" --data-binary @/tmp/lobby.bin
curl -s localhost:8080/api/lobby | python3 -m json.tool | head -50
```

Expected: the 10 players have a non-null `toon_handle` (they are all in the dev archive since
the lobby comes from one of these replays), `history.games_with` or `games_against` ≥ 1 for each, and
`me` points at `matella`'s index.

Measure the spec's budget:

```bash
for i in $(seq 20); do curl -s -o /dev/null -w '%{time_total}\n' localhost:8080/api/lobby; done | sort -n | tail -2
```

Expected: p95 < 0.100 s. Beyond that, the culprit is the per-player loop of step 4: replace it
with **one** query taking the array of toon handles (`= ANY($1)`) rather than running ten.

- [ ] **Step 6: Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server/src/lobby/enrich.rs
git commit -m "feat(server): enrich the lobby against the archive (identities, histories, build, stats)"
```

---

## Task 5: Manual entries and corrections

**Files:**
- Modify: `crates/storm-codex-server/src/lobby/api.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (routes)

**Interfaces:**
- Produces: `POST /api/lobby/hero`, `POST /api/lobby/map`, `POST /api/lobby/teams`. Each one mutates
  the state, re-enriches, persists and broadcasts `lobby.updated`.
- Consumes: `LobbyState` (task 3), `enrich` (task 4).

The blob does not carry the hero (measured, plan 1). And the team inference, perfect in matchmaking,
is wrong in custom games: in 94.7% of the wrong cases the order carries **no** team
information, so a swap button would not be enough — hence a reassignment
**per player**.

- [ ] **Step 1: Add the three routes**

At the end of `crates/storm-codex-server/src/lobby/api.rs`:

```rust
#[derive(serde::Deserialize)]
pub struct HeroBody {
    /// Key `dim_heroes.id`. `null` to clear the entry.
    pub hero: Option<String>,
}

/// `POST /api/lobby/hero` — the hero is never in the blob: it is the only mandatory tap.
pub async fn set_hero(State(s): State<AppState>, Json(b): Json<HeroBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| st.hero = b.hero).await
}

#[derive(serde::Deserialize)]
pub struct MapBody {
    pub map: Option<String>,
}

/// `POST /api/lobby/map` — fallback when the map could not be inferred from the `.s2ma` hashes.
pub async fn set_map(State(s): State<AppState>, Json(b): Json<MapBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| {
        st.map = b.map;
        st.map_manual = true;
    })
    .await
}

#[derive(serde::Deserialize)]
pub struct TeamsBody {
    /// `battletag → team (0 or 1)`. Players absent from the table keep their team.
    pub teams: std::collections::HashMap<String, u8>,
}

/// `POST /api/lobby/teams` — per-player reassignment. A simple swap button would only fix
/// 5.3% of the wrong cases (plan 1 measurement): in the others, the lobby order carries
/// no team information and only an explicit entry can rebuild it.
pub async fn set_teams(State(s): State<AppState>, Json(b): Json<TeamsBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| {
        for p in &mut st.players {
            if let Some(t) = b.teams.get(&p.battletag) {
                if *t <= 1 {
                    p.team = Some(*t);
                    p.team_manual = true;
                }
            }
        }
    })
    .await
}

/// Mutation + re-enrichment + persistence + broadcast, factored out: the three routes
/// above differ only by the mutation itself.
async fn muter<F>(s: AppState, f: F) -> (StatusCode, Json<J>)
where
    F: FnOnce(&mut LobbyState),
{
    let mut guard = s.lobby.write().await;
    let Some(st) = guard.as_mut() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "aucun lobby courant" })),
        );
    };
    f(st);
    crate::lobby::enrich::enrich(&s.db, st).await;
    let _ = store::save(&s.db, st).await;
    let out = serde_json::to_value(&*st).unwrap_or(J::Null);
    drop(guard);
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    (StatusCode::OK, Json(out))
}
```

- [ ] **Step 2: Declare the routes**

```rust
        .route("/api/lobby/hero", post(lobby::api::set_hero))
        .route("/api/lobby/map", post(lobby::api::set_map))
        .route("/api/lobby/teams", post(lobby::api::set_teams))
```

- [ ] **Step 3: Check**

```bash
curl -s -XPOST localhost:8080/api/lobby/hero -H 'content-type: application/json' \
  -d '{"hero":"Tychus"}' | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d["hero"], d["build"] is not None, d["me_stats"])'
```

Expected: `Tychus True {...}` — the hero's build and the stats appear immediately, proof
that the mutation re-enriches.

```bash
BT=$(curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print(json.load(sys.stdin)["players"][0]["battletag"])')
curl -s -XPOST localhost:8080/api/lobby/teams -H 'content-type: application/json' \
  -d "{\"teams\":{\"$BT\":1}}" | python3 -c 'import sys,json;p=json.load(sys.stdin)["players"][0];print(p["team"], p["team_manual"])'
```

Expected: `1 True`. And without a current lobby (`DELETE /api/lobby` first), the three routes must
answer 404, not 500.

- [ ] **Step 4: Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server
git commit -m "feat(server): hero/map entry and per-player team reassignment"
```

---

## Task 6: Replay ↔ lobby link and build debrief

**Files:**
- Modify: `crates/storm-codex-server/src/lobby/mod.rs` (link function)
- Modify: `crates/storm-codex-server/src/upload.rs` (call after a successful parse)

**Interfaces:**
- Produces: `pub async fn lier_match(db, lobby, match_id)` — fills `LobbyState.match_id` when the
  BattleTags match. The front (plan 3) then switches to debrief and compares `build.default.picks`
  with the talents played, two objects of the same shape.
- Consumes: `LobbyState` (task 3), the existing parse path (`upload.rs:295`, where
  `match.parsed` is already broadcast).

- [ ] **Step 1: Write the link**

In `crates/storm-codex-server/src/lobby/mod.rs`:

```rust
/// Links a freshly parsed match to the current lobby, if it is the same game. Criterion: the set
/// of BattleTags. The full parse rebuilds the same `name#tag` from the blob embedded in the
/// replay (`storm_stats`, `get_battletags`), so both sides carry the same key — without assuming
/// anything about the binary format. 6 h window to rule out a composition replayed later.
pub async fn lier_match(db: &sqlx::PgPool, state: &mut LobbyState, match_id: i64) -> bool {
    if state.match_id.is_some() || state.players.is_empty() {
        return false;
    }
    let tags_match: Vec<String> = sqlx::query_scalar(
        "SELECT name || '#' || (data ->> 'tag')
         FROM match_players
         WHERE match_id = $1 AND name IS NOT NULL AND data ->> 'tag' IS NOT NULL",
    )
    .bind(match_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    if tags_match.len() != state.players.len() {
        return false;
    }
    let mut a: Vec<String> = tags_match;
    let mut b: Vec<String> = state.players.iter().map(|p| p.battletag.clone()).collect();
    a.sort();
    b.sort();
    if a != b {
        return false;
    }
    state.match_id = Some(match_id);
    true
}
```

- [ ] **Step 2: Call after a successful parse**

In `crates/storm-codex-server/src/upload.rs`, just before broadcasting `match.parsed`
(line ~295):

```rust
            // If this match is the game of the open lobby, the companion switches to debrief.
            {
                let mut guard = state.lobby.write().await;
                if let Some(st) = guard.as_mut() {
                    if crate::lobby::lier_match(&state.db, st, match_id).await {
                        let _ = crate::lobby::store::save(&state.db, st).await;
                        let _ = state.events.send(serde_json::json!({ "type": "lobby.updated" }));
                    }
                }
            }
```

⚠️ Do not hold the write-lock during the broadcast if the compiler reports a borrow: take
the `bool` out of the block, `drop(guard)`, then broadcast — same precaution as in `muter` (task 5).

- [ ] **Step 3: Check the switch**

```bash
curl -s -XDELETE localhost:8080/api/lobby
curl -s -XPOST localhost:8080/api/lobby -H "authorization: Bearer $TOKEN" --data-binary @/tmp/lobby.bin
curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print("match_id:", json.load(sys.stdin)["match_id"])'
```

Expected: `match_id: None` (the replay is already in the database, but no parse has happened since
the lobby was opened).

Reprocess the matching replay to trigger the parse path:

```bash
curl -s -XPOST localhost:8080/api/admin/reprocess -H "authorization: Bearer $ADMIN_TOKEN" \
  -H 'content-type: application/json' -d '{"all":true}'
curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print("match_id:", json.load(sys.stdin)["match_id"])'
```

Expected: a numeric `match_id`. Then check that the lobby of **another** game does not get
linked: post a blob from a different replay again and reprocess — `match_id` must stay `None`
for the games that are not its own.

- [ ] **Step 4: Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
cargo test --workspace
git add crates/storm-codex-server
git commit -m "feat(server): link the parsed replay to the current lobby (debrief switch)"
```

---

## End of plan

- [x] `cargo test --workspace` green
- [x] `cargo clippy -p storm-lobby --all-targets -- -D warnings` green, and
      `cargo clippy -p storm-codex-server -- -D warnings` (lib + bin) green.
      ⚠️ Correction: the box was originally ticked for `-p storm-codex-server --all-targets`, which
      **fails** — that target includes the test module of `src/draft/mod.rs` and its 11 `unwrap()`s,
      pre-existing on `main`. The original claim was false.
      (⚠️ `--workspace` fails for a **pre-existing** reason on the tests of
      `storm-codex-server`: 11 `unwrap_used`. Checked on `main`, out of scope — do not
      fix it in this plan, and do not mistake it for a regression.)
- [x] `/api/lobby` measured via `backfill_bench.py` (pass added in the final review — the old
      "p95 ≈ 35 ms" was measured by no tool in the repo): **p95 ≈ 18 ms in steady state**
      on `POST` (the path that runs the enrichment), against the 100 ms budget; `GET`
      (memory) ≈ 0.6 ms. Detail and cold peak recorded in `docs/STATUS.md`.
- [x] Map coverage by the `.s2ma` table: **19/19 maps, 116 hashes**, recorded in `docs/STATUS.md`
- [x] `docs/STATUS.md` updated (state + next step), as required by `CLAUDE.md`

Then: plan 3 (front `/companion` and `/builds`), and plan 4 (`client-rs` watcher, Hots-Overlay repo,
the only piece that needs the gaming PC).
