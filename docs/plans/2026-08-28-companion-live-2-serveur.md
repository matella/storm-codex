# Companion live — Plan 2 : serveur (lobby, builds, enrichissement)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** exposer côté serveur tout ce dont la page companion aura besoin — ingérer un lobby live,
l'enrichir avec l'historique de l'archive, gérer une bibliothèque de builds, et relier la partie
terminée au lobby pour le debrief.

**Architecture:** `storm-lobby` (plan 1, mergé) fournit les BattleTags et les équipes. Le serveur
résout ces BattleTags en identités applicatives **contre l'archive elle-même**
(`match_players.name` + `data->>'tag'` → `toon_handle`), puis agrège l'historique en une seule
requête `jsonb_build_object`, comme le fait déjà `read::get_player`. L'état du lobby courant est un
singleton persisté, calqué sur `draft_live`.

**Tech Stack:** Rust 2021, axum 0.8, sqlx 0.8 (requêtes runtime, pas de macros vérifiées),
PostgreSQL, `tokio::sync::broadcast` pour le WS.

## Périmètre

**Ce plan couvre le serveur uniquement.** Le front (`/companion`, `/builds`) est le plan 3 ; le
watcher `client-rs` (repo Hots-Overlay) est le plan 4 et exige le PC de jeu. Chaque route livrée
ici est testable au curl sans une ligne de front.

## Global Constraints

- Rust 2021 ; lints workspace : `clippy::unwrap_used` = deny, `clippy::expect_used` = warn.
  Aucun `unwrap()` hors tests ; les fichiers de test ouvrent par
  `#![allow(clippy::expect_used, clippy::unwrap_used)]`.
- **sqlx en requêtes runtime** (`sqlx::query`, `query_scalar`), jamais les macros vérifiées à la
  compilation — c'est le choix déjà fait partout dans `crates/storm-codex-server`.
- Jamais d'interpolation de chaîne dans du SQL : uniquement des binds `$1`, `$2`…
- `storm-lobby` reste **pur** : aucune I/O, aucune dépendance runtime à `storm-replay`/`storm-stats`.
- Budget de la spec programme : `/api/lobby` doit tenir **p95 < 100 ms**.
- Les mutations d'état de lobby diffusent sur le canal `/ws` existant, comme `draft.updated`.
- Commits conventionnels. Branche : `feat/companion-live-serveur`.
- **Aucune modification du box** dans ce plan : tout se développe contre le Postgres de dev
  (`docker-compose.dev.yml`).

## Décisions produit actées par l'opérateur (2026-08-28)

1. **Réassignation d'équipe par joueur** (et non un simple bouton d'inversion) : la mesure du plan 1
   montre que l'inversion ne répare que 5,3 % des cas fautifs, contre 94,7 % d'ordres qui ne portent
   aucune information d'équipe. L'API doit donc permettre de fixer l'équipe **de chaque joueur**.
2. **Table `hash .s2ma → carte` dérivée de l'archive**, pour supprimer la saisie manuelle de la
   carte. C'est la tâche 1.

## Fichiers

| Fichier | Responsabilité |
|---|---|
| `crates/storm-lobby/src/lib.rs` | + extraction des hashes `.s2ma`, + résolution de la carte via table embarquée |
| `crates/storm-lobby/src/maps.rs` | table `hash → nom de carte`, générée, committée |
| `crates/storm-lobby/examples/derive_maps.rs` | dérive `maps.rs` depuis un dossier de replays |
| `crates/storm-codex-server/migrations/0009_companion.sql` | tables `builds`, `lobby_live`, index de résolution |
| `crates/storm-codex-server/src/builds.rs` | CRUD bibliothèque + import depuis un match |
| `crates/storm-codex-server/src/lobby/mod.rs` | type d'état enrichi + singleton |
| `crates/storm-codex-server/src/lobby/store.rs` | persistance `lobby_live` (calque de `draft/store.rs`) |
| `crates/storm-codex-server/src/lobby/enrich.rs` | résolution BattleTag→identité + agrégats d'archive |
| `crates/storm-codex-server/src/lobby/api.rs` | routes `/api/lobby*` |
| `crates/storm-codex-server/src/main.rs` | déclaration des routes + champ `lobby` dans `AppState` |

---

## Task 1 : Carte déduite des hashes `.s2ma`

