# Visionneuse 2D de replay — Plan d'implémentation (MVP-1)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ajouter un onglet « Replay 2D » au détail de match qui rejoue les positions des héros sur la minimap avec un scrub à seek instantané (état vivant/mort + marqueurs de mort ; pas d'HP/mana, pas d'animation).

**Architecture :** Nouveau crate `storm-replay-viewer` (dépend de `storm-replay` seul) qui projette les événements décodés en géométrie normalisée `[0,1]` clé par `playerId` replay. Un endpoint serveur `GET /api/matches/{id}/replay2d` décode à la volée (chemin + cache disque de `raw.rs`), fusionne les métadonnées joueurs depuis Postgres, et émet le JSON. Le front charge tout le modèle une fois et scrub 100 % côté client sur `<canvas>`.

**Tech Stack :** Rust 2021 (serde, thiserror), storm-replay ; axum 0.8 + sqlx (serveur) ; React 18 + TS + Vite + TanStack Query + vitest (front).

**Spec de référence :** `docs/specs/2026-07-09-visionneuse-2d-replay-design.md`

**Conventions clés vérifiées dans le code :**
- `storm_replay::Value` : `.field("m_x")`, `.as_int()`, `.as_str_lossy()`, `.as_array()`. Events = `Vec<Value>` de `Struct`.
- Héros : `SUnitBornEvent` avec `m_controlPlayerId` ∈ 1..=10 et `m_unitTypeName` préfixé `"Hero"` (ex. `HeroSylvanas`, `HeroDVaMech`).
- Base de temps : `t = (gameloop − 610) / 16`. Étendue : `SStatGameEvent{m_eventName:"GameStart"}` → `m_fixedData` `MapSizeX/MapSizeY` (point-fixe ×4096 ; `map_w = MapSizeX/4096`).
- Coords tracker (`m_x`, positions `m_items`) = tuiles entières 0..`map_w`. Coords game (`m_data.TargetPoint.x`) = ×4096 → diviser par 4096 avant normalisation.
- Serveur : `AppState { cfg: Arc<Config> (raw_cache_dir, raw_cache_max_bytes), db }` ; route base `/api/matches/{id}/…` (pas `/match/`) ; players via table `match_players (match_id, toon_handle, name, hero, team, win)`.
- Front : helper `get<T>(path)`, route `match/:id` → `pages/MatchDetail.tsx`, helpers `mapImage(map)`, `heroIcon(hero)`, `universeColor(hero)`.

**⚠️ Avant de commencer :** créer une branche/worktree (`git switch -c feat/replay2d`). Ne pas travailler sur `main`.

---

## Chunk 1 — Crate `storm-replay-viewer` (extraction → géométrie)

**Responsabilité :** replay décodé → `ViewerModel` (meta + heroes samples/life + deaths), coords normalisées `[0,1]`, clé `playerId` replay. **Zéro dépendance DB / réseau.**

**Fichiers :**
- Modifier : `Cargo.toml` (ajouter le membre workspace)
- Créer : `crates/storm-replay-viewer/Cargo.toml`
- Créer : `crates/storm-replay-viewer/src/lib.rs` (API publique `build_model`)
- Créer : `crates/storm-replay-viewer/src/model.rs` (types sérialisables)
- Créer : `crates/storm-replay-viewer/src/extract.rs` (logique d'extraction)
- Test : `crates/storm-replay-viewer/tests/extract.rs` (mini-corpus + golden)
- Données de test : réutiliser `crates/storm-stats/tests/data/silver-city-aram.StormReplay` (copier vers `crates/storm-replay-viewer/tests/data/`)

- [ ] **Step 1 : Scaffolder le crate et l'inscrire au workspace**

`Cargo.toml` (racine) — ajouter à `members` :
```toml
members = ["crates/storm-replay", "crates/storm-stats", "crates/storm-codex-server", "crates/storm-replay-viewer"]
```
`crates/storm-replay-viewer/Cargo.toml` :
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

- [ ] **Step 2 : Définir les types du modèle** — `src/model.rs`

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
    pub map_size: [f64; 2], // en tuiles (MapSize/4096), diagnostic
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

- [ ] **Step 3 : Écrire le test de base d'invariants (échoue)** — `tests/extract.rs`

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
    assert_eq!(m.heroes.len(), 10, "10 héros attendus");
    for h in &m.heroes {
        assert!((1..=10).contains(&h.player_id));
        assert!(!h.samples.is_empty(), "player {} sans samples", h.player_id);
    }
}

