//! Oracle de parité : à partir du seul blob lobby, le parser doit retrouver les joueurs que le
//! parse complet du replay identifie — par BattleTag, seule identité que le format expose.
//!
//! Chaque `.StormReplay` porte le même blob que celui écrit en live par le jeu : chaque replay est
//! donc à la fois un échantillon et sa propre correction.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use storm_lobby::LobbyPlayer;

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