**Files:**
- Modify: `crates/storm-lobby/src/lib.rs`
- Create: `crates/storm-lobby/src/maps.rs`
- Create: `crates/storm-lobby/examples/derive_maps.rs`
- Test: `crates/storm-lobby/tests/oracle.rs` (ajout d'un test)

**Interfaces:**
- Produit : `Lobby { players: Vec<LobbyPlayer>, map: Option<String> }` — le champ `map` est **ajouté**
  (`Lobby` est `#[non_exhaustive]`, donc c'est non cassant). `map` porte le nom de carte tel que
  `matches.map` le stocke, ou `None` si aucun hash connu n'est trouvé.
- Consomme : rien de nouveau.

Le rapport de format (`docs/research/2026-08-27-lobby-format.md`, Q5) établit que le blob contient
des chemins de cache vers des fichiers `.s2ma` identifiés par hash, mais aucun nom de carte. Chaque
replay archivé porte à la fois ces hashes **et** sa carte connue : la correspondance se dérive donc
de l'archive, une fois pour toutes.

- [ ] **Étape 1 : Écrire le test d'extraction des hashes (rouge)**

Ajouter à `crates/storm-lobby/tests/oracle.rs` :

```rust
/// Le blob contient des chemins de cache Battle.net vers des fichiers `.s2ma` (les cartes),
/// identifiés par un hash de 32 caractères hexadécimaux. Ils sont la seule piste de carte du
/// format (rapport de format, Q5).
#[test]
fn les_hashes_de_carte_sont_extraits() {
    let path = data("crates/storm-stats/tests/data/silver-city-aram.StormReplay");
    let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
    let blob = replay.battlelobby_raw().expect("stream battlelobby");
    let hashes = storm_lobby::map_hashes(&blob);
    assert!(
        !hashes.is_empty(),
        "aucun hash .s2ma extrait — le rapport de format en documente 9"
    );
    assert!(
        hashes.iter().all(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit())),
        "hash mal formé : {hashes:?}"
    );
}
```

- [ ] **Étape 2 : Lancer le test, vérifier l'échec**

Run: `cargo test -p storm-lobby --test oracle les_hashes`
Expected: FAIL à la compilation — `map_hashes` n'existe pas.

- [ ] **Étape 3 : Implémenter l'extraction**

Dans `crates/storm-lobby/src/lib.rs`, ajouter :

```rust
/// Hashes des fichiers `.s2ma` (cartes) référencés par le blob, dans l'ordre d'apparition,
/// dédupliqués. Le blob ne nomme jamais la carte : ces hashes sont la seule piste, et leur
/// correspondance vers un nom vit dans [`maps`] (dérivée de l'archive).
#[must_use]
pub fn map_hashes(bytes: &[u8]) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // Même motif que `parse()` : le littéral est vérifié par les tests, donc l'échec de
    // compilation de la regex est impossible en usage normal — mais pas de `unwrap()` nu.
    let re = RE.get_or_init(|| match regex::Regex::new(r"([0-9a-f]{32})\.s2ma") {
        Ok(r) => r,
        Err(e) => unreachable!("regex .s2ma invalide : {e}"),
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

Note : `unreachable!` est le même échappatoire que `parse()` utilise déjà pour sa propre regex —
il satisfait `clippy::unwrap_used = deny` sans masquer d'erreur réelle, puisque le motif est un
littéral couvert par les tests.

- [ ] **Étape 4 : Vérifier le vert**

Run: `cargo test -p storm-lobby --test oracle les_hashes`
Expected: PASS

- [ ] **Étape 5 : Écrire l'outil de dérivation**

`crates/storm-lobby/examples/derive_maps.rs` :

```rust
//! Dérive la table `hash .s2ma → nom de carte` depuis un dossier de replays archivés.
//!
//! Principe : chaque replay donne (carte connue via le parse complet, hashes du blob). Un hash est
//! retenu pour une carte s'il apparaît dans TOUS les replays de cette carte et dans AUCUN replay
//! d'une autre carte. Les hashes communs à plusieurs cartes (assets partagés) sont donc écartés.
//!
//! Usage : cargo run --release -p storm-lobby --example derive_maps -- <dossier> > src/maps.rs

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: derive_maps <dossier>"))?
        .into();

    // carte → nombre de replays, et hash → cartes où il apparaît, et (carte,hash) → occurrences
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

    // Un hash est discriminant s'il n'apparaît que sur une carte, et sur TOUS ses replays.
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

    println!("//! Table `hash .s2ma → nom de carte`, GÉNÉRÉE — ne pas éditer à la main.");
    println!("//!");
    println!("//! Produite par `cargo run --release -p storm-lobby --example derive_maps -- <dossier>`");
    println!("//! sur l'archive du box. Un hash n'est retenu que s'il apparaît sur une seule carte");
    println!("//! ET sur tous les replays de cette carte.");
    println!();
    println!("/// `(hash, nom de carte)`, trié par hash — les noms sont ceux que `matches.map` stocke.");
    println!("pub(crate) const MAP_BY_HASH: &[(&str, &str)] = &[");
    for (h, c) in &table {
        println!("    ({h:?}, {c:?}),");
    }
    println!("];");

    eprintln!("cartes couvertes : {}", 
        table.iter().map(|(_, c)| c).collect::<BTreeSet<_>>().len());
    eprintln!("hashes retenus   : {}", table.len());
    eprintln!("cartes vues      : {}", replays_par_carte.len());
    Ok(())
}
```

- [ ] **Étape 6 : Générer la table depuis l'archive**

L'archive vit dans un volume Docker sur le box. Récupération en **lecture seule** (le motif du
runbook `docs/runbooks/2026-07-09-visionneuse-2d-verif-box.md`) :

```bash
mkdir -p /tmp/lobby-archive && \
ssh matella@192.168.129.85 "docker cp storm-codex-server:/data/archive -" | tar -xf - -C /tmp/lobby-archive && \
ls /tmp/lobby-archive/archive | wc -l
```

Expected: ~3300 fichiers. Puis (plusieurs minutes — arrière-plan ou timeout élevé) :

```bash
cargo run --release -p storm-lobby --example derive_maps -- /tmp/lobby-archive/archive \
  > crates/storm-lobby/src/maps.rs
```

Regarder la sortie d'erreur : « cartes couvertes » doit approcher « cartes vues ». Si moins de la
moitié des cartes sont couvertes, **ne pas forcer** : consigner le résultat et remonter à
l'opérateur — la carte restera une saisie manuelle, ce que le plan 3 gère déjà.

- [ ] **Étape 7 : Brancher la table dans `parse()`**

Dans `crates/storm-lobby/src/lib.rs` :

```rust
mod maps;

// … dans la construction du Lobby, après les joueurs :
let map = map_hashes(bytes)
    .iter()
    .find_map(|h| {
        maps::MAP_BY_HASH
            .iter()
            .find(|(hash, _)| hash == h)
            .map(|(_, nom)| (*nom).to_string())
    });
```

et ajouter le champ à la structure :

```rust
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Carte, déduite des hashes `.s2ma` via une table dérivée de l'archive. `None` si aucun hash
    /// connu — le format ne nomme jamais la carte.
    pub map: Option<String>,
}
```

- [ ] **Étape 8 : Test de bout en bout de la carte**

Ajouter à `crates/storm-lobby/tests/oracle.rs` :

```rust
/// La carte déduite des hashes doit correspondre à celle du parse complet, quand elle est déduite.
/// Une carte `None` est acceptable (hash inconnu de la table) ; une carte FAUSSE ne l'est pas.
#[test]
fn la_carte_deduite_ne_ment_jamais() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let filename = label;
        let out = storm_stats::process_replay(&path, filename);
        assert_eq!(out.status, 1, "{label} : parse complet rejeté");
        let json = out.to_json();
        let attendue = json["match"]["map"].as_str().unwrap_or_default().to_string();

        if let Some(deduite) = lobby_of(&path).map {
            assert_eq!(deduite, attendue, "{label} : carte déduite fausse");
        }
    }
}
```

Run: `cargo test -p storm-lobby`
Expected: PASS (les 4 tests précédents + les 2 nouveaux).

- [ ] **Étape 9 : Nettoyer et committer**

```bash
rm -rf /tmp/lobby-archive
cargo clippy -p storm-lobby --all-targets -- -D warnings
git add crates/storm-lobby
git commit -m "feat(storm-lobby): déduire la carte des hashes .s2ma via une table dérivée de l'archive"
```

---

## Task 2 : Migration `0009` et bibliothèque de builds

**Files:**
- Create: `crates/storm-codex-server/migrations/0009_companion.sql`
- Create: `crates/storm-codex-server/src/builds.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (déclaration du module + routes)