#[test]
fn all_coords_normalized() {
    let m = model();
    for h in &m.heroes {
        for s in &h.samples {
            assert!((0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
                "coord hors [0,1]: {:?}", s);
        }
    }
    for d in &m.deaths {
        assert!((0.0..=1.0).contains(&d.x) && (0.0..=1.0).contains(&d.y));
    }
}

#[test]
fn duration_and_meta_sane() {
    let m = model();
    assert!(m.meta.duration_sec > 60.0, "durée trop courte");
    assert_eq!(m.meta.viewer_version, 1);
    assert!(m.meta.map_size[0] > 0.0 && m.meta.map_size[1] > 0.0);
}

// Le filet de sécurité du mapping user→player : les samples EXACTS viennent des positions
// (indépendants du mapping), donc « 10 tracks non vides » ne prouve RIEN sur le mapping des
// commandes. Ces deux assertions le couvrent réellement.
#[test]
fn command_densification_lands_on_right_player() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.samples.len()).sum();
    assert!(total > 2000, "densification commande absente (total {total}) — mapping user→player cassé ?");
    for h in &m.heroes {
        let exact = h.samples.iter().filter(|s| s.exact).count();
        let cmd = h.samples.iter().filter(|s| !s.exact).count();
        assert!(exact >= 1 && cmd >= 1, "player {} : exact={exact} cmd={cmd}", h.player_id);
        // Cohérence spatiale : un joueur clique près de là où son héros se trouve. Un mapping
        // faux (ex. décalage d'équipe) éloignerait les clics des positions exactes.
        let mean = |it: &dyn Fn(&storm_replay_viewer::Sample) -> bool| {
            let v: Vec<_> = h.samples.iter().filter(|s| it(s)).collect();
            (v.iter().map(|s| s.x).sum::<f64>() / v.len() as f64,
             v.iter().map(|s| s.y).sum::<f64>() / v.len() as f64)
        };
        let (ex, ey) = mean(&|s| s.exact);
        let (cx, cy) = mean(&|s| !s.exact);
        let d = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
        assert!(d < 0.4, "player {} : clics loin des positions (d={d:.2}) — mapping suspect", h.player_id);
    }
}
```
> Ce test est le vrai garde-fou du mapping (le Step 3 ne l'était pas). Validation finale = vérif visuelle Chunk 4.
Copier le replay : `mkdir -p crates/storm-replay-viewer/tests/data && cp crates/storm-stats/tests/data/silver-city-aram.StormReplay crates/storm-replay-viewer/tests/data/`

- [ ] **Step 4 : Vérifier l'échec de compilation/tests**

Run : `cargo test -p storm-replay-viewer`
Expected : FAIL (`build_model` / `ViewerModel` introuvables).

- [ ] **Step 5 : Implémenter l'extraction** — `src/extract.rs` + `src/lib.rs`

`src/lib.rs` :
```rust
mod extract;
mod model;
pub use model::*;

