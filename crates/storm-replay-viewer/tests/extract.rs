#![allow(clippy::unwrap_used, clippy::expect_used)]

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
            assert!(
                (0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
                "coord hors [0,1]: {:?}",
                s
            );
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
    assert_eq!(m.meta.viewer_version, 4);
    assert!(m.meta.map_size[0] > 0.0 && m.meta.map_size[1] > 0.0);
}

// Le filet de sécurité du mapping user→player : les samples EXACTS viennent des positions
// (indépendants du mapping), donc « 10 tracks non vides » ne prouve RIEN sur le mapping des
// commandes. Ces deux assertions le couvrent réellement.
#[test]
fn command_densification_lands_on_right_player() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.samples.len()).sum();
    assert!(
        total > 2000,
        "densification commande absente (total {total}) — mapping user→player cassé ?"
    );
    for h in &m.heroes {
        let exact = h.samples.iter().filter(|s| s.exact).count();
        let cmd = h.samples.iter().filter(|s| !s.exact).count();
        assert!(
            exact >= 1 && cmd >= 1,
            "player {} : exact={exact} cmd={cmd}",
            h.player_id
        );
        // Cohérence spatiale : un joueur clique près de là où son héros se trouve. Un mapping
        // faux (ex. décalage d'équipe) éloignerait les clics des positions exactes.
        let mean = |it: &dyn Fn(&storm_replay_viewer::Sample) -> bool| {
            let v: Vec<_> = h.samples.iter().filter(|s| it(s)).collect();
            (
                v.iter().map(|s| s.x).sum::<f64>() / v.len() as f64,
                v.iter().map(|s| s.y).sum::<f64>() / v.len() as f64,
            )
        };
        let (ex, ey) = mean(&|s| s.exact);
        let (cx, cy) = mean(&|s| !s.exact);
        let d = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
        assert!(
            d < 0.4,
            "player {} : clics loin des positions (d={d:.2}) — mapping suspect",
            h.player_id
        );
    }
}

#[test]
fn life_intervals_ordered_and_bounded() {
    let m = model();
    for h in &m.heroes {
        assert!(
            !h.life.is_empty(),
            "player {} sans intervalle de vie",
            h.player_id
        );
        for iv in &h.life {
            assert!(iv.from <= iv.to);
        }
        // intervalles strictement croissants et non chevauchants
        for w in h.life.windows(2) {
            assert!(w[0].to <= w[1].from);
        }
    }
}

// player_toons() est autoritaire : le serveur (Chunk 2) s'en sert pour joindre les métadonnées
// joueurs de Postgres par toon_handle. On garantit ici la forme du contrat.
#[test]
fn player_toons_authoritative_mapping() {
    let replay = Replay::open("tests/data/silver-city-aram.StormReplay").expect("open");
    let toons = storm_replay_viewer::player_toons(&replay).expect("player_toons");

    assert_eq!(toons.len(), 10, "10 joueurs attendus");

    // playerIds = exactement l'ensemble 1..=10 (uniques, sans trou).
    let ids: Vec<i64> = toons.iter().map(|(p, _)| *p).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    sorted_ids.dedup();
    assert_eq!(
        sorted_ids,
        (1..=10).collect::<Vec<_>>(),
        "playerIds != {{1..=10}}"
    );

    // tous les toon_handle non vides.
    for (p, toon) in &toons {
        assert!(!toon.is_empty(), "player {p} : toon_handle vide");
    }

    // résultat trié par playerId croissant.
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "player_toons non trié par playerId croissant: {ids:?}"
    );
}

#[test]
fn structures_present_and_classified() {
    let m = model();
    assert!(
        m.structures.iter().any(|s| s.team == 0 && s.kind == "core"),
        "aucun core team 0"
    );
    assert!(
        m.structures.iter().any(|s| s.team == 1 && s.kind == "core"),
        "aucun core team 1"
    );
    for s in &m.structures {
        assert!(
            (0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
            "coord hors [0,1]: {:?} ({})",
            (s.x, s.y),
            s.kind
        );
        if let Some(d) = s.destroyed_at {
            assert!(
                (0.0..=m.meta.duration_sec).contains(&d),
                "destroyedAt hors bornes: {d} (duration {})",
                m.meta.duration_sec
            );
        }
    }
}

#[test]
fn feed_events_sorted_nonempty() {
    let m = model();
    assert!(!m.events.is_empty(), "events vide");
    assert!(
        m.events.windows(2).all(|w| w[0].t <= w[1].t),
        "events non triés par t croissant"
    );
    for e in &m.events {
        assert!(
            (0.0..=m.meta.duration_sec).contains(&e.t),
            "event t hors bornes: {} (duration {})",
            e.t,
            m.meta.duration_sec
        );
    }
    assert!(
        m.events.iter().any(|e| e.kind == "takedown"),
        "aucun event takedown"
    );
}

// US-18 : indicateurs flash de cast d'aptitude. On ne cherche pas à identifier l'aptitude —
// juste « un cast a eu lieu » — donc un simple comptage + bornage + tri suffit à couvrir le
// contrat côté extraction.
#[test]
fn casts_present() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.casts.len()).sum();
    assert!(total > 100, "trop peu de casts extraits (total {total})");
    for h in &m.heroes {
        for &t in &h.casts {
            assert!(
                (0.0..=m.meta.duration_sec).contains(&t),
                "player {} : cast hors bornes t={t} (duration {})",
                h.player_id,
                m.meta.duration_sec
            );
        }
        assert!(
            h.casts.windows(2).all(|w| w[0] <= w[1]),
            "player {} : casts non triés",
            h.player_id
        );
    }
}

#[test]
fn golden_json_stable() {
    let m = model();
    // exclure viewerVersion du comparé (bump volontaire → régénérer le golden)
    let mut v = serde_json::to_value(&m).unwrap();
    v["meta"]["viewerVersion"] = serde_json::json!("<ignored>");
    let path = "tests/data/silver-city-aram.golden.json";
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        // Écrit joliment pour la lecture humaine. L'ORDRE des clés dépend des features serde_json
        // (voir plus bas) — on ne s'y fie donc pas pour la comparaison.
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }
    let want_str = std::fs::read_to_string(path).expect("golden manquant — lancer UPDATE_GOLDEN=1");
    let want: serde_json::Value = serde_json::from_str(&want_str).expect("golden JSON invalide");
    // Comparaison SÉMANTIQUE (indépendante de l'ordre des clés), pas une égalité de String :
    // sous `cargo test --workspace`, l'unification des features Cargo active
    // `serde_json/preserve_order` (demandé par storm-stats) → clés en ordre d'insertion ; sinon
    // ordre alphabétique (BTreeMap). Comparer des `Value` immunise le test contre ce basculement.
    assert_eq!(v, want);
}
