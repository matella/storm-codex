# Companion live — Plan 1 : crate `storm-lobby` (parser autonome + parité)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** produire un crate Rust pur qui, à partir des seuls octets d'un
`replay.server.battlelobby`, retrouve les joueurs d'une partie par leur **BattleTag** et déduit
leur équipe, prouvé à **≥ 99 %** contre le parse complet sur l'archive.

> **Révisé le 2026-08-27 après la tâche 1.** L'investigation a établi que le blob ne porte **ni
> toon handle, ni héros pické, ni carte, ni champ d'équipe explicite** — seulement les BattleTags,
> en clair. Les tâches 2 à 5 ont été réécrites en conséquence ; la tâche 1 est close et son
> constat fait foi : `docs/research/2026-08-27-lobby-format.md`.

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

> **Révision du 2026-08-27, après la tâche 1.** Le blob ne contient **ni toon handle, ni héros, ni
> carte, ni champ d'équipe explicite** (`docs/research/2026-08-27-lobby-format.md`). La seule
> identité qu'il expose est le **BattleTag** (`nom#discriminant`), en clair, préfixé par une
> longueur **en octets UTF-8**. L'oracle compare donc des BattleTags, et le type public ne porte
> plus de champ que le format ne sait pas remplir.

**Files:**
- Create: `crates/storm-lobby/Cargo.toml`
- Create: `crates/storm-lobby/src/lib.rs`
- Create: `crates/storm-lobby/tests/oracle.rs`
- Modify: `Cargo.toml` (racine, `members`)

**Interfaces:**
- Produit : `storm_lobby::parse(&[u8]) -> Result<Lobby, LobbyError>`, `Lobby { players:
  Vec<LobbyPlayer> }`, `LobbyPlayer { name: String, discriminant: String, team: Option<u8> }` et
  `LobbyPlayer::battletag() -> String`. Ces noms sont ceux que le plan 2 (serveur) consommera —
  ne pas les renommer.
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
regex = "1"
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
//! les joueurs d'une partie en cours.
//!
//! Ce que le format expose réellement, et ce qu'il n'expose pas, est constaté dans
//! `docs/research/2026-08-27-lobby-format.md` : les BattleTags sont en clair, mais ni le toon
//! handle, ni le héros pické, ni la carte, ni un champ d'équipe explicite ne s'y trouvent. Le type
//! public ne porte donc que ce qui est réellement décodable. La résolution BattleTag → identité
//! applicative se fait en aval, côté serveur, contre l'archive des parties déjà jouées.
//!
//! Crate pur : aucune I/O, aucune dépendance sur `storm-replay`.

use thiserror::Error;

/// Un joueur du lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    /// Nom du compte, sans le discriminant. Peut contenir de l'UTF-8 non-ASCII (cf. le cas
    /// cyrillique documenté dans le rapport de format).
    pub name: String,
    /// Discriminant seul, la partie après `#`.
    pub discriminant: String,
    /// 0 ou 1. `None` quand l'appartenance n'a pas pu être déterminée — le format ne porte aucun
    /// champ d'équipe, elle est déduite de l'ordre (cf. `parse`).
    pub team: Option<u8>,
}

impl LobbyPlayer {
    /// `"nom#1234"` — la clé d'identité du joueur. C'est ce que le serveur rapprochera de
    /// `match_players.name` + `match_players.data->>'tag'` pour retrouver son historique.
    #[must_use]
    pub fn battletag(&self) -> String {
        format!("{}#{}", self.name, self.discriminant)
    }
}

/// Un lobby décodé. `players` est dans l'ordre d'apparition dans le blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
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
/// Retourne [`LobbyError`] si le blob est trop court ou ne contient aucun joueur identifiable.
/// Ne panique jamais, quelle que soit l'entrée.
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
//! Oracle de parité : à partir du seul blob lobby, le parser doit retrouver les joueurs que le
//! parse complet du replay identifie — par BattleTag, seule identité que le format expose.
//!
//! Chaque `.StormReplay` porte le même blob que celui écrit en live par le jeu : chaque replay est
//! donc à la fois un échantillon et sa propre correction.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn data(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// Les replays committés dans le workspace.
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

