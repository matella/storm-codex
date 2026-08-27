# Companion live — Plan 1 : crate `storm-lobby` (parser autonome + parité)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** produire un crate Rust pur qui, à partir des seuls octets d'un
`replay.server.battlelobby`, retrouve les 10 joueurs d'une partie (nom, BattleTag, toon handle,
équipe, et héros si le format le porte), prouvé à **≥ 99 %** contre le parse complet sur l'archive.

**Architecture:** `storm-lobby` ne dépend de rien (il reçoit `&[u8]`). Sa vérité de référence est
`storm-stats`, qui connaît la composition réelle de chaque partie : chaque `.StormReplay` archivé
contient le même blob lobby que celui écrit en live, donc chaque replay est à la fois un
échantillon et sa propre correction. `storm-replay` et `storm-stats` n'entrent qu'en
**dev-dependencies** — le crate publié reste pur.

**Tech Stack:** Rust 2021, `thiserror`, tests intégration `cargo test`, exemples `cargo run
--example`.

## Pourquoi ce plan s'arrête au parser

La spec couvre aussi le serveur, le front et le watcher `client-rs`. Ils ne sont **pas** dans ce
plan : le format binaire du battlelobby est inconnu à ce jour, et écrire aujourd'hui le code exact
des tâches serveur/front reviendrait à figer des détails (présence du héros, de la carte) que
seule la tâche 1 peut trancher. Le plan 2 (serveur + front) s'écrit dès que ce plan est vert ;
le plan 3 (watcher `client-rs`, repo Hots-Overlay) en dernier, c'est le seul qui exige le PC de jeu.

Ce plan produit à lui seul un livrable utile et testable : un parser prouvé et chiffré.

## Global Constraints

- Rust édition 2021 ; lints workspace : `clippy::unwrap_used = deny`, `clippy::expect_used = warn`.
  Les fichiers de test ouvrent par `#![allow(clippy::expect_used, clippy::unwrap_used)]`, comme
  `crates/storm-stats/tests/extended_maps.rs`.
- Erreurs typées via `thiserror`. Aucun `unwrap()` hors tests.
- `storm-lobby` est **pur** : aucune I/O, aucune dépendance runtime à `storm-replay`/`storm-stats`.
- Tout champ dont la présence n'est pas garantie par le format est un `Option`. Le parser ne comble
  jamais un trou par une valeur inventée.
- Commits conventionnels. Co-auteur non requis pour ce repo.
- Tâches 1 à 4 : 100 % sur le Mac, aucun accès au box. Seule la tâche 5 lit l'archive du box
  (Tailscale, le soir), **en lecture seule**.
- Toute la V1 se fait sur la branche `feat/companion-live`.

---

## Préparation

- [ ] **Étape 1 : Créer la branche**

```bash
git checkout -b feat/companion-live
```

- [ ] **Étape 2 : Vérifier que la base est verte avant de toucher quoi que ce soit**

Run: `cargo test --workspace`
Expected: PASS (aucun échec préexistant à confondre avec les nôtres)

---

## Fichiers

| Fichier | Responsabilité |
|---|---|
| `crates/storm-replay/examples/dump_lobby.rs` | extraire le blob lobby d'un `.StormReplay` vers un fichier (outil d'inspection) |
| `docs/research/2026-08-27-lobby-format.md` | rapport d'inspection : ce que le blob contient, réponses aux 4 questions ouvertes |
| `crates/storm-lobby/Cargo.toml` | manifeste ; deps runtime minimales, oracle en dev-deps |
| `crates/storm-lobby/src/lib.rs` | API publique (`parse`, `Lobby`, `LobbyPlayer`, `LobbyError`) |
| `crates/storm-lobby/tests/oracle.rs` | parité contre `storm-stats` sur les 5 replays committés |
| `crates/storm-lobby/tests/robustness.rs` | entrées tronquées / vides / aléatoires → `Err`, jamais `panic` |
| `crates/storm-lobby/examples/parity.rs` | harnais de parité sur un dossier d'archive (tâche 5) |
| `Cargo.toml` (racine) | ajouter `crates/storm-lobby` aux membres du workspace |
| `docs/research/2026-08-27-lobby-parity.md` | rapport go/no-go chiffré |

