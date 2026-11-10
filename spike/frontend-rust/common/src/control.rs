//! Cast control room (`/cast`): pure helpers ported from `web/src/castState.ts` and `pages/Cast.tsx`.

use crate::model::{Scene, Team};

/// Scene bar, in order: `(scene, label)`.
pub const SCENES: [(Scene, &str); 5] = [
    (Scene::Lineups, "Lineups"),
    (Scene::Series, "Series"),
    (Scene::Draft, "Draft"),
    (Scene::Ingame, "In-game"),
    (Scene::Hide, "Hide"),
];

pub fn scene_label(s: Scene) -> &'static str {
    SCENES.iter().find(|(x, _)| *x == s).map(|(_, l)| *l).unwrap_or("")
}

pub fn scene_id(s: Scene) -> &'static str {
    match s {
        Scene::Lineups => "lineups",
        Scene::Series => "series",
        Scene::Draft => "draft",
        Scene::Ingame => "ingame",
        Scene::Hide => "hide",
    }
}

/// Working tab asked for by `?scene=`; any other value is ignored (target of the `/draft` redirect).
pub fn scene_from_search(search: &str) -> Option<Scene> {
    let v = search.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("scene="))?;
    SCENES.iter().find(|(s, _)| scene_id(*s) == v).map(|(s, _)| *s)
}

/// Keyboard navigation of the scene bar (manual activation: arrows only move the focus, never a scene on
/// air). `None` = key without effect.
pub fn tab_key_target(i: usize, key: &str, n: usize) -> Option<usize> {
    match key {
        "ArrowRight" => Some((i + 1) % n),
        "ArrowLeft" => Some((i + n - 1) % n),
        "Home" => Some(0),
        "End" => Some(n - 1),
        _ => None,
    }
}

/// Resync of a team editor with the server: the input is replaced by the new server version if it equals the
/// last server version seen (nothing modified, or modification cancelled), kept otherwise.
pub fn resync_team(local: &Team, base: &Team, server: &Team) -> Team {
    if local == base {
        server.clone()
    } else {
        local.clone()
    }
}

/// Plain-words message for a server error code (the raw code stays as a fallback).
pub fn friendly_error(code: &str) -> Option<&'static str> {
    Some(match code {
        "NotInPool" => "This map is not in the series pool.",
        "SeriesInactive" => "No series is running. Create one first.",
        "MapAlreadyPlayed" => "This map has already been played in this series.",
        "MapNotChosen" => "Pick or decide the map before starting the game.",
        "DuplicateDecider" => "A decider map is already set.",
        "UnknownMap" => "Unknown map.",
        "UnknownGame" => "No game with this number.",
        "WinnerAlreadySet" => "This game already has a winner.",
        "GameHasResult" => "This game already has a result.",
        "NotLastGame" => "Only the last game can be changed or removed.",
        "series_changed" => "The series changed while the replay was decoding. Load the replay again.",
        "persist_failed" => "The server could not save the change. Try again.",
        "bad_index" => "Slide number out of range.",
        "series_active" => "The series is running: edit teams and score in the rail.",
        _ => return None,
    })
}

const W: f64 = 1920.0;
const H: f64 = 1080.0;

/// Styles of the preview frame and of the iframe for a measured scale (`None` = not measured yet: the frame
/// keeps the 16:9 ratio and the iframe stays hidden, no image at an arbitrary scale).
pub fn preview_styles(scale: Option<f64>) -> (String, String) {
    match scale {
        None => (format!("aspect-ratio:{W} / {H}"), "visibility:hidden".into()),
        Some(s) => (format!("height:{}px", H * s), format!("transform:scale({s})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CastPlayer;

    #[test]
    fn scene_query_picks_the_working_tab_only_for_known_values() {
        assert_eq!(scene_from_search("?scene=draft"), Some(Scene::Draft));
        assert_eq!(scene_from_search("?x=1&scene=hide"), Some(Scene::Hide));
        assert_eq!(scene_from_search("?scene=nope"), None);
        assert_eq!(scene_from_search(""), None);
        assert_eq!(scene_label(Scene::Ingame), "In-game");
    }

    #[test]
    fn arrow_keys_only_move_the_focus_and_wrap() {
        assert_eq!(tab_key_target(4, "ArrowRight", 5), Some(0));
        assert_eq!(tab_key_target(0, "ArrowLeft", 5), Some(4));
        assert_eq!(tab_key_target(2, "Home", 5), Some(0));
        assert_eq!(tab_key_target(2, "End", 5), Some(4));
        assert_eq!(tab_key_target(2, "Enter", 5), None);
        assert_eq!(tab_key_target(2, " ", 5), None);
    }

    #[test]
    fn resync_keeps_a_modified_input_and_replaces_an_untouched_one() {
        let base = Team::named("A");
        let server = Team { record: "4-1".into(), ..Team::named("A") };
        assert_eq!(resync_team(&base, &base, &server), server);
        let edited = Team::named("A edited");
        assert_eq!(resync_team(&edited, &base, &server), edited);
    }

    #[test]
    fn errors_have_plain_words_when_known() {
        assert_eq!(friendly_error("NotInPool"), Some("This map is not in the series pool."));
        assert_eq!(friendly_error("whatever"), None);
    }

    #[test]
    fn preview_hides_the_iframe_until_the_scale_is_known() {
        assert_eq!(preview_styles(None), ("aspect-ratio:1920 / 1080".into(), "visibility:hidden".into()));
        assert_eq!(preview_styles(Some(0.5)), ("height:540px".into(), "transform:scale(0.5)".into()));
    }

    #[test]
    fn team_round_trips_through_json() {
        let t = Team {
            name: "A".into(),
            logo: None,
            record: "1-0".into(),
            players: vec![CastPlayer { name: "p".into(), tagline: "t".into(), toon: Some("2-Hero-1-1".into()) }],
            captain: Some(0),
        };
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Team>(&j).unwrap(), t);
    }
}
