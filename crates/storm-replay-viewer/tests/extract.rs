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