---

## Task 1 : Outil de dump et inspection du format

**Files:**
- Create: `crates/storm-replay/examples/dump_lobby.rs`
- Create: `docs/research/2026-08-27-lobby-format.md`

**Interfaces:**
- Consomme : `storm_replay::Replay::open()` et `::battlelobby_raw()` (existants,
  `crates/storm-replay/src/lib.rs:90` et `:258`).
- Produit : les réponses factuelles dont la tâche 3 a besoin pour écrire le parser, et les fichiers
  `.bin` de travail.

Cette tâche est de l'investigation, pas du TDD : on ne peut pas tester une hypothèse sur un format
qu'on n'a pas encore lu. Son livrable est un **document de constat**, et il conditionne tout le reste.

- [ ] **Étape 1 : Écrire l'outil de dump**

```rust
//! Extrait le stream `replay.server.battlelobby` d'un .StormReplay vers un fichier brut.
//! C'est le même blob que celui écrit en live par le jeu dans
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.
//!
//! Usage : cargo run -p storm-replay --example dump_lobby -- <replay> <sortie.bin>
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = || anyhow::anyhow!("usage: dump_lobby <replay.StormReplay> <sortie.bin>");
    let input: PathBuf = args.next().ok_or_else(usage)?.into();
    let output: PathBuf = args.next().ok_or_else(usage)?.into();

    let replay = storm_replay::Replay::open(&input)?;
    let blob = replay.battlelobby_raw()?;
    std::fs::write(&output, &blob)?;
    println!("{} octets → {}", blob.len(), output.display());
    Ok(())
}
```

- [ ] **Étape 2 : Vérifier que l'outil tourne**

```bash
mkdir -p /tmp/lobby && \
cargo run -p storm-replay --example dump_lobby -- \
  "crates/storm-stats/tests/data/silver-city-aram.StormReplay" /tmp/lobby/silver-city.bin
```

Expected: une ligne `NNNNN octets → /tmp/lobby/silver-city.bin`, taille non nulle.

- [ ] **Étape 3 : Dumper les 5 replays committés**

```bash
for f in crates/storm-replay/tests/data/*.StormReplay crates/storm-stats/tests/data/*.StormReplay; do
  out="/tmp/lobby/$(basename "$f" .StormReplay).bin"
  cargo run -q -p storm-replay --example dump_lobby -- "$f" "$out"
done
ls -l /tmp/lobby/
```

