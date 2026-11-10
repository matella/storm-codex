#![allow(clippy::unwrap_used, clippy::expect_used)]

use storm_replay::Replay;
use storm_replay_viewer::{build_model, LevelTick};

fn model() -> storm_replay_viewer::ViewerModel {
    let replay = Replay::open("tests/data/silver-city-aram.StormReplay").expect("open");
    build_model(&replay).expect("build_model")
}

#[test]
fn ten_hero_tracks_with_samples() {
    let m = model();
    assert_eq!(m.heroes.len(), 10, "10 heroes expected");
    for h in &m.heroes {
        assert!((1..=10).contains(&h.player_id));
        assert!(
            !h.samples.is_empty(),
            "player {} without samples",
            h.player_id
        );
    }
}

#[test]
fn all_coords_normalized() {
    let m = model();
    for h in &m.heroes {
        for s in &h.samples {
            assert!(
                (0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
                "coord outside [0,1]: {:?}",
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
    assert!(m.meta.duration_sec > 60.0, "duration too short");
    assert_eq!(m.meta.viewer_version, 7);
    assert!(m.meta.map_size[0] > 0.0 && m.meta.map_size[1] > 0.0);
}

// The safety net of the user→player mapping: the EXACT samples come from the positions
// (independent of the mapping), so "10 non-empty tracks" proves NOTHING about the mapping of
// the commands. These two assertions really cover it.
#[test]
fn command_densification_lands_on_right_player() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.samples.len()).sum();
    assert!(
        total > 2000,
        "command densification missing (total {total}) — user→player mapping broken?"
    );
    for h in &m.heroes {
        let exact = h.samples.iter().filter(|s| s.exact).count();
        let cmd = h.samples.iter().filter(|s| !s.exact).count();
        assert!(
            exact >= 1 && cmd >= 1,
            "player {}: exact={exact} cmd={cmd}",
            h.player_id
        );
        // Spatial consistency: a player clicks near where their hero is. A wrong
        // mapping (e.g. team shift) would move the clicks away from the exact positions.
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
            "player {}: clicks far from the positions (d={d:.2}) — suspicious mapping",
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
            "player {} without a life interval",
            h.player_id
        );
        for iv in &h.life {
            assert!(iv.from <= iv.to);
        }
        // strictly increasing and non-overlapping intervals
        for w in h.life.windows(2) {
            assert!(w[0].to <= w[1].from);
        }
    }
}

// player_toons() is authoritative: the server (Chunk 2) uses it to join the Postgres
// player metadata by toon_handle. Here we guarantee the shape of the contract.
#[test]
fn player_toons_authoritative_mapping() {
    let replay = Replay::open("tests/data/silver-city-aram.StormReplay").expect("open");
    let toons = storm_replay_viewer::player_toons(&replay).expect("player_toons");

    assert_eq!(toons.len(), 10, "10 players expected");

    // playerIds = exactly the set 1..=10 (unique, no gap).
    let ids: Vec<i64> = toons.iter().map(|(p, _)| *p).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    sorted_ids.dedup();
    assert_eq!(
        sorted_ids,
        (1..=10).collect::<Vec<_>>(),
        "playerIds != {{1..=10}}"
    );

    // all toon_handles non-empty.
    for (p, toon) in &toons {
        assert!(!toon.is_empty(), "player {p}: empty toon_handle");
    }

    // result sorted by increasing playerId.
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "player_toons not sorted by increasing playerId: {ids:?}"
    );
}