use storm_replay::Replay;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("replay: {0}")]
    Replay(#[from] storm_replay::Error),
    #[error("donnée manquante: {0}")]
    Missing(&'static str),
}

/// Projette un replay en modèle visionneuse (géométrie seule, clé playerId replay).
pub fn build_model(replay: &Replay) -> Result<ViewerModel, Error> {
    extract::build(replay)
}

/// Mapping autoritaire `playerId (tracker, 1..=10) → toon_handle` — pour que le serveur
/// attache les métadonnées joueurs de Postgres. Dérivé de `SPlayerSetupEvent` (m_playerId/m_slotId)
/// croisé avec `details().players[*].working_set_slot_id → toon_handle`. Réutilisé par Chunk 2.
pub fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, Error> {
    extract::player_toons(replay)
}
```

`src/extract.rs` — logique (référence, l'implémenteur ajuste au besoin) :
```rust
use crate::model::*;
use crate::Error;
use std::collections::HashMap;
use storm_replay::{Replay, Value};

fn loop_to_sec(gameloop: i64) -> f64 { (gameloop as f64 - LOOP_OFFSET as f64) / LOOPS_PER_SEC }

pub(crate) fn build(replay: &Replay) -> Result<ViewerModel, Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;

    // 1) Étendue de carte via SStatGameEvent{GameStart}.MapSizeX/Y (×4096)
    let (msx, msy) = map_size(&tracker).ok_or(Error::Missing("MapSize"))?;
    let (map_w, map_h) = (msx / FIXED, msy / FIXED); // en tuiles
    let norm_tile = |x: i64, y: i64| (x as f64 / map_w, y as f64 / map_h);
    let norm_world = |x: i64, y: i64| (x as f64 / msx, y as f64 / msy); // coords ×4096

    // 2) unité(tagIndex) → (player, isHero) depuis SUnitBornEvent (recycle: dernier gagne)
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

    // 4) densification via game events SCmd*/TargetPoint clé par _userid → playerId.
    //    Mapping AUTORITAIRE via SPlayerSetupEvent (m_userId → m_playerId) — PAS l'hypothèse
    //    fragile « slot+1 ». `user_to_player` construit une seule fois depuis le tracker.
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

    // 5) fusion samples (exacts + commande), tri par t croissant, quantif 3 décimales (q3),
    //    dédup des points quasi-immobiles (delta < EPS depuis le précédent retenu) pour tenir le
    //    budget payload (centaines de Ko). Voir Step 7 pour l'assemblage testé.
    // 6) intervalles de vie depuis Born(spawn)/Died/Revived de l'unité-héros du joueur
    // 7) deaths depuis SUnitDiedEvent des unités-héros suivies
    //    x,y = norm_tile(m_x,m_y) ; victim = unit_player[m_unitTagIndex].0 ;
    //    killer = m_killerPlayerId si ∈1..10 sinon None
    // 8) heroes triés par player_id ; warnings = vec![] (US-7 plus tard)

    // … (assembler ViewerModel ; voir Steps 6-9 pour la vie & les morts testées)
    todo!("assembler après Steps 6-9")
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

// Quantif à 3 décimales (stabilise le golden + réduit le payload).
fn q3(v: f64) -> f64 { (v * 1000.0).round() / 1000.0 }

// AUTORITAIRE : m_userId (des game events `_userid`) → m_playerId tracker (1..=10),
// depuis SPlayerSetupEvent (m_type == 1 = joueur). Remplace l'hypothèse « slot+1 ».
fn user_to_player(tracker: &[Value]) -> HashMap<i64, i64> {
    let mut m = HashMap::new();
    for e in tracker {
        if event_name(e) != "SPlayerSetupEvent" { continue; }
        let (Some(uid), Some(pid)) = (field_int(e, "m_userId"), field_int(e, "m_playerId")) else { continue; };
        if (1..=10).contains(&pid) { m.insert(uid, pid); }
    }
    m
}

// playerId (1..=10) → toon_handle : SPlayerSetupEvent (m_playerId → m_slotId) croisé avec
// details().players[*].working_set_slot_id → toon_handle. Utilisé par le serveur (Chunk 2).
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
> Notes d'implémentation :
> - Vérifier les champs exacts de `ReplayDetails`/`PlayerDetails` dans `crates/storm-replay/src/lib.rs` (`players`, `working_set_slot_id`, `toon_handle`).
> - Si `visit_game_events` fournit une `Value` propriétaire (par valeur), adapter la signature de la closure (`|ev: Value|` + `ev.field(...)`).
> - `SPlayerSetupEvent` a été observé dans un vrai replay : `{m_playerId, m_slotId, m_userId, m_type}`, 10 entrées, `m_userId == m_slotId == m_playerId-1` ici — mais **ne pas** re-hardcoder cette égalité, lire les champs (elle ne tient pas toujours, ex. observateurs/rejoins).

- [ ] **Step 6 : Écrire le test des intervalles de vie (échoue)** — ajouter à `tests/extract.rs`

```rust
#[test]
fn life_intervals_ordered_and_bounded() {
    let m = model();
    for h in &m.heroes {
        assert!(!h.life.is_empty(), "player {} sans intervalle de vie", h.player_id);
        for iv in &h.life { assert!(iv.from <= iv.to); }
        // intervalles strictement croissants et non chevauchants
        for w in h.life.windows(2) { assert!(w[0].to <= w[1].from); }
    }
}
```

- [ ] **Step 7 : Compléter `build` — fusion samples + vie + morts (remplace le `todo!()`)**

Cette étape assemble tout et retire le `todo!()` du Step 5 :
1. **Fusion samples** : pour chaque player, concaténer `samples[p]` (exacts) + `cmd_samples[p]`, trier par `t` croissant. **Dédup** : ignorer un sample dont `(|dx|,|dy|)` vs le dernier retenu est `< EPS` (`const EPS: f64 = 0.004;`) **et** de même `exact` — garde toujours les samples exacts. Clamp final `[0,1]`.
2. **Vie** : intervalle ouvert au premier Born de l'unité-héros du player (spawn), fermé au `t` du `SUnitDiedEvent`, ré-ouvert au `SUnitRevivedEvent` (résoudre l'unité → player via `unit_player`). Clore le dernier intervalle à `meta.duration_sec`. Intervalles triés, non chevauchants.
3. **Morts** : un `Death` par `SUnitDiedEvent` dont `m_unitTagIndex` ∈ unités-héros suivies. `x,y = q3(norm_tile(m_x,m_y))`, `victim_player_id = unit_player[idx].0`, `killer_player_id = m_killerPlayerId` si ∈ 1..=10 sinon `None`.
4. Assembler `ViewerModel { meta, heroes (triés par player_id), deaths (triés par t), warnings: vec![] }`.