Expected: 4 ou 5 fichiers `.bin` (un replay peut apparaître deux fois s'il est partagé).

- [ ] **Étape 4 : Extraire les chaînes lisibles**

```bash
strings -n 3 /tmp/lobby/silver-city.bin | head -80
```

Noter : les BattleTags (`nom#1234`) apparaissent-ils ? Dans quel ordre ? Y a-t-il des noms
ressemblant à des identifiants de héros (`HeroTychus`, `Tychus`…) ou de carte ?

- [ ] **Étape 5 : Localiser les ancres à l'octet près**

```bash
xxd /tmp/lobby/silver-city.bin | head -60
xxd /tmp/lobby/silver-city.bin | grep -i -m5 'tychus'
```

But : repérer comment un enregistrement joueur est délimité (longueur préfixée ? séparateur ?) et
ce qui entoure chaque BattleTag.

- [ ] **Étape 6 : Comparer un blob live à son homologue archivé (question n° 4 de la spec)**

À faire quand tu es sur le PC de jeu, ou à reporter en tâche 5 si tu ne l'es pas. Copier
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` juste après un chargement,
puis, la partie finie, dumper le blob du replay correspondant et comparer :

```bash
cmp /tmp/lobby/live.bin /tmp/lobby/archive.bin && echo "IDENTIQUES" || echo "DIFFERENTS"
```

Si identiques → la liaison replay↔lobby pourra se faire par hash d'octets (plus exact).
Sinon → on garde la liaison par ensemble de `toon_handle`, comme prévu par défaut.

- [ ] **Étape 7 : Écrire le rapport de constat**

Créer `docs/research/2026-08-27-lobby-format.md` répondant explicitement, **par oui ou non et avec
la preuve à l'appui** (extrait hexadécimal ou de `strings`) :

1. Les BattleTags sont-ils présents, et dans l'ordre du lobby ?
2. Les composantes du toon handle (`m_region`, `m_programId`, `m_realm`, `m_id`) sont-elles
   présentes, ou faut-il reconstituer le handle autrement ?
3. L'appartenance à une équipe est-elle déductible (champ explicite, ou position/ordre) ?
4. **Le héros pické est-il présent ?** Si oui : sous quelle forme, et est-ce le héros réellement
   joué en ARAM (où le shuffle à 3 choix pollue l'attribut 4002 du replay — cf. `docs/STATUS.md`) ?
5. La carte et le mode sont-ils présents ?
6. Le blob live est-il bit-à-bit identique au blob archivé (étape 6, ou « non testé »).

Toute réponse inconnue doit être écrite « inconnu », jamais devinée.

- [ ] **Étape 8 : Commit**

```bash
git add crates/storm-replay/examples/dump_lobby.rs docs/research/2026-08-27-lobby-format.md
git commit -m "feat(storm-replay): exemple dump_lobby + rapport d'inspection du format battlelobby"
```

---

## Task 2 : Crate `storm-lobby` et oracle de parité (rouge)

**Files:**
- Create: `crates/storm-lobby/Cargo.toml`
- Create: `crates/storm-lobby/src/lib.rs`
- Create: `crates/storm-lobby/tests/oracle.rs`
- Modify: `Cargo.toml` (racine, `members`)

**Interfaces:**
- Produit : `storm_lobby::parse(&[u8]) -> Result<Lobby, LobbyError>`, `Lobby { players:
  Vec<LobbyPlayer>, map: Option<String> }`, `LobbyPlayer { name: String, battletag: Option<String>,
  toon_handle: String, team: Option<u8>, hero: Option<String> }`. Ces noms sont ceux que le plan 2
  (serveur) consommera — ne pas les renommer.
- Consomme (dev only) : `storm_replay::Replay`, `storm_stats::process_replay`.

- [ ] **Étape 1 : Créer le manifeste**

```toml
[package]
name = "storm-lobby"
version = "0.1.0"
description = "Parseur autonome du fichier de lobby Heroes of the Storm (replay.server.battlelobby)"
keywords = ["heroes-of-the-storm", "lobby", "parser", "blizzard"]
categories = ["parser-implementations", "games"]
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
thiserror = "2"

# L'oracle a besoin du décodeur et du parseur complets pour connaître la vérité de chaque replay.
# En dev-dependencies uniquement : le crate publié reste pur.
[dev-dependencies]
storm-replay = { path = "../storm-replay" }
storm-stats = { path = "../storm-stats" }
serde_json = "1"
anyhow = "1"

[lints]
workspace = true
```

- [ ] **Étape 2 : Déclarer le crate dans le workspace**

Dans `Cargo.toml` à la racine, remplacer la ligne `members` par :

```toml
members = ["crates/storm-replay", "crates/storm-stats", "crates/storm-codex-server", "crates/storm-replay-viewer", "crates/storm-lobby"]
```

- [ ] **Étape 3 : Écrire l'API publique, sans implémentation**

`crates/storm-lobby/src/lib.rs` :

```rust
//! Parseur autonome du fichier de lobby Heroes of the Storm.
//!
//! Le jeu écrit `replay.server.battlelobby` pendant l'écran de chargement, avant que le replay
//! n'existe. Ce crate lit ce blob **seul** — sans le stream `details` du replay — pour identifier
//! les 10 joueurs d'une partie en cours.
//!
//! Crate pur : aucune I/O, aucune dépendance sur `storm-replay`.

use thiserror::Error;

/// Un joueur du lobby. Tout champ dont la présence n'est pas garantie par le format est `Option` :
/// on préfère l'absence explicite à une valeur inventée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    /// Nom du compte, sans le discriminant.
    pub name: String,
    /// Discriminant seul (partie après `#`), quand il est lisible.
    pub battletag: Option<String>,
    /// `"region-programId-realm-id"` — même format que `match_players.toon_handle` en base.
    pub toon_handle: String,
    /// 0 ou 1. `None` si le format ne permet pas de trancher.
    pub team: Option<u8>,
    /// Héros pické, normalisé sur la clé `dim_heroes.id`. `None` si absent du blob.
    pub hero: Option<String>,
}