#[test]
fn structures_present_and_classified() {
    let m = model();
    assert!(
        m.structures.iter().any(|s| s.team == 0 && s.kind == "core"),
        "no core team 0"
    );
    assert!(
        m.structures.iter().any(|s| s.team == 1 && s.kind == "core"),
        "no core team 1"
    );
    for s in &m.structures {
        assert!(
            (0.0..=1.0).contains(&s.x) && (0.0..=1.0).contains(&s.y),
            "coord outside [0,1]: {:?} ({})",
            (s.x, s.y),
            s.kind
        );
        if let Some(d) = s.destroyed_at {
            assert!(
                (0.0..=m.meta.duration_sec).contains(&d),
                "destroyedAt out of bounds: {d} (duration {})",
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
        "events not sorted by increasing t"
    );
    for e in &m.events {
        assert!(
            (0.0..=m.meta.duration_sec).contains(&e.t),
            "event t out of bounds: {} (duration {})",
            e.t,
            m.meta.duration_sec
        );
    }
    assert!(
        m.events.iter().any(|e| e.kind == "takedown"),
        "no takedown event"
    );
}

// US-18: ability cast flash indicators. We do not try to identify the ability —
// just "a cast happened" — so a simple count + bounds + sort is enough to cover the
// contract on the extraction side.
#[test]
fn casts_present() {
    let m = model();
    let total: usize = m.heroes.iter().map(|h| h.casts.len()).sum();
    assert!(total > 100, "too few casts extracted (total {total})");
    for h in &m.heroes {
        for &t in &h.casts {
            assert!(
                (0.0..=m.meta.duration_sec).contains(&t),
                "player {}: cast out of bounds t={t} (duration {})",
                h.player_id,
                m.meta.duration_sec
            );
        }
        assert!(
            h.casts.windows(2).all(|w| w[0] <= w[1]),
            "player {}: casts not sorted",
            h.player_id
        );
    }
}

// US-19: talent/level indicators. We do not check the VALUES of talent_id (V1 =
// reference-data-free, always None) but the SHAPE: at least one hero with picks (strictly
// increasing tier), and non-empty levels with a non-decreasing level per team.
#[test]
fn talents_and_levels() {
    let m = model();

    assert!(
        m.heroes.iter().any(|h| !h.talents.is_empty()),
        "no hero with extracted talents"
    );
    for h in &m.heroes {
        for tp in &h.talents {
            assert!(
                (0.0..=m.meta.duration_sec).contains(&tp.t),
                "player {}: talent out of bounds t={} (duration {})",
                h.player_id,
                tp.t,
                m.meta.duration_sec
            );
        }
        assert!(
            h.talents.windows(2).all(|w| w[0].tier < w[1].tier),
            "player {}: tiers not strictly increasing: {:?}",
            h.player_id,
            h.talents.iter().map(|t| t.tier).collect::<Vec<_>>()
        );
    }

    assert!(!m.levels.is_empty(), "levels vide");
    for lt in &m.levels {
        assert!(
            (0.0..=m.meta.duration_sec).contains(&lt.t),
            "level out of bounds t={} (duration {})",
            lt.t,
            m.meta.duration_sec
        );
        assert!(
            (0..=1).contains(&lt.team),
            "team outside {{0,1}}: {}",
            lt.team
        );
    }
    // Non-decreasing per team, once sorted/grouped by team then t.
    for team in [0i64, 1i64] {
        let mut lv: Vec<&LevelTick> = m.levels.iter().filter(|l| l.team == team).collect();
        lv.sort_by(|a, b| a.t.total_cmp(&b.t));
        assert!(
            lv.windows(2).all(|w| w[0].level <= w[1].level),
            "team {team}: levels not non-decreasing: {:?}",
            lv.iter().map(|l| l.level).collect::<Vec<_>>()
        );
    }
}

// US-21..24: silver-city is an ARAM (not Braxis, not a known gap map) → no objective,
// no warning. The [0, duration] bounds contract is checked even if the list is empty (it
// is here), to document the expected invariant as soon as another map feeds it.
#[test]
fn objectives_framework() {
    let m = model();
    assert!(
        m.objectives.is_empty(),
        "silver-city (ARAM) must not produce any objective"
    );
    assert!(m.warnings.is_empty(), "silver-city is not a known gap map");
    for o in &m.objectives {
        assert!(
            (0.0..=m.meta.duration_sec).contains(&o.t),
            "objective t out of bounds: {} (duration {})",
            o.t,
            m.meta.duration_sec
        );
    }
}

// US-26: bounded minion/camp samples, in controlled volume (dedup + cap in extract.rs). No
// non-emptiness requirement (some maps/replays may have few) but silver-city
// (ARAM, constant minions) must produce some.
#[test]
fn minions_bounded() {
    let m = model();
    assert!(
        m.minions.len() < 20000,
        "too many minions: {}",
        m.minions.len()
    );
    for ms in &m.minions {
        assert!(
            (0.0..=1.0).contains(&ms.x) && (0.0..=1.0).contains(&ms.y),
            "minion coord outside [0,1]: {:?}",
            ms
        );
        assert!(
            matches!(ms.team, -1..=1),
            "minion team outside {{0,1,-1}}: {}",
            ms.team
        );
    }
}

#[test]
fn golden_json_stable() {
    let m = model();
    // exclude viewerVersion from the comparison (deliberate bump → regenerate the golden)
    let mut v = serde_json::to_value(&m).unwrap();
    v["meta"]["viewerVersion"] = serde_json::json!("<ignored>");
    // US-26: minions/camps are a high volume, non-deterministic in their detail — excluded from the
    // golden (like viewerVersion) so as not to bloat the fixture; covered by `minions_bounded`.
    v["minions"] = serde_json::json!("<excluded>");
    let path = "tests/data/silver-city-aram.golden.json";
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        // Written prettily for human reading. The ORDER of the keys depends on the serde_json features
        // (see below) — so we do not rely on it for the comparison.
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }
    let want_str = std::fs::read_to_string(path).expect("golden manquant — lancer UPDATE_GOLDEN=1");
    let want: serde_json::Value = serde_json::from_str(&want_str).expect("golden JSON invalide");
    // SEMANTIC comparison (independent of the key order), not a String equality:
    // under `cargo test --workspace`, Cargo feature unification enables
    // `serde_json/preserve_order` (requested by storm-stats) → keys in insertion order; otherwise
    // alphabetical order (BTreeMap). Comparing `Value`s immunises the test against that switch.
    assert_eq!(v, want);
}