- [ ] **Step 8 : Golden-JSON — écrire le test (échoue)**

```rust
#[test]
fn golden_json_stable() {
    let m = model();
    // exclure viewerVersion du comparé (bump volontaire → régénérer le golden)
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

- [ ] **Step 9 : Générer le golden + faire passer toute la suite**

Run : `UPDATE_GOLDEN=1 cargo test -p storm-replay-viewer golden_json_stable` puis `cargo test -p storm-replay-viewer`
Expected : PASS (tous). Puis `cargo clippy -p storm-replay-viewer -- -D warnings`.
Inspecter le golden à l'œil : 10 héros, samples croissants en `t`, coords plausibles.

- [ ] **Step 10 : Commit**
```bash
git add Cargo.toml crates/storm-replay-viewer
git commit -m "feat(viewer): crate storm-replay-viewer — extraction positionnelle → modèle JSON normalisé"
```

---

## Chunk 2 — Endpoint serveur `/api/matches/{id}/replay2d`

**Responsabilité :** décoder le replay archivé à la volée (+ cache disque), appeler `build_model`, fusionner les métadonnées joueurs (name/hero/team/win) depuis Postgres par `playerId`, émettre le JSON final.

**Fichiers :**
- Modifier : `crates/storm-codex-server/Cargo.toml` (dép `storm-replay-viewer`)
- Créer : `crates/storm-codex-server/src/replay2d.rs`
- Modifier : `crates/storm-codex-server/src/main.rs` (`mod replay2d;` + route)

**Décisions :**
- Route : `GET /api/matches/{id}/replay2d` (cohérent avec `/api/matches/{id}/raw`).
- Cache disque : réutiliser `s.cfg.raw_cache_dir`, fichier `{id}-replay2d-v{VIEWER_VERSION}.json`, mêmes `filetime_now`/`enforce_lru` que `raw.rs` (factoriser si trivial, sinon dupliquer le pattern — DRY léger, ne pas sur-abstraire).
- Métadonnées joueurs : la géométrie est clé par `playerId` tracker (1..=10). Le serveur mappe
  `playerId → toon_handle` via le helper **autoritaire** `player_toons()` du crate (croise
  `SPlayerSetupEvent.m_slotId` avec `details.working_set_slot_id`) — **PAS** « index details+1 ». Il joint
  ensuite `match_players` par `toon_handle` et émet `players:[{playerId, name, hero, team, win}]`. La
  couleur d'univers et le portrait sont dérivés **côté front** (`universeColor(hero)`, `heroIcon(hero)`),
  donc **non** inclus dans le payload.

- [ ] **Step 1 : Écrire un test d'intégration serveur (échoue)** — `crates/storm-codex-server/tests/replay2d.rs` si un harnais de test serveur existe ; sinon, test manuel documenté (voir Step 5). Vérifier d'abord la présence d'un pattern de test dans `crates/storm-codex-server/tests/`. S'il n'y en a pas, sauter le test automatisé serveur (couvert par le crate en Chunk 1 + la vérif E2E du Chunk 4) et cocher cette étape comme « N/A — pas de harnais serveur ».

- [ ] **Step 2 : Implémenter `replay2d.rs`**

Structure (miroir de `raw.rs`) — le `todo!()` est remplacé par l'implémentation des items 1→6 :
```rust
use crate::AppState;
use axum::{extract::{Path, State}, http::{header, StatusCode}, response::{IntoResponse, Response}};
use std::collections::HashMap;
use std::path::PathBuf;
use storm_replay::Replay;
use storm_replay_viewer::{build_model, player_toons, ViewerModel, VIEWER_VERSION};