/// Vérité de référence : `{battletag: équipe}` d'après le parse complet du replay.
/// Les joueurs dont le parse complet n'a pas résolu le discriminant sont écartés : l'oracle ne
/// peut pas juger ce que sa propre référence ignore.
fn truth(path: &Path) -> BTreeMap<String, i64> {
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
        assert_eq!(expected.len(), 10, "{label} : référence incomplète");

        let decoded: BTreeMap<String, Option<u8>> = lobby_of(&path)
            .players
            .iter()
            .map(|p| (p.battletag(), p.team))
            .collect();

        let attendus: Vec<&String> = expected.keys().collect();
        let obtenus: Vec<&String> = decoded.keys().collect();
        assert_eq!(obtenus, attendus, "{label} : BattleTags divergents");
    }
}

/// Le format ne porte aucun champ d'équipe : elle est déduite de l'ordre (5 premiers / 5 derniers).
/// Cette hypothèse n'a été observée que sur 2 échantillons en tâche 1 — ce test la vérifie sur le
/// corpus committé, et le harnais de parité (tâche 5) la vérifie sur l'archive complète.
#[test]
fn les_equipes_deduites_correspondent_au_replay() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let expected = truth(&path);
        for p in &lobby_of(&path).players {
            let Some(team) = p.team else { continue };
            let attendu = expected
                .get(&p.battletag())
                .unwrap_or_else(|| panic!("{label} : BattleTag inconnu « {} »", p.battletag()));
            assert_eq!(
                i64::from(team),
                *attendu,
                "{label} : équipe divergente pour {}",
                p.battletag()
            );
        }
    }
}

