//! Tests d'intégration sur le mini-corpus committé (3 replays, `tests/data/`).
//! Chaque replay doit livrer ses 7 streams avec des invariants de partie 5v5.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use storm_replay::{Replay, Value};

fn mini_corpus() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 3, "mini-corpus incomplet");
    files
}

#[test]
fn seven_streams_decode_with_invariants() {
    for path in mini_corpus() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let replay = Replay::open(&path).unwrap_or_else(|e| panic!("{name}: open: {e}"));

        // header
        assert!(replay.header.base_build > 90_000, "{name}: build {}", replay.header.base_build);
        assert!(replay.header.elapsed_game_loops > 0, "{name}: durée nulle");

        // details (vue typée)
        let details = replay.details().unwrap_or_else(|e| panic!("{name}: details: {e}"));
        assert!(!details.title.is_empty(), "{name}: carte vide");
        assert_eq!(details.players.len(), 10, "{name}: joueurs");
        assert_eq!(
            details.players.iter().filter(|p| p.result == 1).count(),
            5,
            "{name}: vainqueurs"
        );
        for p in &details.players {
            assert!(!p.name.is_empty() && !p.hero.is_empty(), "{name}: joueur incomplet");
            assert!(p.toon_handle.contains("-Hero-"), "{name}: toon {}", p.toon_handle);
        }

        // initdata
        let initdata = replay.initdata_raw().unwrap_or_else(|e| panic!("{name}: initdata: {e}"));
        assert!(
            initdata.field("m_syncLobbyState").is_some(),
            "{name}: initdata sans m_syncLobbyState"
        );

        // attributes
        let attrs = replay.attributes().unwrap_or_else(|e| panic!("{name}: attributes: {e}"));
        assert!(!attrs.scopes.is_empty(), "{name}: attributes vides");

        // tracker events
        let tracker = replay.tracker_events().unwrap_or_else(|e| panic!("{name}: tracker: {e}"));
        assert!(tracker.len() > 1_000, "{name}: {} tracker events", tracker.len());
        let has = |events: &[Value], suffix: &str| {
            events.iter().any(|e| {
                e.field("_event")
                    .and_then(Value::as_str_lossy)
                    .is_some_and(|n| n.ends_with(suffix))
            })
        };
        assert!(has(&tracker, "SScoreResultEvent"), "{name}: pas de score final");
        assert!(has(&tracker, "SStatGameEvent"), "{name}: pas de stat events");

        // game events (le plus gros stream)
        let game = replay.game_events().unwrap_or_else(|e| panic!("{name}: game events: {e}"));
        assert!(game.len() > tracker.len(), "{name}: {} game events", game.len());

        // message events (peut être vide si personne n'a parlé/ping — décodage sans erreur suffit)
        let _messages =
            replay.message_events().unwrap_or_else(|e| panic!("{name}: messages: {e}"));

        // mini-corpus ⊂ builds connus du moment de génération… sauf si plus récent que les
        // tables : le fallback doit alors être signalé, jamais silencieux.
        if let Some((asked, used)) = replay.protocol_fallback() {
            assert!(asked > used, "{name}: fallback incohérent {asked}->{used}");
        }
    }
}