**Interfaces:**
- Produit : les routes `GET /api/builds`, `POST /api/builds`, `PUT /api/builds/{id}`,
  `DELETE /api/builds/{id}`, `POST /api/builds/from-match`. La tâche 4 lira la table `builds` pour
  joindre le build par défaut au lobby ; la tâche 6 comparera `builds.picks` aux talents joués.
- Consomme : `AppState { db: PgPool, … }` (`main.rs:31`).

- [ ] **Étape 1 : Écrire la migration**

`crates/storm-codex-server/migrations/0009_companion.sql` :

```sql
-- Bibliothèque de builds. `picks` a EXACTEMENT la forme écrite par le parser dans
-- match_players.data.talents : {"Tier1Choice": "<talentTreeId>", ...}. C'est ce qui rend l'import
-- depuis un match et le diff post-game de simples comparaisons d'objets, sans mapping à inventer.
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
-- Invariant tenu par la base, pas par le code : au plus un build par défaut par héros.
CREATE UNIQUE INDEX builds_one_default_per_hero ON builds (hero_id) WHERE is_default;

-- Lobby courant. Singleton, calque exact de draft_live : tout l'état dans le JSON, écrasé à chaque
-- nouveau lobby. Aucun historique — le replay archivé reste la source de vérité.
CREATE TABLE lobby_live (
    id         INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Résolution BattleTag → toon_handle : le blob de lobby ne porte que "nom#discriminant", et
-- l'archive est la seule table de correspondance dont on dispose (cf. spec companion-live).
CREATE INDEX match_players_name_tag_idx
    ON match_players (lower(name), (data ->> 'tag'));
```

- [ ] **Étape 2 : Vérifier que la migration s'applique**

```bash
docker compose -f docker-compose.dev.yml up -d
cargo run -p storm-codex-server 2>&1 | head -20
```

Expected: le serveur démarre sans erreur de migration. Vérifier :

```bash
docker compose -f docker-compose.dev.yml exec -T postgres \
  psql -U postgres -d storm_codex -c '\d builds' | head -15
```

Expected: les 9 colonnes de `builds`.

- [ ] **Étape 3 : Écrire le module builds**

`crates/storm-codex-server/src/builds.rs` :

