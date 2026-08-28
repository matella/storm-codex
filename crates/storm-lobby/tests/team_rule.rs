//! `team = None` dès que le nombre de joueurs décodés n'est pas exactement 10 : c'est le signal
//! produit qui attrape les parties personnalisées avec observateurs (cf.
//! `docs/research/2026-08-27-lobby-parity.md`, section « Mode d'échec »). C'est un contrat du
//! parseur — il mérite un test dédié, indépendant de tout fixture binaire.
//!
//! `parse` est pure (aucune I/O, aucune dépendance à `storm-replay`/`storm-stats`) : un blob
//! synthétique en mémoire suffit, pas besoin d'un `.StormReplay` réel. C'est le premier test du
//! crate à ne dépendre d'aucun des deux.

#![allow(clippy::expect_used, clippy::unwrap_used)]

/// Un blob synthétique avec `n` BattleTags distincts, séparés par des espaces — assez pour que la
/// regex de production les retrouve tous, sans rien emprunter à un replay réel.
fn blob_avec_n_joueurs(n: usize) -> String {
    (0..n)
        .map(|i| format!("joueur{i:02}#{:04}", 1000 + i))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn dix_joueurs_donnent_deux_equipes_de_cinq() {
    let blob = blob_avec_n_joueurs(10);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("blob synthétique valide");
    assert_eq!(lobby.players.len(), 10);

    for (i, p) in lobby.players.iter().enumerate() {
        let attendu = if i < 5 { Some(0) } else { Some(1) };
        assert_eq!(
            p.team, attendu,
            "joueur {i} ({}) : équipe attendue {attendu:?}, obtenue {:?}",
            p.battletag(),
            p.team
        );
    }
}

#[test]
fn neuf_joueurs_ne_donnent_aucune_equipe() {
    let blob = blob_avec_n_joueurs(9);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("blob synthétique valide");
    assert_eq!(lobby.players.len(), 9);
    assert!(
        lobby.players.iter().all(|p| p.team.is_none()),
        "une équipe a été déduite hors du cas 10 pile"
    );
}

#[test]
fn onze_joueurs_ne_donnent_aucune_equipe() {
    let blob = blob_avec_n_joueurs(11);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("blob synthétique valide");
    assert_eq!(lobby.players.len(), 11);
    assert!(
        lobby.players.iter().all(|p| p.team.is_none()),
        "une équipe a été déduite hors du cas 10 pile"
    );
}