pub async fn get_replay2d(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    // 1) archived_path : même requête que raw.rs (uploads WHERE match_id=$1 AND status='parsed').
    // 2) cache: raw_cache_dir/{id}-replay2d-v{VIEWER_VERSION}.json → hit : filetime_now + servir.
    // 3) miss : spawn_blocking qui rend BOTH la géométrie ET le mapping playerId→toon
    //    (le Replay est consommé dans la closure, donc on en sort tout ce dont l'async a besoin) :
    //      let r = Replay::open(&archived)?;
    //      let model = build_model(&r)?;              // meta/heroes/deaths/warnings
    //      let toons = player_toons(&r)?;             // Vec<(playerId, toon_handle)>
    //      (model, toons)
    // 4) requête async match_players → HashMap<toon, (name,hero,team,win)> :
    //      SELECT toon_handle, name, hero, team, win FROM match_players WHERE match_id = $1
    // 5) players[] = pour chaque (playerId, toon) de `toons`, joindre les métadonnées du HashMap :
    //      { playerId, name, hero, team, win }  (universeColor/portrait dérivés côté front)
    // 6) JSON final = merge du modèle + players via serde_json (voir helper ci-dessous),
    //    sérialiser, écrire cache, enforce_lru (pattern raw.rs), servir.
    todo!("implémenter items 1→6")
}
```
Détails :
- **Ordre/identité joueurs = autoritaire**, PAS « index details+1 ». Le crate fournit `player_toons()`
  (playerId tracker → toon via `SPlayerSetupEvent.m_slotId` × `details.working_set_slot_id`). Le serveur
  joint `match_players` par `toon_handle`.
- `build_model` + `player_toons` tournent dans le **même** `spawn_blocking` (un seul décodage, ~130 ms) ;
  la requête `match_players` reste async, hors blocking.
- **Merge JSON** : `ViewerModel` sérialise `{meta, heroes, deaths, warnings}` ; le serveur y insère
  `players`. Helper :
  ```rust
  fn merge(model: &ViewerModel, players: serde_json::Value) -> serde_json::Value {
      let mut v = serde_json::to_value(model).unwrap_or_default();
      if let Some(o) = v.as_object_mut() { o.insert("players".into(), players); }
      v
  }
  ```
- JSON final servi = `{ meta, players, heroes, deaths, warnings }`.

- [ ] **Step 3 : Câbler la route** — `main.rs`
```rust
mod replay2d;
// … dans le Router :
.route("/api/matches/{id}/replay2d", get(replay2d::get_replay2d))
```
Ajouter la dép dans `crates/storm-codex-server/Cargo.toml` :
```toml
storm-replay-viewer = { path = "../storm-replay-viewer" }
```

- [ ] **Step 4 : Compiler + clippy**
Run : `cargo build -p storm-codex-server && cargo clippy -p storm-codex-server -- -D warnings`
Expected : OK.

- [ ] **Step 5 : Vérif manuelle contre la DB backfillée** (le box tourne le soir, ou Postgres Docker local `docker-compose.dev.yml`)
```bash
# récupérer un id de match existant
curl -s localhost:5102/api/matches | python3 -c "import sys,json;print(json.load(sys.stdin)[0]['id'])"
curl -s localhost:5102/api/matches/<ID>/replay2d | python3 -m json.tool | head -40
```
Expected : `meta.mapName` correct, `players` 10 entrées avec `hero`, `heroes` 10 tracks, `deaths` non vide. Deuxième appel instantané (cache hit — vérifier le fichier dans `raw_cache_dir`).

- [ ] **Step 6 : Commit**
```bash
git add crates/storm-codex-server
git commit -m "feat(server): endpoint /api/matches/{id}/replay2d (décodage à la demande + cache + méta joueurs)"
```

---

## Chunk 3 — Onglet front « Replay 2D » (canvas + scrub)

**Responsabilité :** charger le modèle, afficher la minimap, 10 pastilles héros, barre de scrub, `seek(t)` client (lerp + vie), atténuation morts + marqueurs de mort. Onglet dans `MatchDetail`.

**Fichiers :**
- Modifier : `web/package.json` (+ vitest) ; `web/vite.config.ts` (config test)
- Créer : `web/src/replay2d.ts` (types + `seek(t)` pur + `sampleAt`)
- Créer : `web/src/replay2d.test.ts` (vitest)
- Créer : `web/src/components/Replay2D.tsx` (canvas + scrub)
- Modifier : `web/src/api.ts` (`fetchReplay2d` + types)
- Modifier : `web/src/pages/MatchDetail.tsx` (onglet)

- [ ] **Step 1 : Ajouter vitest** — `web/package.json`
```json
"scripts": { "dev": "vite", "build": "tsc -b && vite build", "preview": "vite preview", "test": "vitest run" },
"devDependencies": { "vitest": "^2.1.0", ...existant }
```
`web/vite.config.ts` : ajouter `test: { environment: 'node' }` (import depuis `vitest/config` si besoin). Puis `cd web && npm install`.

- [ ] **Step 2 : Écrire le test de `seek` (échoue)** — `web/src/replay2d.test.ts`
```ts
import { describe, it, expect } from "vitest";
import { sampleAt, type HeroTrack } from "./replay2d";