```rust
//! Bibliothèque de builds de talents. `picks` reprend la forme du parser
//! (`{TierNChoice: talentTreeId}`), ce qui permet d'importer un build depuis une partie jouée et de
//! diffuser « prévu vs pris » sans conversion. L'unicité du build par défaut est tenue par un index
//! partiel unique en base, pas par ce code.
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

/// Les mutations de la bibliothèque suivent la même garde que teams/collections : ouvertes si
/// aucun `ADMIN_TOKEN` n'est configuré (mode local par défaut de la spec suite), protégées sinon.
/// La lecture reste toujours ouverte, comme `list_teams`.
fn refus_admin() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin token requis" })),
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Filtre optionnel sur le héros (clé `dim_heroes.id`).
    pub hero: Option<String>,
}

/// `GET /api/builds` — la bibliothèque, éventuellement filtrée par héros.
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

/// `POST /api/builds` — créer un build. Marquer `is_default` retire d'abord le défaut existant du
/// même héros : sans ça, l'index partiel unique rejetterait l'insertion.
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

/// `PUT /api/builds/{id}` — remplacer un build. Même précaution sur le défaut que `create`.
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

/// `POST /api/builds/from-match` — amorcer un build depuis une partie jouée. C'est ce qui évite de
/// saisir 90 héros à la main : les talents de l'archive ont déjà la bonne forme.
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

- [ ] **Étape 4 : Déclarer le module et les routes**

Dans `crates/storm-codex-server/src/manage.rs`, rendre la garde réutilisable — elle est
aujourd'hui privée :

```rust
pub(crate) fn is_admin(h: &axum::http::HeaderMap, s: &AppState) -> bool {
```

(seul le mot-clé change ; le corps reste identique.)

Dans `crates/storm-codex-server/src/main.rs`, à côté des autres `mod` :

```rust
mod builds;
```

et dans le `Router::new()`, à la suite des routes `/api/collections` :

```rust
        .route("/api/builds", get(builds::list).post(builds::create))
        .route(
            "/api/builds/{id}",
            axum::routing::put(builds::update).delete(builds::delete),
        )
        .route("/api/builds/from-match", post(builds::from_match))
```

- [ ] **Étape 5 : Vérifier au curl, y compris l'invariant de la base**

Démarrer le serveur, puis :

```bash
# En dev sans ADMIN_TOKEN configuré, la garde laisse passer ; avec un token, ajouter
# -H "authorization: Bearer $ADMIN_TOKEN" aux trois mutations.
curl -s -XPOST localhost:8080/api/builds -H 'content-type: application/json' \
  -d '{"hero_id":"Tychus","name":"Anti-heal","picks":{"Tier1Choice":"TychusMasterAssassin"},"is_default":true}'
curl -s -XPOST localhost:8080/api/builds -H 'content-type: application/json' \
  -d '{"hero_id":"Tychus","name":"Poke","picks":{"Tier1Choice":"TychusOverdrive"},"is_default":true}'
curl -s 'localhost:8080/api/builds?hero=Tychus'
```

Expected: deux `{"id":N}`, puis une liste de **deux** builds dont **un seul** a `is_default: true`
(le second — le premier a été démarqué par la transaction). Si les deux sortent à `true`, ou si la
seconde insertion échoue en 500, la transaction de `create` est fautive.

Vérifier ensuite que l'index protège bien contre un contournement :

```bash
docker compose -f docker-compose.dev.yml exec -T postgres psql -U postgres -d storm_codex \
  -c "UPDATE builds SET is_default = true WHERE hero_id = 'Tychus';"
```

Expected: `ERROR: duplicate key value violates unique constraint "builds_one_default_per_hero"`.
C'est le comportement recherché — l'invariant tient même si un futur code l'oublie.

- [ ] **Étape 6 : Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server/migrations/0009_companion.sql crates/storm-codex-server/src/builds.rs crates/storm-codex-server/src/main.rs
git commit -m "feat(server): migration 0009 + bibliothèque de builds (CRUD, import depuis un match)"
```

---

## Task 3 : Ingestion du lobby live

**Files:**
- Create: `crates/storm-codex-server/src/lobby/mod.rs`
- Create: `crates/storm-codex-server/src/lobby/store.rs`
- Create: `crates/storm-codex-server/src/lobby/api.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (champ `lobby` dans `AppState`, chargement au
  démarrage, routes)
- Modify: `crates/storm-codex-server/Cargo.toml` (dépendance `storm-lobby`)

**Interfaces:**
- Produit : `LobbyState` (sérialisable, porte `schema_version`), les routes `POST /api/lobby`,
  `GET /api/lobby`, `DELETE /api/lobby`, et l'événement WS `lobby.detected`. La tâche 4 remplira le
  champ `players[].history` ; la tâche 5 mutera `hero`, `map` et les équipes ; la tâche 6 remplira
  `match_id`.
- Consomme : `storm_lobby::{parse, Lobby, LobbyPlayer}` (plan 1), `crate::AppState`.

- [ ] **Étape 1 : Ajouter la dépendance**

Dans `crates/storm-codex-server/Cargo.toml`, section `[dependencies]` :

```toml
storm-lobby = { path = "../storm-lobby" }
```

- [ ] **Étape 2 : Définir l'état**

`crates/storm-codex-server/src/lobby/mod.rs` :

```rust
//! Lobby live : état courant d'une partie en cours de chargement. Singleton persisté (calque de
//! `draft_live`) — le lobby courant écrase le précédent, il n'y a pas d'historique de lobbies : le
//! replay archivé reste la source de vérité (étage 1 des trois étages de données).
pub mod api;
pub mod enrich;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

/// Version du schéma de `lobby_live.state` — un état d'une version inconnue est ignoré au
/// démarrage plutôt que désérialisé de travers.
pub const LOBBY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayerState {
    pub name: String,
    pub discriminant: String,
    /// `"nom#1234"` — la clé d'identité issue du blob.
    pub battletag: String,
    /// Équipe : celle déduite par `storm-lobby`, ou celle fixée à la main (tâche 5).
    pub team: Option<u8>,
    /// `true` si l'équipe a été corrigée à la main — le front l'affiche différemment.
    #[serde(default)]
    pub team_manual: bool,
    /// Identité applicative, résolue contre l'archive. `None` = joueur jamais croisé.
    pub toon_handle: Option<String>,
    /// Agrégats d'archive (tâche 4). `null` tant que non enrichi ou si joueur inconnu.
    #[serde(default)]
    pub history: Option<J>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyState {
    pub schema_version: u32,
    pub detected_at: String,
    pub players: Vec<LobbyPlayerState>,
    /// Carte : déduite des hashes `.s2ma`, ou saisie à la main.
    pub map: Option<String>,
    #[serde(default)]
    pub map_manual: bool,
    /// Héros de l'opérateur — jamais dans le blob, toujours saisi (tâche 5).
    pub hero: Option<String>,
    /// Index dans `players` du joueur identifié comme l'opérateur, si trouvé.
    pub me: Option<usize>,
    /// Build suggéré + alternatives (tâche 4).
    #[serde(default)]
    pub build: Option<J>,
    /// Stats de l'opérateur sur ce héros / cette carte (tâche 4).
    #[serde(default)]
    pub me_stats: Option<J>,
    /// Rempli quand le replay correspondant est parsé (tâche 6) → bascule en debrief.
    #[serde(default)]
    pub match_id: Option<i64>,
    /// `parse_failed` quand le blob est illisible : la page affiche alors le sélecteur manuel
    /// plutôt qu'une page vide.
    #[serde(default)]
    pub status: Option<String>,
}

impl LobbyState {
    /// Construit l'état brut depuis un lobby décodé, avant enrichissement.
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

    /// État dégradé quand le blob n'a pas pu être décodé (nouveau build Blizzard). La page reste
    /// utilisable : l'opérateur saisit son héros, le build s'affiche.
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

- [ ] **Étape 3 : Persistance (calque de `draft/store.rs`)**

`crates/storm-codex-server/src/lobby/store.rs` :

```rust
//! Persistance du lobby courant (singleton `lobby_live`, id=1). Requêtes runtime, comme le reste
//! du serveur. Copie assumée du motif de `draft/store.rs` : deux singletons indépendants, chacun
//! avec son cycle de vie ; les factoriser coûterait plus en indirection qu'il ne rapporterait.
use crate::lobby::{LobbyState, LOBBY_SCHEMA_VERSION};
use sqlx::PgPool;

/// Charge l'état persistant. Un état d'une autre version de schéma est ignoré (→ pas de lobby).
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

/// Écrit l'état (upsert sur la ligne unique).
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

/// Efface le lobby courant.
pub async fn clear(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_live WHERE id = 1")
        .execute(db)
        .await
        .map(|_| ())
}
```

- [ ] **Étape 4 : Les routes d'ingestion**

`crates/storm-codex-server/src/lobby/api.rs` :

```rust
//! Routes du lobby live. `POST /api/lobby` reçoit les octets bruts du fichier de lobby (même
//! contrat que `/api/upload-raw` : `Bearer` + corps binaire), le décode, l'enrichit, le persiste et
//! diffuse `lobby.detected`. Le parseur vivant côté serveur, une casse de format Blizzard se
//! corrige par un redéploiement du box, sans jamais retoucher le binaire Windows.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value as J};

use crate::lobby::{store, LobbyState};
use crate::AppState;

/// Idempotence : deux POST du même fichier (le watcher redémarre, le jeu réécrit) ne doivent pas
/// écraser un état déjà enrichi ni rediffuser un événement. On compare l'ensemble des BattleTags.
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

/// `POST /api/lobby` — octets bruts, `Bearer` d'upload.
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
            // Pas une erreur HTTP : la page doit s'ouvrir quand même, avec le sélecteur manuel.
            tracing::warn!("lobby illisible ({} octets) : {e}", bytes.len());
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

/// `GET /api/lobby` — l'état enrichi courant, `204` si aucun lobby.
pub async fn get(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    match s.lobby.read().await.as_ref() {
        None => (StatusCode::NO_CONTENT, Json(J::Null)),
        Some(st) => (
            StatusCode::OK,
            Json(serde_json::to_value(st).unwrap_or(J::Null)),
        ),
    }
}

/// `DELETE /api/lobby` — fermer le companion.
pub async fn clear(State(s): State<AppState>) -> Json<J> {
    let _ = store::clear(&s.db).await;
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
}
```

- [ ] **Étape 5 : Extraire la validation de token pour la réutiliser**

Dans `crates/storm-codex-server/src/upload.rs`, la validation vit aujourd'hui inline dans `upload`
(lignes ~73-96). L'extraire en fonction publique et l'appeler depuis les deux endroits :

```rust
/// Vrai si l'en-tête porte un `Bearer` correspondant à un token d'upload non révoqué.
/// Partagé avec `lobby::api::ingest` : les deux endpoints ont le même contrat d'authentification.
pub async fn token_valide(db: &sqlx::PgPool, headers: &axum::http::HeaderMap) -> bool {
    token_id(db, headers).await.is_some()
}

/// L'id du token, quand il est valide (l'upload en a besoin pour tracer `uploads.token_id`).
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

Puis remplacer le bloc inline de `upload` par `let Some(token_id) = upload::token_id(&state.db,
&headers).await else { … }`, en conservant sa réponse 401 existante à l'identique.

- [ ] **Étape 6 : Câbler l'état dans `AppState`**

Dans `crates/storm-codex-server/src/main.rs`, ajouter le module et le champ :

```rust
mod lobby;
```

```rust
pub struct AppState {
    // … champs existants …
    /// Lobby live courant (singleton, persisté dans `lobby_live`). `None` = aucun lobby.
    pub lobby: Arc<RwLock<Option<lobby::LobbyState>>>,
}
```

À la construction de l'état, après le chargement du draft :

```rust
    let lobby_initial = lobby::store::load(&db).await;
```

et dans le littéral `AppState { … }` :

```rust
        lobby: Arc::new(RwLock::new(lobby_initial)),
```

Routes, à la suite de celles du draft :

```rust
        .route("/api/lobby", get(lobby::api::get).post(lobby::api::ingest))
        .route("/api/lobby", axum::routing::delete(lobby::api::clear))
```

⚠️ axum refuse deux `.route()` sur le même chemin : **fusionner** en un seul appel —
`.route("/api/lobby", get(lobby::api::get).post(lobby::api::ingest).delete(lobby::api::clear))`.

- [ ] **Étape 7 : Vérifier de bout en bout**

Créer un token, extraire un blob réel, le poster :

```bash
TOKEN=$(curl -s -XPOST localhost:8080/api/admin/tokens -H "authorization: Bearer $ADMIN_TOKEN" \
  -H 'content-type: application/json' -d '{"name":"test-lobby"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
cargo run -q -p storm-replay --example dump_lobby -- \
  "crates/storm-stats/tests/data/silver-city-aram.StormReplay" /tmp/lobby.bin
curl -s -XPOST localhost:8080/api/lobby -H "authorization: Bearer $TOKEN" \
  --data-binary @/tmp/lobby.bin
curl -s localhost:8080/api/lobby | head -c 400
```

Expected: `{"status":"ok","players":10}` puis un état JSON avec 10 joueurs et leurs BattleTags.
Reposter le même fichier doit répondre `{"status":"unchanged"}`. Sans `Bearer` : 401.

- [ ] **Étape 8 : Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server
git commit -m "feat(server): ingestion du lobby live (POST/GET/DELETE /api/lobby + WS lobby.detected)"
```

---

## Task 4 : Enrichissement contre l'archive

**Files:**
- Create: `crates/storm-codex-server/src/lobby/enrich.rs`

**Interfaces:**
- Produit : `pub async fn enrich(db: &PgPool, state: &mut LobbyState)` — remplit `toon_handle`,
  `history`, `me`, `build` et `me_stats`. Appelée par `lobby::api::ingest` (tâche 3) et après chaque
  mutation manuelle (tâche 5).
- Consomme : `LobbyState`, `LobbyPlayerState` (tâche 3) ; la table `builds` (tâche 2).

C'est le cœur de la valeur du companion : ce que le blob donne (des BattleTags) devient ce que tu
veux voir (qui sont ces gens, et comment ça s'est passé avec eux).

- [ ] **Étape 1 : Écrire la résolution d'identité**

```rust
//! Enrichissement du lobby contre l'archive. Le blob ne porte que `nom#discriminant` : le
//! `toon_handle` se retrouve dans `match_players` (nom + `data->>'tag'`), c'est-à-dire dans les
//! parties déjà jouées. Un joueur jamais croisé reste non résolu — sans conséquence, puisqu'il n'a
//! de toute façon aucun historique à afficher.
use serde_json::Value as J;
use sqlx::PgPool;

use crate::lobby::LobbyState;

/// `nom#tag` → `toon_handle`, d'après l'archive. `None` si jamais croisé.
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

- [ ] **Étape 2 : Écrire l'agrégat par joueur**

```rust
/// Historique d'un joueur du point de vue de l'opérateur : parties ensemble, parties contre,
/// winrates, héros favoris. Une seule requête — le contrat p95 < 100 ms de la spec s'applique à
/// `/api/lobby`, et un aller-retour par joueur le ferait sauter.
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

- [ ] **Étape 3 : Écrire les stats de l'opérateur et le build**

```rust
/// Stats de l'opérateur sur ce héros, cette carte, et le couple. `hero` peut être `None` tant que
/// l'opérateur ne l'a pas saisi : les stats de carte restent alors calculables.
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

/// Build par défaut du héros + alternatives. `None` si aucun build enregistré : le front propose
/// alors l'import depuis la meilleure partie sur ce héros.
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

- [ ] **Étape 4 : Assembler**

```rust
/// Remplit l'état avec tout ce que l'archive sait. Best-effort : une requête qui échoue laisse le
/// champ à `None` plutôt que de faire échouer l'ingestion — un lobby à moitié enrichi vaut mieux
/// qu'une page vide.
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

L'import `serde_json::json` n'est pas utilisé par ce module : ne l'importer que si une évolution
l'exige — `cargo clippy -D warnings` signalerait un import mort.

- [ ] **Étape 5 : Vérifier contre une base peuplée**

Il faut des données : uploader quelques replays du corpus committé dans la base de dev, puis
reposter le blob de lobby correspondant.

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

Expected: les 10 joueurs ont un `toon_handle` non nul (ils sont tous dans l'archive de dev puisque
le lobby vient d'un de ces replays), `history.games_with` ou `games_against` ≥ 1 pour chacun, et
`me` pointe sur l'index de `matella`.

Mesurer le budget de la spec :

```bash
for i in $(seq 20); do curl -s -o /dev/null -w '%{time_total}\n' localhost:8080/api/lobby; done | sort -n | tail -2
```

Expected: p95 < 0,100 s. Au-delà, le coupable est la boucle par joueur de l'étape 4 : la remplacer
par **une** requête prenant le tableau des toon handles (`= ANY($1)`) plutôt que d'en faire dix.

- [ ] **Étape 6 : Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server/src/lobby/enrich.rs
git commit -m "feat(server): enrichir le lobby contre l'archive (identités, historiques, build, stats)"
```

---

## Task 5 : Saisies et corrections manuelles

**Files:**
- Modify: `crates/storm-codex-server/src/lobby/api.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (routes)

**Interfaces:**
- Produit : `POST /api/lobby/hero`, `POST /api/lobby/map`, `POST /api/lobby/teams`. Chacune mute
  l'état, ré-enrichit, persiste et diffuse `lobby.updated`.
- Consomme : `LobbyState` (tâche 3), `enrich` (tâche 4).

Le blob ne porte pas le héros (mesuré, plan 1). Et la déduction d'équipe, parfaite en matchmaking,
se trompe en partie personnalisée : dans 94,7 % des cas fautifs l'ordre ne porte **aucune**
information d'équipe, donc un bouton d'inversion ne suffirait pas — d'où une réassignation
**par joueur**.

- [ ] **Étape 1 : Ajouter les trois routes**

À la fin de `crates/storm-codex-server/src/lobby/api.rs` :

```rust
#[derive(serde::Deserialize)]
pub struct HeroBody {
    /// Clé `dim_heroes.id`. `null` pour effacer la saisie.
    pub hero: Option<String>,
}

/// `POST /api/lobby/hero` — le héros n'est jamais dans le blob : c'est le seul tap obligatoire.
pub async fn set_hero(State(s): State<AppState>, Json(b): Json<HeroBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| st.hero = b.hero).await
}

#[derive(serde::Deserialize)]
pub struct MapBody {
    pub map: Option<String>,
}

/// `POST /api/lobby/map` — repli quand la carte n'a pas pu être déduite des hashes `.s2ma`.
pub async fn set_map(State(s): State<AppState>, Json(b): Json<MapBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| {
        st.map = b.map;
        st.map_manual = true;
    })
    .await
}

#[derive(serde::Deserialize)]
pub struct TeamsBody {
    /// `battletag → équipe (0 ou 1)`. Les joueurs absents de la table gardent leur équipe.
    pub teams: std::collections::HashMap<String, u8>,
}

/// `POST /api/lobby/teams` — réassignation par joueur. Un simple bouton d'inversion ne réparerait
/// que 5,3 % des cas fautifs (mesure du plan 1) : dans les autres, l'ordre du lobby ne porte
/// aucune information d'équipe et seule une saisie explicite peut la reconstruire.
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

/// Mutation + ré-enrichissement + persistance + diffusion, factorisés : les trois routes
/// ci-dessus ne diffèrent que par la mutation elle-même.
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

- [ ] **Étape 2 : Déclarer les routes**

```rust
        .route("/api/lobby/hero", post(lobby::api::set_hero))
        .route("/api/lobby/map", post(lobby::api::set_map))
        .route("/api/lobby/teams", post(lobby::api::set_teams))
```

- [ ] **Étape 3 : Vérifier**

```bash
curl -s -XPOST localhost:8080/api/lobby/hero -H 'content-type: application/json' \
  -d '{"hero":"Tychus"}' | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d["hero"], d["build"] is not None, d["me_stats"])'
```

Expected: `Tychus True {...}` — le build du héros et les stats apparaissent immédiatement, preuve
que la mutation ré-enrichit.

```bash
BT=$(curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print(json.load(sys.stdin)["players"][0]["battletag"])')
curl -s -XPOST localhost:8080/api/lobby/teams -H 'content-type: application/json' \
  -d "{\"teams\":{\"$BT\":1}}" | python3 -c 'import sys,json;p=json.load(sys.stdin)["players"][0];print(p["team"], p["team_manual"])'
```

Expected: `1 True`. Et sans lobby courant (`DELETE /api/lobby` d'abord), les trois routes doivent
répondre 404, pas 500.

- [ ] **Étape 4 : Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
git add crates/storm-codex-server
git commit -m "feat(server): saisie du héros/carte et réassignation d'équipe par joueur"
```

---

## Task 6 : Liaison replay ↔ lobby et debrief de build

**Files:**
- Modify: `crates/storm-codex-server/src/lobby/mod.rs` (fonction de liaison)
- Modify: `crates/storm-codex-server/src/upload.rs` (appel après parse réussi)

**Interfaces:**
- Produit : `pub async fn lier_match(db, lobby, match_id)` — remplit `LobbyState.match_id` quand les
  BattleTags correspondent. Le front (plan 3) bascule alors en debrief et compare `build.default.picks`
  aux talents joués, deux objets de même forme.
- Consomme : `LobbyState` (tâche 3), le chemin de parse existant (`upload.rs:295`, là où
  `match.parsed` est déjà diffusé).

- [ ] **Étape 1 : Écrire la liaison**

Dans `crates/storm-codex-server/src/lobby/mod.rs` :

```rust
/// Relie un match fraîchement parsé au lobby courant, si c'est la même partie. Critère : l'ensemble
/// des BattleTags. Le parse complet reconstruit les mêmes `nom#tag` depuis le blob embarqué dans le
/// replay (`storm_stats`, `get_battletags`), donc les deux côtés portent la même clé — sans rien
/// supposer du format binaire. Fenêtre de 6 h pour écarter une composition rejouée plus tard.
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

- [ ] **Étape 2 : Appeler après un parse réussi**

Dans `crates/storm-codex-server/src/upload.rs`, juste avant la diffusion de `match.parsed`
(ligne ~295) :

```rust
            // Si ce match est la partie du lobby ouvert, le companion bascule en debrief.
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

⚠️ Ne pas garder le write-lock pendant la diffusion si le compilateur signale un emprunt : sortir
le `bool` du bloc, `drop(guard)`, puis diffuser — même précaution que dans `muter` (tâche 5).

- [ ] **Étape 3 : Vérifier la bascule**

```bash
curl -s -XDELETE localhost:8080/api/lobby
curl -s -XPOST localhost:8080/api/lobby -H "authorization: Bearer $TOKEN" --data-binary @/tmp/lobby.bin
curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print("match_id:", json.load(sys.stdin)["match_id"])'
```

Expected: `match_id: None` (le replay est déjà en base, mais aucun parse n'a eu lieu depuis
l'ouverture du lobby).

Reprocesser le replay correspondant pour déclencher le chemin de parse :

```bash
curl -s -XPOST localhost:8080/api/admin/reprocess -H "authorization: Bearer $ADMIN_TOKEN" \
  -H 'content-type: application/json' -d '{"all":true}'
curl -s localhost:8080/api/lobby | python3 -c 'import sys,json;print("match_id:", json.load(sys.stdin)["match_id"])'
```

Expected: un `match_id` numérique. Vérifier ensuite que le lobby d'une **autre** partie ne se lie
pas : reposter un blob issu d'un replay différent et reprocesser — `match_id` doit rester `None`
pour les parties qui ne sont pas la sienne.

- [ ] **Étape 4 : Commit**

```bash
cargo clippy -p storm-codex-server --all-targets -- -D warnings
cargo test --workspace
git add crates/storm-codex-server
git commit -m "feat(server): relier le replay parsé au lobby courant (bascule debrief)"
```

---

## Fin de plan

- [x] `cargo test --workspace` vert
- [x] `cargo clippy -p storm-lobby -p storm-codex-server --all-targets -- -D warnings` vert
      (⚠️ `--workspace` échoue pour une raison **pré-existante** sur les tests de
      `storm-codex-server` : 11 `unwrap_used`. Vérifié sur `main`, hors périmètre — ne pas le
      corriger dans ce plan, et ne pas le confondre avec une régression.)
- [x] `/api/lobby` mesuré **p95 ≈ 35 ms** sur `POST` (le chemin qui exécute l'enrichissement), consigné dans `docs/STATUS.md`
- [x] Couverture de la carte par la table `.s2ma` : **19/19 cartes, 116 hashes**, consignée dans `docs/STATUS.md`
- [x] `docs/STATUS.md` mis à jour (état + prochaine étape), conformément à `CLAUDE.md`

Puis : plan 3 (front `/companion` et `/builds`), et plan 4 (watcher `client-rs`, repo Hots-Overlay,
seul morceau exigeant le PC de jeu).
