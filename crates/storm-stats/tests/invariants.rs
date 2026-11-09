//! Structural invariants of the full pipeline (decode + stats) on a real committed replay.
//! Complements the Node parity harness (not runnable in CI): any gross regression of the
//! engine (messages, timeline, score, draft) breaks here without an external dependency.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

/// Replay of storm-replay's mini-corpus (committed) — a complete 2026 ARAM game.
fn replay() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
    let out =
        storm_stats::process_replay(&path, "2026-06-09 20.35.02 Industrial District.StormReplay");
    assert_eq!(
        out.status, 1,
        "reference replay not parsed ({})",
        out.status
    );
    out.to_json()
}

#[test]
fn identite_et_coherence_du_match() {
    let json = replay();
    let m = &json["match"];
    assert_eq!(m["map"], "Industrial District");
    let length = m["length"].as_f64().expect("length");
    assert!(length > 60.0, "abnormally short game: {length}");
    // loopLength ≈ 16 loops/s; the duration in seconds must be consistent with it (wide margin:
    // `length` is counted from the start of the game, after draft/loading)
    let loops = m["loopLength"].as_f64().expect("loopLength");
    assert!(
        (loops / 16.0) > length && (loops / 16.0) < length + 120.0,
        "loopLength {loops} inconsistent with length {length}"
    );
    let winner = m["winner"].as_i64().expect("winner");
    assert!(winner == 0 || winner == 1);

    // 10 players, 5 per team, 5 winners, all on the `winner` side
    let ps = json["players"].as_object().expect("players");
    assert_eq!(ps.len(), 10);
    for team in [0, 1] {
        assert_eq!(
            ps.values().filter(|p| p["team"] == team).count(),
            5,
            "team {team} incomplete"
        );
    }
    for p in ps.values() {
        assert_eq!(
            p["win"] == true,
            p["team"] == winner,
            "win/team inconsistent"
        );
    }
}

#[test]
fn messages_chat_pings_annonces_bien_formes() {
    let json = replay();
    let msgs = json["match"]["messages"].as_array().expect("messages");
    assert!(!msgs.is_empty(), "no message");
    let toons: Vec<&str> = json["players"]
        .as_object()
        .expect("players")
        .keys()
        .map(String::as_str)
        .collect();

    let mut last_loop = i64::MIN;
    let (mut chats, mut pings) = (0, 0);
    for msg in msgs {
        // chronological order (the front relies on it)
        let loop_ = msg["loop"].as_i64().expect("loop");
        assert!(loop_ >= last_loop, "messages not sorted by loop");
        last_loop = loop_;
        // the author is one of the 10 players (observers are filtered)
        let player = msg["player"].as_str().expect("player");
        assert!(toons.contains(&player), "unknown author: {player}");
        assert!(msg["team"] == 0 || msg["team"] == 1);
        // payload per type (MessageType from constants.json)
        match msg["type"].as_i64().expect("type") {
            0 => {
                chats += 1;
                assert!(
                    msg["text"].as_str().is_some_and(|t| !t.is_empty()),
                    "chat without text"
                );
            }
            1 => {
                pings += 1;
                assert!(
                    msg["point"]["x"].is_number() && msg["point"]["y"].is_number(),
                    "ping without coordinates"
                );
            }
            5 => assert!(msg["announcement"].is_object() || msg["announcement"].is_number()),
            2 => panic!("LoadingProgress must be filtered"),
            t => panic!("unexpected message type: {t}"),
        }
    }
    // this specific replay contains chat and pings (checked when the test was written)
    assert!(chats >= 1, "chat expected in the reference replay");
    assert!(pings >= 1, "pings expected in the reference replay");
}

#[test]
fn score_talents_takedowns_presents() {
    let json = replay();
    let ps = json["players"].as_object().expect("players");
    for (toon, p) in ps {
        let g = &p["gameStats"];
        for k in [
            "Takedowns",
            "Deaths",
            "HeroDamage",
            "ExperienceContribution",
        ] {
            assert!(g[k].is_number(), "{toon}: gameStats.{k} missing");
        }
        assert!(!p["hero"].as_str().unwrap_or("").is_empty(), "empty hero");
        // complete game (level 10+) → at least 4 talent tiers chosen
        let talents = p["talents"].as_object().expect("talents");
        assert!(
            talents.keys().filter(|k| k.starts_with("Tier")).count() >= 4,
            "{toon}: incomplete talents"
        );
    }
    // the match's takedowns reference known victims, with at least one killer
    let tds = json["match"]["takedowns"].as_array().expect("takedowns");
    assert!(!tds.is_empty());
    for td in tds {
        let victim = td["victim"]["player"].as_str().expect("victime");
        assert!(ps.contains_key(victim), "unknown victim: {victim}");
        assert!(
            td["killers"].as_array().is_some_and(|k| !k.is_empty()),
            "takedown without a killer"
        );
    }
}

#[test]
fn timeline_xp_monotone() {
    let json = replay();
    let xp = json["match"]["XPBreakdown"]
        .as_array()
        .expect("XPBreakdown");
    assert!(xp.len() > 1, "XPBreakdown vide");
    let mut last = f64::MIN;
    for point in xp {
        let t = point["time"].as_f64().expect("time");
        assert!(t >= last, "XPBreakdown not sorted");
        last = t;
    }
}