/// Un lobby décodé. `players` est dans l'ordre du lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    pub map: Option<String>,
}

#[derive(Debug, Error)]
pub enum LobbyError {
    #[error("blob trop court ({0} octets) pour contenir un lobby")]
    TooShort(usize),
    #[error("aucun joueur identifiable dans le blob")]
    NoPlayers,
    #[error("structure de lobby non reconnue : {0}")]
    Unrecognized(String),
}

/// Décode un blob `replay.server.battlelobby`.
///
/// # Errors
/// Retourne [`LobbyError`] si le blob est trop court, vide de joueurs, ou d'une structure
/// non reconnue. Ne panique jamais, quelle que soit l'entrée.
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError> {
    Err(LobbyError::Unrecognized(format!(
        "parse non implémenté ({} octets)",
        bytes.len()
    )))
}
```

- [ ] **Étape 4 : Écrire l'oracle**

`crates/storm-lobby/tests/oracle.rs` :

```rust
//! Oracle de parité : à partir du seul blob lobby, le parser doit retrouver ce que le parse
//! complet du replay sait de source sûre (noms, toon handles, équipes).
//!
//! Chaque `.StormReplay` porte le même blob que celui écrit en live — donc chaque replay est à la
//! fois un échantillon et sa propre correction.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Les replays committés dans le workspace, chemins relatifs à la racine.
fn replays() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [
        "crates/storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay",
        "crates/storm-stats/tests/data/silver-city-aram.StormReplay",
    ]
    .iter()
    .map(|p| root.join(p))
    .collect()
}

/// Vérité de référence : `{toon_handle: (nom, équipe)}` d'après le parse complet.
fn truth(path: &Path) -> BTreeMap<String, (String, i64)> {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("nom de fichier");
    let out = storm_stats::process_replay(path, filename);
    assert_eq!(out.status, 1, "{filename} : parse complet rejeté");
    let json = out.to_json();
    json["players"]
        .as_object()
        .expect("players")
        .values()
        .map(|p| {
            (
                p["ToonHandle"].as_str().expect("ToonHandle").to_string(),
                (
                    p["name"].as_str().expect("name").to_string(),
                    p["team"].as_i64().expect("team"),
                ),
            )
        })
        .collect()
}

#[test]
fn parse_retrouve_les_joueurs_du_replay() {
    for path in replays() {
        let name = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
        let blob = replay.battlelobby_raw().expect("stream battlelobby");

        let lobby = storm_lobby::parse(&blob).expect("parse lobby");
        let expected = truth(&path);

        assert_eq!(
            lobby.players.len(),
            expected.len(),
            "{name} : {} joueurs décodés, {} attendus",
            lobby.players.len(),
            expected.len()
        );

        for p in &lobby.players {
            let (exp_name, exp_team) = expected
                .get(&p.toon_handle)
                .unwrap_or_else(|| panic!("{name} : toon handle inconnu « {} »", p.toon_handle));
            assert_eq!(&p.name, exp_name, "{name} : nom divergent");
            if let Some(team) = p.team {
                assert_eq!(i64::from(team), *exp_team, "{name} : équipe divergente");
            }
        }
    }
}
```

- [ ] **Étape 5 : Lancer l'oracle et vérifier qu'il échoue pour la bonne raison**

Run: `cargo test -p storm-lobby --test oracle`
Expected: FAIL sur `parse lobby` → `Unrecognized("parse non implémenté (… octets)")`.
Un échec sur autre chose (compilation, chemin de replay introuvable, `status != 1`) doit être
corrigé avant de passer à la tâche 3 — sinon l'oracle ne prouve rien.

- [ ] **Étape 6 : Commit**

```bash
git add Cargo.toml crates/storm-lobby
git commit -m "test(storm-lobby): squelette du crate + oracle de parité (rouge)"
```

---

## Task 3 : Implémenter le parser jusqu'au vert

**Files:**
- Modify: `crates/storm-lobby/src/lib.rs`

**Interfaces:**
- Consomme : le rapport de la tâche 1 (offsets, ancres, présence des champs).
- Produit : `parse()` fonctionnel. Signature et types **inchangés** par rapport à la tâche 2.

Le code de cette tâche ne peut pas être écrit à l'avance : les décalages d'octets sont le livrable
du reverse-engineering de la tâche 1. Ce qui **est** fixé d'avance, et ne se négocie pas : la
signature, les types, l'interdiction du `unwrap()`, et l'oracle qui juge.

- [ ] **Étape 1 : Extraire les BattleTags (le morceau le plus sûr)**

Réutiliser la regex déjà éprouvée en production dans `crates/storm-stats/src/process.rs:313` —
`[\p{L}\d]{3,24}#\d{4,10}[zØ]?` — mais sans la corrélation à `details.m_playerList`, absente ici.
Si cela impose la dépendance `regex`, l'ajouter aux `[dependencies]` du manifeste (elle est déjà
utilisée par `storm-stats`, donc déjà dans le lockfile).