const h: HeroTrack = {
  playerId: 1,
  samples: [ {t:0,x:0,y:0,exact:true}, {t:10,x:1,y:1,exact:false} ],
  life: [ {from:0,to:6}, {from:8,to:10} ],
};

describe("sampleAt", () => {
  it("lerp entre deux samples vivants", () => {
    const p = sampleAt(h, 5); // vivant (0..6)
    expect(p).not.toBeNull();
    expect(p!.x).toBeCloseTo(0.5); expect(p!.alive).toBe(true);
  });
  it("fige la position pendant l'intervalle mort (pas de lerp à travers)", () => {
    const p = sampleAt(h, 7); // mort (6..8)
    expect(p!.alive).toBe(false);
    expect(p!.x).toBeCloseTo(0.6); // dernière position vivante à t=6, pas 0.7
  });
  it("borne avant le premier / après le dernier sample", () => {
    expect(sampleAt(h, -1)!.x).toBeCloseTo(0);
    expect(sampleAt(h, 99)!.x).toBeCloseTo(1);
  });
});
```

- [ ] **Step 3 : Vérifier l'échec**
Run : `cd web && npm run test`
Expected : FAIL (`sampleAt` introuvable).

- [ ] **Step 4 : Implémenter `replay2d.ts`**
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

/** Fin de vie la plus récente avant/à t (= instant de la mort). null si aucune (avant spawn). */
function lastAliveEnd(life: Interval[], t: number): number | null {
  let best: number | null = null;
  for (const iv of life) if (iv.to <= t && (best === null || iv.to > best)) best = iv.to;
  return best;
}

/** Interpolation linéaire de la position à l'instant `t` (bornée au 1er/dernier sample). */
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

/** Position + état vivant/mort d'un héros à l'instant t (pure). null si pas de samples.
 *  Mort : on FIGE la position à l'instant de la mort (fin du dernier intervalle vivant) — pas de
 *  lerp à travers le trou mort→respawn (le respawn est à la base, loin du lieu de mort). */
export function sampleAt(h: HeroTrack, t: number): { x: number; y: number; alive: boolean } | null {
  const s = h.samples;
  if (!s.length) return null;
  const alive = aliveAt(h.life, t);
  const et = alive ? t : (lastAliveEnd(h.life, t) ?? t); // temps effectif : mort → instant du décès
  const p = interp(s, et);
  return { x: p.x, y: p.y, alive };
}

/** Morts « récentes » à t (marqueur qui persiste ~4 s de scrub). */
export function deathsNear(deaths: Death[], t: number, window = 4): Death[] {
  return deaths.filter((d) => t >= d.t && t <= d.t + window);
}
```

