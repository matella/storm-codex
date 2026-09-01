//! Invariants structurels du pipeline complet (decode + stats) sur un replay réel committé.
//! Complète le harnais de parité Node (non exécutable en CI) : toute régression grossière du
//! moteur (messages, timeline, score, draft) casse ici sans dépendance externe.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

/// Replay du mini-corpus de storm-replay (committé) — partie ARAM complète de 2026.
fn replay() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
    let out = storm_stats::process_replay(&path, "2026-06-09 20.35.02 Industrial District.StormReplay");
    assert_eq!(out.status, 1, "replay de référence non parsé ({})", out.status);
    out.to_json()
}

#[test]
fn identite_et_coherence_du_match() {
    let json = replay();
    let m = &json["match"];
    assert_eq!(m["map"], "Industrial District");
    let length = m["length"].as_f64().expect("length");
    assert!(length > 60.0, "partie anormalement courte : {length}");
    // loopLength ≈ 16 boucles/s ; la durée en secondes doit en être cohérente (marge large :
    // `length` est comptée depuis le début de partie, après draft/chargement)
    let loops = m["loopLength"].as_f64().expect("loopLength");
    assert!(
        (loops / 16.0) > length && (loops / 16.0) < length + 120.0,
        "loopLength {loops} incohérent avec length {length}"
    );
    let winner = m["winner"].as_i64().expect("winner");
    assert!(winner == 0 || winner == 1);

    // 10 joueurs, 5 par équipe, 5 vainqueurs, tous du côté `winner`
    let ps = json["players"].as_object().expect("players");
    assert_eq!(ps.len(), 10);
    for team in [0, 1] {
        assert_eq!(
            ps.values().filter(|p| p["team"] == team).count(),
            5,
            "équipe {team} incomplète"
        );
    }
    for p in ps.values() {
        assert_eq!(p["win"] == true, p["team"] == winner, "win/team incohérents");
    }
}

#[test]
fn messages_chat_pings_annonces_bien_formes() {
    let json = replay();
    let msgs = json["match"]["messages"].as_array().expect("messages");
    assert!(!msgs.is_empty(), "aucun message");
    let toons: Vec<&str> = json["players"]
        .as_object()
        .expect("players")
        .keys()
        .map(String::as_str)
        .collect();

    let mut last_loop = i64::MIN;
    let (mut chats, mut pings) = (0, 0);
    for msg in msgs {
        // ordre chronologique (le front s'y fie)
        let loop_ = msg["loop"].as_i64().expect("loop");
        assert!(loop_ >= last_loop, "messages non triés par loop");
        last_loop = loop_;
        // l'auteur est un des 10 joueurs (les observateurs sont filtrés)
        let player = msg["player"].as_str().expect("player");
        assert!(toons.contains(&player), "auteur inconnu : {player}");
        assert!(msg["team"] == 0 || msg["team"] == 1);
        // charge utile par type (MessageType de constants.json)
        match msg["type"].as_i64().expect("type") {
            0 => {
                chats += 1;
                assert!(
                    msg["text"].as_str().is_some_and(|t| !t.is_empty()),
                    "chat sans texte"
                );
            }
            1 => {
                pings += 1;
                assert!(
                    msg["point"]["x"].is_number() && msg["point"]["y"].is_number(),
                    "ping sans coordonnées"
                );
            }
            5 => assert!(msg["announcement"].is_object() || msg["announcement"].is_number()),
            2 => panic!("LoadingProgress doit être filtré"),
            t => panic!("type de message inattendu : {t}"),
        }
    }
    // ce replay précis contient du chat et des pings (vérifié à l'écriture du test)
    assert!(chats >= 1, "chat attendu dans le replay de référence");
    assert!(pings >= 1, "pings attendus dans le replay de référence");
}

#[test]
fn score_talents_takedowns_presents() {
    let json = replay();
    let ps = json["players"].as_object().expect("players");
    for (toon, p) in ps {
        let g = &p["gameStats"];
        for k in ["Takedowns", "Deaths", "HeroDamage", "ExperienceContribution"] {
            assert!(g[k].is_number(), "{toon} : gameStats.{k} absent");
        }
        assert!(!p["hero"].as_str().unwrap_or("").is_empty(), "héros vide");
        // partie complète (niveau 10+) → au moins 4 paliers de talents choisis
        let talents = p["talents"].as_object().expect("talents");
        assert!(
            talents.keys().filter(|k| k.starts_with("Tier")).count() >= 4,
            "{toon} : talents incomplets"
        );
    }
    // les takedowns du match référencent des victimes connues, avec au moins un tueur
    let tds = json["match"]["takedowns"].as_array().expect("takedowns");
    assert!(!tds.is_empty());
    for td in tds {
        let victim = td["victim"]["player"].as_str().expect("victime");
        assert!(ps.contains_key(victim), "victime inconnue : {victim}");
        assert!(
            td["killers"].as_array().is_some_and(|k| !k.is_empty()),
            "takedown sans tueur"
        );
    }
}

#[test]
fn timeline_xp_monotone() {
    let json = replay();
    let xp = json["match"]["XPBreakdown"].as_array().expect("XPBreakdown");
    assert!(xp.len() > 1, "XPBreakdown vide");
    let mut last = f64::MIN;
    for point in xp {
        let t = point["time"].as_f64().expect("time");
        assert!(t >= last, "XPBreakdown non trié");
        last = t;
    }
}