/// Régression. Un BattleTag cyrillique est présent dans ce replay en UTF-8 multi-octets ; `strings`
/// ne le voit pas. Un parser qui supposerait de l'ASCII perdrait silencieusement un joueur sur dix
/// dans les parties européennes — sans échouer, ce qui est pire. Cf. le rapport de format, Q1.
#[test]
fn battletag_non_ascii_est_decode() {
    let path = data("crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
    let lobby = lobby_of(&path);
    assert!(
        lobby
            .players
            .iter()
            .any(|p| p.name == "ЛовкийЭльф" && p.discriminant == "215346"),
        "BattleTag cyrillique absent — décodés : {:?}",
        lobby.players.iter().map(LobbyPlayer::battletag).collect::<Vec<_>>()
    );
}
```

Note : le dernier `assert!` référence `LobbyPlayer` ; ajouter `use storm_lobby::LobbyPlayer;` en
tête du fichier si le compilateur le réclame, ou remplacer par `|p| p.battletag()`.

- [ ] **Étape 5 : Lancer l'oracle et vérifier qu'il échoue pour la bonne raison**

Run: `cargo test -p storm-lobby --test oracle`
Expected: les 3 tests FAIL sur `parse lobby` → `Unrecognized("parse non implémenté (… octets)")`.
Un échec sur autre chose (compilation, replay introuvable, `status != 1`, `référence incomplète`)
doit être corrigé avant la tâche 3 — sinon l'oracle ne prouve rien.

- [ ] **Étape 6 : Commit**

```bash
git add Cargo.toml crates/storm-lobby
git commit -m "test(storm-lobby): squelette du crate + oracle de parité par BattleTag (rouge)"
```

---

## Task 3 : Implémenter le parser jusqu'au vert

**Files:**
- Modify: `crates/storm-lobby/src/lib.rs`

**Interfaces:**
- Consomme : `docs/research/2026-08-27-lobby-format.md` (constats de la tâche 1).
- Produit : `parse()` fonctionnel. Signature et types **inchangés** par rapport à la tâche 2.

- [ ] **Étape 1 : Extraire les BattleTags**

Réutiliser la regex déjà éprouvée en production dans `crates/storm-stats/src/process.rs:313` :

```rust
r"[\p{L}\d]{3,24}#\d{4,10}[zØ]?"
```

appliquée à `String::from_utf8_lossy(bytes)`. `\p{L}` couvre le cyrillique, et `from_utf8_lossy`
décode correctement l'UTF-8 multi-octets — c'est `strings` qui ne les voyait pas en tâche 1, pas
cette approche. Le test `battletag_non_ascii_est_decode` de la tâche 2 est le juge de ce point
précis : ne pas le désactiver ni l'assouplir.

Découper chaque occurrence au premier `#` : la partie gauche est `name`, la droite `discriminant`.
Conserver l'ordre d'apparition et **dédupliquer** — le blob peut contenir la même chaîne plusieurs
fois ; c'est la première occurrence de chaque BattleTag qui fait foi pour l'ordre.

Run: `cargo test -p storm-lobby --test oracle`
Expected: `parse_retrouve_les_battletags_du_replay` et `battletag_non_ascii_est_decode` PASS ;
`les_equipes_deduites_correspondent_au_replay` PASS trivialement tant que `team` vaut `None`.

- [ ] **Étape 2 : Déduire les équipes par l'ordre**

Le format ne porte aucun champ d'équipe (rapport de format, Q3). Règle, et rien de plus
ambitieux : **si et seulement si exactement 10 joueurs ont été décodés**, les 5 premiers reçoivent
`Some(0)` et les 5 derniers `Some(1)`. Dans tous les autres cas — 9 joueurs, 11, un mode non
standard — laisser `None` pour tous.

C'est délibérément conservateur : une équipe fausse est pire qu'une équipe absente, puisqu'elle
afficherait un adversaire comme allié. Le harnais de la tâche 5 mesurera si l'hypothèse d'ordre
tient sur l'archive ; tant qu'elle n'est pas mesurée, elle ne vaut que pour 10 joueurs pile.

```rust
if players.len() == 10 {
    for (i, p) in players.iter_mut().enumerate() {
        p.team = Some(u8::from(i >= 5));
    }
}
```

Run: `cargo test -p storm-lobby --test oracle`
Expected: les 3 tests PASS.

- [ ] **Étape 3 : Traiter les cas dégénérés**

`parse` doit retourner `LobbyError::TooShort(len)` sur un blob manifestement trop court pour
contenir un lobby, et `LobbyError::NoPlayers` quand aucun BattleTag n'est trouvé. Ne jamais
retourner un `Lobby` vide en `Ok` : le serveur du plan 2 distingue « lobby illisible » de « lobby
lu » pour choisir ce qu'il affiche.

- [ ] **Étape 4 : Vérifier le vert et la propreté**

Run: `cargo test -p storm-lobby`
Expected: PASS

Run: `cargo clippy -p storm-lobby --all-targets -- -D warnings`
Expected: aucun avertissement (`unwrap_used` est `deny` au niveau workspace).

- [ ] **Étape 5 : Commit**

```bash
git add crates/storm-lobby
git commit -m "feat(storm-lobby): extraction des BattleTags et déduction des équipes — oracle vert"
```

---

## Task 4 : Robustesse aux entrées mal formées

**Files:**
- Create: `crates/storm-lobby/tests/robustness.rs`
- Modify: `crates/storm-lobby/src/lib.rs` (si un test met en évidence une panique)

**Interfaces:**
- Consomme : `storm_lobby::parse`, `storm_lobby::LobbyError`.
- Produit : la garantie « ne panique jamais », dont dépend l'endpoint serveur du plan 2 — un
  `panic!` dans un handler axum tuerait la requête à chaque changement de build Blizzard.

- [ ] **Étape 1 : Écrire les tests de robustesse**

```rust
//! `parse` consomme du binaire potentiellement mal formé (nouveau build Blizzard, fichier lu
//! pendant son écriture par le jeu). Il doit TOUJOURS retourner, jamais paniquer.
//!
//! Dans les deux derniers tests, **l'absence de panique EST l'assertion** : le harnais de test
//! échoue de lui-même si `parse` panique. Le résultat est volontairement ignoré, car aucune valeur
//! de retour n'est « correcte » sur une entrée corrompue — seule l'absence de panique l'est.

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
    assert!(
        storm_lobby::parse(&bytes).is_err(),
        "du bruit aléatoire a été accepté comme lobby"
    );
}

#[test]
fn blob_tronque_a_toute_longueur_ne_panique_jamais() {
    let blob = blob_reel();
    // Un fichier lu pendant son écriture est un préfixe du fichier complet : on exerce exactement
    // ce cas, à tous les points de troncature (pas de 97 pour rester rapide).
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

- [ ] **Étape 2 : Lancer les tests**

Run: `cargo test -p storm-lobby --test robustness`
Expected: PASS. Une panique (`index out of bounds`, `byte index … is not a char boundary`) est un
défaut réel à corriger dans `parse` — remplacer l'indexation directe par `get()`/`get(..n)` et
propager `LobbyError::TooShort`.

- [ ] **Étape 3 : Vérifier que rien n'a régressé**

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
- Produit : les deux chiffres qui décident du go/no-go — le taux de lobbies dont **les 10
  BattleTags** sont exacts (critère : **≥ 99 %**), et le taux dont **les équipes déduites par
  l'ordre** sont exactes, qui décide si l'on affiche deux équipes ou une liste plate de 10.

- [ ] **Étape 1 : Écrire le harnais**

```rust
//! Parité du parser autonome contre le parse complet, sur un dossier de replays.
//!
//! Usage : cargo run --release -p storm-lobby --example parity -- <dossier> [max]
//!
//! Deux mesures indépendantes, à ne pas confondre :
//!   - `battletags exacts` : le parser retrouve exactement les 10 BattleTags de la partie.
//!   - `équipes exactes`   : parmi ceux-là, la déduction par l'ordre (5+5) correspond à la vérité.
//! La première décide du go/no-go ; la seconde décide si le companion affiche deux équipes.

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
    par_build: BTreeMap<u32, (usize, usize)>, // build → (évalués, battletags exacts)
}