- [ ] **Step 5 : Vérifier le passage des tests**
Run : `cd web && npm run test`
Expected : PASS.

- [ ] **Step 6 : Ajouter `fetchReplay2d`** — `web/src/api.ts`
```ts
import type { Replay2D } from "./replay2d";
export const fetchReplay2d = (id: string | number) => get<Replay2D>(`/api/matches/${id}/replay2d`);
```
(réutilise le `get<T>` existant.)

- [ ] **Step 7 : Composant canvas** — `web/src/components/Replay2D.tsx`

Éléments :
- `useQuery(["replay2d", id], () => fetchReplay2d(id))`.
- État `t` (secondes) piloté par un `<input type="range" min=0 max=durationSec step=0.1>`.
- Fond : `<img>` via `mapImage(meta.mapName)` ; si `null`, fond dégradé (classe existante). Dessiner l'image dans le canvas (ou la poser en CSS background sous le canvas transparent).
- Boucle de dessin dans un `useEffect([t, data])` : pour chaque héros, `sampleAt(track, t)` → px `cx = x*W`, `cy = (1 - y)*H` (**flip Y** : le monde monte, le canvas descend). Pastille remplie couleur d'équipe (team 0 = bleu, team 1 = rouge — réutiliser les tokens/classes équipe existants), anneau `universeColor(hero)`, portrait `heroIcon(hero)` clippé en cercle (fallback initiales). Héros mort → `globalAlpha` réduit + croix.
- Marqueurs de mort : `deathsNear(deaths, t)` → petit ✕ à `(x, 1-y)`.
- Légende joueurs (nom + héros) à côté, couleur d'équipe.

Rendu **canvas 2D** (pas SVG). Redimensionnement : canvas carré responsive (max-width conteneur), coord map ×W/H.

> ⚠️ **Flip Y** : si les héros apparaissent inversés verticalement à la vérif visuelle (Chunk 4), c'est le signe du flip — ajuster `cy = y*H` vs `(1-y)*H` une seule fois.

