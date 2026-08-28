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
