//! `team = None` as soon as the number of decoded players is not exactly 10: it is the product
//! signal that catches custom games with observers (see
//! `docs/research/2026-08-27-lobby-parity.md`, section "Failure mode"). It is a contract of the
//! parser — it deserves a dedicated test, independent of any binary fixture.
//!
//! `parse` is pure (no I/O, no dependency on `storm-replay`/`storm-stats`): an in-memory
//! synthetic blob is enough, no need for a real `.StormReplay`. It is the first test of the
//! crate that depends on neither.

#![allow(clippy::expect_used, clippy::unwrap_used)]

/// A synthetic blob with `n` distinct BattleTags, separated by spaces — enough for the
/// production regex to find them all, without borrowing anything from a real replay.
fn blob_avec_n_joueurs(n: usize) -> String {
    (0..n)
        .map(|i| format!("joueur{i:02}#{:04}", 1000 + i))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn dix_joueurs_donnent_deux_equipes_de_cinq() {
    let blob = blob_avec_n_joueurs(10);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("valid synthetic blob");
    assert_eq!(lobby.players.len(), 10);

    for (i, p) in lobby.players.iter().enumerate() {
        let attendu = if i < 5 { Some(0) } else { Some(1) };
        assert_eq!(
            p.team,
            attendu,
            "player {i} ({}): expected team {attendu:?}, got {:?}",
            p.battletag(),
            p.team
        );
    }
}

#[test]
fn neuf_joueurs_ne_donnent_aucune_equipe() {
    let blob = blob_avec_n_joueurs(9);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("valid synthetic blob");
    assert_eq!(lobby.players.len(), 9);
    assert!(
        lobby.players.iter().all(|p| p.team.is_none()),
        "a team was deduced outside the exactly-10 case"
    );
}

#[test]
fn onze_joueurs_ne_donnent_aucune_equipe() {
    let blob = blob_avec_n_joueurs(11);
    let lobby = storm_lobby::parse(blob.as_bytes()).expect("valid synthetic blob");
    assert_eq!(lobby.players.len(), 11);
    assert!(
        lobby.players.iter().all(|p| p.team.is_none()),
        "a team was deduced outside the exactly-10 case"
    );
}