- [ ] **Step 8 : Intégrer l'onglet** — `web/src/pages/MatchDetail.tsx`
- `const [tab, setTab] = useState<"score" | "replay2d">("score")` (le fichier importe déjà `useState`).
- Deux boutons d'onglet (classes existantes `bdg`/carte) : « Score » / « Replay 2D ».
- `{tab === "replay2d" ? <Replay2D id={id} /> : <…score existant…>}`. Ne pas casser l'affichage score actuel.

- [ ] **Step 9 : Build front**
Run : `cd web && npm run build`
Expected : `✓ built` (⚠️ un échec `tsc` fait échouer silencieusement — vérifier la ligne `✓ built`, cf. STATUS.md).

- [ ] **Step 10 : Commit**
```bash
git add web
git commit -m "feat(web): onglet Replay 2D — canvas, scrub, seek(t) client, vivant/mort + marqueurs de mort"
```

---

## Chunk 4 — Vérification visuelle E2E + calage minimap

**Responsabilité :** prouver sur un vrai match que les héros sont bien placés sur la bonne carte et que le scrub est instantané ; corriger orientation/recadrage si besoin. **Critère d'acceptation MVP-1.**

- [ ] **Step 1 : Lancer le stack** (Postgres backfillé + serveur + front). Utiliser les outils `preview_*` (jamais Bash pour le serveur). Créer `.claude/launch.json` si absent (serveur Rust + `WEB_DIR`, ou `vite` proxy vers l'API). Ouvrir un match réel → onglet « Replay 2D ».

- [ ] **Step 2 : Vérifs visuelles** (preview_screenshot + preview_console_logs + preview_network) :
  - La bonne minimap s'affiche (ou fallback dégradé pour cartes ARAM sans image — attendu).
  - 10 pastilles visibles, réparties en 2 équipes, dans les limites de la carte.
  - Scrub de 0 → fin : les pastilles bougent de façon cohérente ; à la mort d'un héros connu, la pastille s'atténue et un ✕ apparaît au bon endroit.
  - Seek instantané (pas de latence perceptible ; `preview_network` : un seul GET `/replay2d`, aucune requête par déplacement du curseur).

- [ ] **Step 3 : Corriger le calage si nécessaire**
  - Inversion verticale → basculer le flip Y (Chunk 3 Step 7).
  - Héros décalés/écrasés en bordure → `MapSize` inclut une bordure injouable : ajouter une constante de recadrage par carte (marge en fraction) appliquée à la normalisation côté crate ou au dessin côté front. Documenter la valeur. **Ne pas** sur-ajuster : viser « clairement lisible », pas le pixel.

- [ ] **Step 4 : Vérif de non-régression** : l'onglet « Score » existant fonctionne toujours ; les autres pages inchangées.

- [ ] **Step 5 : Mettre à jour `docs/STATUS.md`** — ajouter une section « Visionneuse 2D (MVP-1) : livré + vérifié » (date, ce qui marche, captures, HP/mana explicitement hors périmètre, fast-follows restants : animation play/pause, structures, kill-feed).

- [ ] **Step 6 : Commit final + proposer la finalisation de branche**
```bash
git add docs/STATUS.md
git commit -m "docs(status): visionneuse 2D MVP-1 livrée + vérifiée E2E"
```
Puis invoquer superpowers:finishing-a-development-branch (merge/PR).

---

## Notes transverses
- **DRY / YAGNI :** pas de snapshots, pas d'animation, pas de structures/kill-feed en MVP-1 (fast-follows). Le cache réutilise le pattern `raw.rs`, ne pas créer de 4e étage.
- **Perf :** budget de calcul = un parse (~130 ms) au 1er accès, puis cache. Seek < 50 ms garanti (100 % client). Payload : ne garder que les `TargetPoint` de déplacement + quantifier les floats (~3 décimales) pour rester en centaines de Ko.
- **Réutilisation :** noms/portraits/couleurs héros viennent de la projection Postgres (correction shuffle ARAM incluse) et des helpers front — la visionneuse ne re-résout jamais l'identité héros.
- **Risque principal (calibration) :** dé-risqué dès le Chunk 1 (test `all_coords_normalized`) et confirmé visuellement au Chunk 4.