/// `{battletag: équipe}` d'après le parse complet, ou `None` si le replay est rejeté.
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
                        .push((label, format!("{} battletags décodés", lobby.players.len())));
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
                        t.divergents.push((label, "équipes divergentes".into()));
                    }
                }
            }
        }
    }

    let base = t.total - t.stats_rejete;
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };
    println!("replays vus          : {}", t.total);
    println!("écartés (stats)      : {}", t.stats_rejete);
    println!("base de comparaison  : {base}");
    println!(
        "battletags exacts    : {} ({:.2} %)",
        t.battletags_exacts,
        pct(t.battletags_exacts, base)
    );
    println!("erreurs de parse     : {}", t.lobby_err);
    println!(
        "équipes exactes      : {} / {} évaluables ({:.2} %)",
        t.equipes_exactes,
        t.equipes_evaluables,
        pct(t.equipes_exactes, t.equipes_evaluables)
    );
    println!("\npar build (build: battletags exacts/évalués)");
    for (build, (evalues, exacts)) in &t.par_build {
        println!("  {build}: {exacts}/{evalues}");
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

Expected: `battletags exacts : 3 (100.00 %)`. Un écart ici signale un bug du harnais, pas du
parser — l'oracle de la tâche 2 couvre déjà ces replays.

- [ ] **Étape 3 : Récupérer l'archive du box (lecture seule)**

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

Si le box est injoignable, **s'arrêter là et le signaler** — ne pas inventer un verdict sur le seul
corpus local de 4 replays, qui est déjà couvert par l'oracle et ne prouve rien à l'échelle.

- [ ] **Étape 4 : Lancer la parité sur l'archive complète**

```bash
cargo run --release -p storm-lobby --example parity -- /tmp/archive 2>&1 | tee /tmp/lobby-parity.txt
```

- [ ] **Étape 5 : Écrire le rapport go/no-go**

`docs/research/2026-08-27-lobby-parity.md` : la sortie du harnais, puis le verdict.

- **BattleTags ≥ 99 % → GO.**
- **BattleTags < 99 % → analyser les divergences par build** avant tout verdict. Si l'échec se
  concentre sur quelques builds anciens, c'est acceptable et à documenter (le companion sert le
  présent, pas 2024) — donner alors le taux **sur les builds de l'année en cours**, qui est le
  chiffre qui compte. Si l'échec est uniforme, c'est un no-go : le remonter à l'opérateur, ne pas
  ajuster le seuil.
- **Équipes** : donner le taux séparément, sans le mélanger au précédent. Sous ~99 %, la
  recommandation est de renvoyer `team: None` en toutes circonstances et d'afficher une liste plate
  de 10 joueurs plutôt que deux équipes dont une partie serait fausse. Écrire la recommandation ;
  la décision revient à l'opérateur.

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
- [ ] `docs/research/2026-08-27-lobby-parity.md` porte deux chiffres et un verdict
- [ ] `docs/STATUS.md` mis à jour (état + prochaine étape), conformément à `CLAUDE.md`

Puis : écrire le plan 2 (migration `0009`, module `lobby.rs`, routes, `/companion`, `/builds`),
sachant désormais que le héros et la carte seront saisis à la main, et que l'identité des joueurs
se résout par BattleTag contre l'archive.