Run: `cargo test -p storm-lobby --test oracle`
Expected: toujours FAIL, mais désormais sur les toon handles ou le compte de joueurs — pas sur
« non implémenté ». C'est la progression attendue.

- [ ] **Étape 2 : Reconstituer les toon handles**

Format cible, identique à celui de la base (`process.rs:540-551`) :
`format!("{region}-{program_id}-{realm}-{id}")`. Les composantes viennent du blob, d'après la
tâche 1. Si le blob ne les porte pas dans cette forme, le rapport de la tâche 1 doit dire d'où
elles sortent — et si elles ne sortent de nulle part, c'est un no-go à remonter immédiatement à
l'opérateur plutôt qu'à contourner.

Run: `cargo test -p storm-lobby --test oracle`
Expected: FAIL réduit aux équipes, ou PASS.

- [ ] **Étape 3 : Déduire les équipes**

`team: Option<u8>` — 0 ou 1. Si la tâche 1 a conclu que l'appartenance n'est pas déductible du
blob, laisser `None` : l'oracle ne vérifie l'équipe **que** lorsqu'elle est `Some`, précisément
pour permettre cette honnêteté. Ne jamais déduire l'équipe de la position dans la liste sans une
preuve écrite dans le rapport de la tâche 1.

- [ ] **Étape 4 : Héros et carte, si et seulement si la tâche 1 les a trouvés**

Renseigner `hero` (normalisé sur la clé `dim_heroes.id`, c'est-à-dire le nom héros de storm-stats)
et `map`. Sinon les laisser à `None` — le plan 2 prévoit déjà le sélecteur manuel de repli, et un
héros inventé serait pire qu'un héros absent.

- [ ] **Étape 5 : Vérifier le vert et la propreté**

Run: `cargo test -p storm-lobby`
Expected: PASS

Run: `cargo clippy -p storm-lobby --all-targets -- -D warnings`
Expected: aucun avertissement (`unwrap_used` est `deny` au niveau workspace).

- [ ] **Étape 6 : Commit**

```bash
git add crates/storm-lobby
git commit -m "feat(storm-lobby): parse autonome du battlelobby — oracle vert sur les replays committés"
```

---

## Task 4 : Robustesse aux entrées mal formées

**Files:**
- Create: `crates/storm-lobby/tests/robustness.rs`
- Modify: `crates/storm-lobby/src/lib.rs` (si un test met en évidence une panique)

**Interfaces:**
- Consomme : `storm_lobby::parse`, `storm_lobby::LobbyError`.
- Produit : la garantie « ne panique jamais », dont dépend l'endpoint serveur du plan 2 — un
  `panic!` dans un handler axum tuerait la requête et polluerait les logs à chaque changement de
  build Blizzard.

- [ ] **Étape 1 : Écrire les tests de robustesse**

```rust
//! `parse` consomme du binaire potentiellement mal formé (nouveau build Blizzard, fichier lu
//! pendant son écriture). Il doit TOUJOURS retourner `Err`, jamais paniquer.

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
    assert!(storm_lobby::parse(&[]).is_err(), "blob vide accepté");
}

#[test]
fn entree_aleatoire_est_une_erreur_pas_une_panique() {
    // Générateur déterministe (xorshift) : pas de dépendance, résultat reproductible.
    let mut state: u32 = 0x1234_5678;
    let mut bytes = Vec::with_capacity(4096);
    for _ in 0..4096 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        bytes.push((state & 0xff) as u8);
    }
    assert!(storm_lobby::parse(&bytes).is_err(), "bruit accepté comme lobby");
}

#[test]
fn blob_tronque_a_toute_longueur_ne_panique_jamais() {
    let blob = blob_reel();
    // Un fichier lu pendant son écriture est un préfixe du fichier complet : on exerce
    // exactement ce cas, à tous les points de troncature (pas de 97 pour rester rapide).
    for n in (0..blob.len()).step_by(97) {
        let _ = storm_lobby::parse(&blob[..n]);
    }
}

#[test]
fn blob_corrompu_en_son_milieu_ne_panique_jamais() {
    let mut blob = blob_reel();
    let milieu = blob.len() / 2;
    for b in &mut blob[milieu..milieu + 64.min(blob.len() - milieu)] {
        *b = 0xff;
    }
    let _ = storm_lobby::parse(&blob);
}
```

- [ ] **Étape 2 : Lancer les tests**

Run: `cargo test -p storm-lobby --test robustness`
Expected: PASS. Une panique (`index out of bounds`, `slice index starts at…`) est un défaut réel
à corriger dans `parse` — remplacer l'indexation directe par `get()`/`get(..n)` et propager
`LobbyError::TooShort`.

- [ ] **Étape 3 : Vérifier que l'oracle est toujours vert**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Étape 4 : Commit**

```bash
git add crates/storm-lobby
git commit -m "test(storm-lobby): robustesse — entrées vides, tronquées, corrompues, aléatoires"
```

---

## Task 5 : Harnais de parité sur l'archive et rapport go/no-go

**Files:**
- Create: `crates/storm-lobby/examples/parity.rs`
- Create: `docs/research/2026-08-27-lobby-parity.md`

**Interfaces:**
- Consomme : `storm_lobby::parse`, `storm_replay::Replay`, `storm_stats::process_replay`.
- Produit : le chiffre qui décide du go/no-go — **≥ 99 %** de lobbies exacts (noms, toon handles,
  équipes) sur l'archive, tous builds confondus.

- [ ] **Étape 1 : Écrire le harnais**

```rust
//! Parité du parser autonome contre le parse complet, sur un dossier de replays.
//!
//! Usage : cargo run --release -p storm-lobby --example parity -- <dossier> [max]
//!
//! Pour chaque replay : on extrait le blob, on le parse seul, et on compare aux joueurs que le
//! parse complet a identifiés. Un replay est « exact » si les 10 toon handles, les 10 noms et
//! (quand le parser les fournit) les 10 équipes correspondent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Tally {
    total: usize,
    exact: usize,
    lobby_err: usize,
    stats_rejete: usize,
    divergents: Vec<(String, String)>,
    heros_presents: usize,
    cartes_presentes: usize,
    par_build: BTreeMap<u32, (usize, usize)>, // build → (total, exact)
}

fn verite(path: &Path) -> Option<BTreeMap<String, (String, i64)>> {
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
                Some((
                    p["ToonHandle"].as_str()?.to_string(),
                    (p["name"].as_str()?.to_string(), p["team"].as_i64()?),
                ))
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

        let entry = t.par_build.entry(build).or_default();
        entry.0 += 1;

        match storm_lobby::parse(&blob) {
            Err(e) => {
                t.lobby_err += 1;
                t.divergents.push((label, format!("parse: {e}")));
            }
            Ok(lobby) => {
                if lobby.map.is_some() {
                    t.cartes_presentes += 1;
                }
                if lobby.players.iter().any(|p| p.hero.is_some()) {
                    t.heros_presents += 1;
                }
                let ok = lobby.players.len() == attendu.len()
                    && lobby.players.iter().all(|p| match attendu.get(&p.toon_handle) {
                        None => false,
                        Some((n, team)) => {
                            &p.name == n && p.team.is_none_or(|t| i64::from(t) == *team)
                        }
                    });
                if ok {
                    t.exact += 1;
                    entry.1 += 1;
                } else {
                    t.divergents
                        .push((label, format!("{} joueurs décodés", lobby.players.len())));
                }
            }
        }
    }

    let base = t.total - t.stats_rejete;
    let pct = if base == 0 { 0.0 } else { 100.0 * t.exact as f64 / base as f64 };
    println!("replays vus          : {}", t.total);
    println!("écartés (stats)      : {}", t.stats_rejete);
    println!("base de comparaison  : {base}");
    println!("exacts               : {} ({pct:.2} %)", t.exact);
    println!("erreurs de parse     : {}", t.lobby_err);
    println!("héros présents       : {}", t.heros_presents);
    println!("cartes présentes     : {}", t.cartes_presentes);
    println!("\npar build (build: exacts/total)");
    for (build, (tot, ex)) in &t.par_build {
        println!("  {build}: {ex}/{tot}");
    }
    println!("\n20 premières divergences");
    for (f, why) in t.divergents.iter().take(20) {
        println!("  {f} — {why}");
    }
    Ok(())
}
```

- [ ] **Étape 2 : Rodage sur le corpus local**

```bash
cargo run --release -p storm-lobby --example parity -- crates/storm-replay/tests/data
```

Expected: `exacts : 3 (100.00 %)`. Un écart ici signale un bug du harnais, pas du parser —
l'oracle de la tâche 2 couvre déjà ces replays.

- [ ] **Étape 3 : Récupérer un échantillon de l'archive du box (lecture seule)**

Le box tourne le soir. Approche read-only, comme le runbook
`docs/runbooks/2026-07-09-visionneuse-2d-verif-box.md` :

```bash
mkdir -p /tmp/archive && \
rsync -av --include='*.StormReplay' --exclude='*' \
  matella@192.168.129.85:~/apps/storm-codex/data/archive/ /tmp/archive/ | tail -3
```

Si le chemin de l'archive diffère, le retrouver sans rien modifier :

```bash
ssh matella@192.168.129.85 "docker inspect storm-codex-server --format '{{range .Config.Env}}{{println .}}{{end}}' | grep -i -E 'archive|data'"
```

- [ ] **Étape 4 : Lancer la parité sur l'archive complète**

```bash
cargo run --release -p storm-lobby --example parity -- /tmp/archive 2>&1 | tee /tmp/lobby-parity.txt
```

Expected: le pourcentage d'exacts, la ventilation par build, les divergences.

- [ ] **Étape 5 : Écrire le rapport go/no-go**

`docs/research/2026-08-27-lobby-parity.md` : la sortie du harnais, puis le verdict.

- **≥ 99 % → GO.** Écrire aussi, noir sur blanc : héros présent ou non, carte présente ou non.
  Ces deux réponses conditionnent le plan 2.
- **< 99 % → analyser les divergences par build** avant tout verdict. Si l'échec se concentre sur
  quelques builds anciens, c'est acceptable et à documenter (le companion sert le présent, pas
  2024) — dire alors le taux **sur les builds de l'année en cours**, qui est le chiffre qui compte.
  Si l'échec est uniforme, c'est un no-go : remonter à l'opérateur, ne pas bricoler le seuil.

- [ ] **Étape 6 : Nettoyer**

```bash
rm -rf /tmp/archive /tmp/lobby
```

- [ ] **Étape 7 : Commit**

```bash
git add crates/storm-lobby/examples/parity.rs docs/research/2026-08-27-lobby-parity.md
git commit -m "test(storm-lobby): harnais de parité sur l'archive + rapport go/no-go"
```

---

## Fin de plan

- [ ] `cargo test --workspace` vert
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` vert
- [ ] `docs/research/2026-08-27-lobby-format.md` répond aux 6 questions, sans « peut-être »
- [ ] `docs/research/2026-08-27-lobby-parity.md` porte un chiffre et un verdict
- [ ] `docs/STATUS.md` mis à jour (état + prochaine étape), conformément à `CLAUDE.md`

Puis : écrire le plan 2 (migration `0009`, module `lobby.rs`, routes, `/companion`, `/builds`),
maintenant que la présence du héros et de la carte est connue.
