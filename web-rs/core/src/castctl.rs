//! Pure logic of the cast control room (`/cast`): the parts of `castState.ts`, `draftReplay.ts` and the cast
//! components that the overlay does not need. The overlay's own helpers are in `logic.rs` and `scenes.rs`.

use crate::model::{CastSeries, DraftAction, DraftOrder, DraftStep, Scene, Side, Team};

/// Selectable maps (one list shared with the Draft console).
pub const MAPS: [&str; 14] = [
    "Alterac Pass",
    "Battlefield of Eternity",
    "Braxis Holdout",
    "Blackheart's Bay",
    "Cursed Hollow",
    "Dragon Shire",
    "Garden of Terror",
    "Hanamura Temple",
    "Infernal Shrines",
    "Sky Temple",
    "Tomb of the Spider Queen",
    "Towers of Doom",
    "Volskaya Foundry",
    "Warhead Junction",
];

/// Scene tabs: `(scene, wire id, label)`.
pub const SCENES: [(Scene, &str, &str); 5] = [
    (Scene::Lineups, "lineups", "Lineups"),
    (Scene::Series, "series", "Series"),
    (Scene::Draft, "draft", "Draft"),
    (Scene::Ingame, "ingame", "In-game"),
    (Scene::Hide, "hide", "Hide"),
];

pub fn scene_id(s: Scene) -> &'static str {
    SCENES.iter().find(|x| x.0 == s).map_or("hide", |x| x.1)
}

pub fn scene_label(s: Scene) -> &'static str {
    SCENES.iter().find(|x| x.0 == s).map_or("", |x| x.2)
}

pub fn side_id(s: Side) -> &'static str {
    match s {
        Side::Blue => "blue",
        Side::Red => "red",
    }
}

/// Local scene requested by `?scene=` (target of the `/draft` redirect); anything else is ignored.
pub fn scene_from_search(search: &str) -> Option<Scene> {
    let q = search.strip_prefix('?').unwrap_or(search);
    let v = q.split('&').find_map(|kv| kv.strip_prefix("scene="))?;
    SCENES.iter().find(|x| x.1 == v).map(|x| x.0)
}

/// Adds or removes `map` from the pool being typed, keeping the order of the others.
pub fn toggle_pool(names: &[String], map: &str) -> Vec<String> {
    if names.iter().any(|n| n == map) {
        names.iter().filter(|n| *n != map).cloned().collect()
    } else {
        let mut v = names.to_vec();
        v.push(map.to_string());
        v
    }
}

/// A winner can be set when game `n` exists and has none.
pub fn can_set_winner(s: &CastSeries, n: u32) -> bool {
    s.games
        .iter()
        .find(|g| g.number == n)
        .is_some_and(|g| g.winner.is_none())
}

/// R21: a hand-entered winner can be corrected while the game has no replay summary.
pub fn can_change_winner(s: &CastSeries, n: u32) -> bool {
    s.games
        .iter()
        .find(|g| g.number == n)
        .is_some_and(|g| g.winner.is_some() && g.summary.is_none())
}

/// R21: removable game = the last one, with neither winner nor summary.
pub fn can_remove_game(s: &CastSeries, n: u32) -> bool {
    s.games.iter().find(|g| g.number == n).is_some_and(|g| {
        !s.games.iter().any(|x| x.number > n) && g.winner.is_none() && g.summary.is_none()
    })
}

/// Local input of a team reconciled with the server: local edits are kept, but the server's logo (file upload) wins
/// over an older local one unless the operator typed the logo URL themselves.
pub fn reconcile_team(local: &Team, server: &Team, touched_logo: bool) -> Team {
    if touched_logo {
        local.clone()
    } else {
        Team {
            logo: server.logo.clone(),
            ..local.clone()
        }
    }
}

/// Team typed in the rail, resynchronised from the server: adopted when the local input has not diverged from the last
/// server version seen (`base`), kept otherwise.
pub fn resync_team(local: &Team, base: &Team, server: &Team) -> Team {
    if local == base {
        server.clone()
    } else {
        local.clone()
    }
}

/// Bounds (s) of the slideshow interval (R25): the range the server accepts.
pub const INTERVAL_MIN: i64 = 1;
pub const INTERVAL_MAX: i64 = 3600;

/// Interval input (s) clamped to the bounds; `None` when it is not a number.
pub fn interval_seconds(raw: &str) -> Option<i64> {
    if raw.trim().is_empty() {
        return None;
    }
    let v = raw.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some((v.round() as i64).clamp(INTERVAL_MIN, INTERVAL_MAX))
}

/// Message for a refused replay upload; `side_mismatch` (R16) gets an instruction, other codes stay as they are.
pub fn replay_error_text(code: &str) -> String {
    match code {
        "series_changed" => "series_changed: the series was replaced after these replays were read. Read them again for the current series.".into(),
        "map_mismatch" => "map_mismatch: this replay is not from the map chosen for its game. Pick the replay's own map in the list.".into(),
        "MapNotChosen" => "MapNotChosen: this map is not picked or decider in the pool, or it is already played.".into(),
        "UnknownMap" => "UnknownMap: this map is not in the series pool.".into(),
        "SeriesInactive" => "SeriesInactive: create or start a series first.".into(),
        "not_custom" => "not_custom: only Custom games can be read from a replay.".into(),
        "parse_failed" => "parse_failed: the replay could not be decoded.".into(),
        "incomplete" => "incomplete: the replay lacks the winner, duration or takedowns.".into(),
        "side_mismatch" => "side_mismatch: the replay's winner contradicts the recorded winner and no linked player tells the sides apart. Tick \"Swap sides\" if this series' Blue team played as the replay's second team, otherwise check the recorded winner.".into(),
        other => other.into(),
    }
}

/// Plain-language text of the cast API error codes (the raw code stays as a fallback).
pub fn friendly_error(code: &str) -> String {
    let known = match code {
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
        "series_changed" => {
            "The series changed while the replay was decoding. Load the replay again."
        }
        "persist_failed" => "The server could not save the change. Try again.",
        "bad_index" => "Slide number out of range.",
        "series_active" => "The series is running: edit teams and score in the rail.",
        _ => return replay_error_text(code),
    };
    known.into()
}

pub const SLIDE_NAMES: [&str; 5] = ["Maps", "Hero damage", "Healing", "Siege", "Takedowns"];

/// Slide label in the console: `(text, title)`, with the "auto" reminder while the rotation runs.
pub fn slide_label(s: &CastSeries, tick: usize) -> (String, String) {
    let n = crate::logic::slide_count(s);
    let i = crate::logic::slide_index(s, tick);
    let name = SLIDE_NAMES.get(i).copied().unwrap_or("");
    if s.slide.auto && n > 1 {
        (
            format!("{} / {n} · auto", i + 1),
            format!(
                "{name}. Rotation every {} s, driven by the server: the preview and the OBS source show the same slide.",
                (s.slide.interval_ms as f64 / 1000.0).round() as i64
            ),
        )
    } else {
        (format!("{} / {n}", i + 1), name.to_string())
    }
}

/// Note shown after an upload: a forced swap contradicts the linked players.
pub fn replay_note(forced_against_evidence: bool) -> Option<&'static str> {
    forced_against_evidence.then_some("Sides swapped against the evidence: the linked players say this replay was NOT swapped. Check the winner and the player rows.")
}

/// Import order of a batch: ascending game date, undated last, ties in drop order. Indices in import order.
pub fn replay_import_order(played_at: &[Option<String>]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..played_at.len()).collect();
    idx.sort_by(|&a, &b| match (&played_at[a], &played_at[b]) {
        (x, y) if x == y => a.cmp(&b),
        (None, _) => std::cmp::Ordering::Greater,
        (_, None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    });
    idx
}

/// Proposed assignment of a replay batch to games: each replay aims at its own map when it can still be started
/// (`candidates`) and no earlier replay took it; otherwise `None` (the operator chooses or skips it).
pub fn propose_imports(
    items: &[(Option<String>, Option<String>)],
    candidates: &[String],
) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = vec![None; items.len()];
    let mut used: Vec<String> = Vec::new();
    let dates: Vec<Option<String>> = items.iter().map(|i| i.1.clone()).collect();
    for i in replay_import_order(&dates) {
        if let Some(m) = &items[i].0 {
            if candidates.contains(m) && !used.contains(m) {
                out[i] = Some(m.clone());
                used.push(m.clone());
            }
        }
    }
    out
}

/// Keyboard navigation of the scene bar (manual activation: arrows only move the focus). `None` = no effect.
pub fn tab_key_target(i: usize, key: &str, n: usize) -> Option<usize> {
    match key {
        "ArrowRight" => Some((i + 1) % n),
        "ArrowLeft" => Some((i + n - 1) % n),
        "Home" => Some(0),
        "End" => Some(n - 1),
        _ => None,
    }
}

/// Text of a mode-A player in the preview, from the state of the recent-heroes query.
pub fn mode_a_hint(
    linked: bool,
    heroes: usize,
    loading: bool,
    error: bool,
) -> Option<&'static str> {
    if !linked {
        Some("hidden (unlinked)")
    } else if loading {
        Some("loading…")
    } else if error {
        Some("recent heroes unavailable (error)")
    } else if heroes > 0 {
        None
    } else {
        Some("hidden (under 5 recent games)")
    }
}

pub struct SetupStep {
    pub id: &'static str,
    pub label: &'static str,
    pub done: bool,
}

/// Setup steps of a series, in order (console rail).
pub fn setup_steps(s: &CastSeries) -> Vec<SetupStep> {
    let ready =
        |t: &Team| !t.name.trim().is_empty() && t.players.iter().all(|p| !p.name.trim().is_empty());
    let teams_ready = ready(&s.blue) && ready(&s.red);
    vec![
        SetupStep {
            id: "series",
            label: "Series",
            done: s.active,
        },
        SetupStep {
            id: "teams",
            label: "Teams & players",
            done: s.active && teams_ready,
        },
        SetupStep {
            id: "games",
            label: "Maps & games",
            done: !s.games.is_empty(),
        },
    ]
}

/// What is left to do, in one sentence; `None` when everything is done.
pub fn next_hint(s: &CastSeries) -> Option<&'static str> {
    let step = setup_steps(s).into_iter().find(|x| !x.done)?;
    Some(match step.id {
        "series" => "Next: choose the map pool and Best-of, then press New series.",
        "teams" => "Next: name both teams and fill in the five players of each (Save teams).",
        _ => "Next: pick a map from the pool and press Start game.",
    })
}

// ── draft from a replay ────────────────────────────────────────────────────────

/// A phase = block of consecutive steps with the same order role (same team).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phase {
    pub start: usize,
    pub len: usize,
    pub action: DraftAction,
    pub order: DraftOrder,
}

pub fn phases_of(steps: &[DraftStep]) -> Vec<Phase> {
    let mut out: Vec<Phase> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        match out.last_mut() {
            Some(last) if last.order == s.order => last.len += 1,
            _ => out.push(Phase {
                start: i,
                len: 1,
                action: s.action,
                order: s.order,
            }),
        }
    }
    out
}

/// Index of the phase revealed by the next "Next" (`None` when the draft is over).
pub fn current_phase(phases: &[Phase], cursor: usize) -> Option<usize> {
    phases
        .iter()
        .position(|p| cursor >= p.start && cursor < p.start + p.len)
}

pub fn phase_label(p: &Phase) -> String {
    let a = if p.action == DraftAction::Ban {
        "ban"
    } else {
        "pick"
    };
    if p.len > 1 {
        format!("{a} ×{}", p.len)
    } else {
        a.to_string()
    }
}

/// Seconds left before the next auto tick (0 when there is no auto play).
pub fn seconds_left(next_at_ms: Option<f64>, now_ms: f64) -> i64 {
    next_at_ms.map_or(0, |n| ((n - now_ms) / 1000.0).ceil().max(0.0) as i64)
}

/// Readable message of a replay-load error code.
pub fn load_error_message(code: Option<&str>) -> &'static str {
    match code {
        Some("not_custom") => "not a custom game — only custom games are accepted",
        Some("no_draft") => "no draft found in this replay",
        Some("parse_failed") => "replay could not be read",
        Some("empty_body") => "empty file",
        _ => "upload failed",
    }
}

pub const OBS_SKINS: [&str; 4] = ["nexus", "glass", "tactical", "mono"];

/// URL of the OBS browser source for an origin and a skin.
pub fn obs_overlay_url(origin: &str, skin: &str) -> String {
    format!("{origin}/cast/overlay?skin={skin}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_from_the_query_string() {
        assert_eq!(scene_from_search("?scene=draft"), Some(Scene::Draft));
        assert_eq!(scene_from_search("?x=1&scene=hide"), Some(Scene::Hide));
        assert_eq!(scene_from_search("?scene=nope"), None);
        assert_eq!(scene_from_search(""), None);
    }

    #[test]
    fn the_pool_toggles_without_reordering() {
        let p = vec!["A".to_string(), "B".to_string()];
        assert_eq!(toggle_pool(&p, "A"), vec!["B".to_string()]);
        assert_eq!(
            toggle_pool(&p, "C"),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn interval_input_is_clamped() {
        assert_eq!(interval_seconds(""), None);
        assert_eq!(interval_seconds("abc"), None);
        assert_eq!(interval_seconds("0.2"), Some(1));
        assert_eq!(interval_seconds("9999"), Some(3600));
        assert_eq!(interval_seconds("12.6"), Some(13));
    }

    #[test]
    fn import_order_is_by_date_then_drop_order() {
        let d = |s: &str| Some(s.to_string());
        let order = replay_import_order(&[None, d("2026-02"), d("2026-01"), d("2026-02")]);
        assert_eq!(order, vec![2, 1, 3, 0]);
    }

    #[test]
    fn imports_take_their_own_map_once() {
        let items = vec![
            (
                Some("Sky Temple".to_string()),
                Some("2026-01-02".to_string()),
            ),
            (
                Some("Sky Temple".to_string()),
                Some("2026-01-01".to_string()),
            ),
            (Some("Cursed Hollow".to_string()), None),
        ];
        let cand = vec!["Sky Temple".to_string()];
        assert_eq!(
            propose_imports(&items, &cand),
            vec![None, Some("Sky Temple".to_string()), None]
        );
    }

    #[test]
    fn tab_keys_only_move_the_focus() {
        assert_eq!(tab_key_target(0, "ArrowLeft", 5), Some(4));
        assert_eq!(tab_key_target(4, "ArrowRight", 5), Some(0));
        assert_eq!(tab_key_target(2, "Home", 5), Some(0));
        assert_eq!(tab_key_target(2, "End", 5), Some(4));
        assert_eq!(tab_key_target(2, "Enter", 5), None);
    }

    #[test]
    fn phases_group_consecutive_steps_of_one_role() {
        let st = |o, a| DraftStep {
            order: o,
            action: a,
        };
        use DraftAction::*;
        use DraftOrder::*;
        let steps = [
            st(First, Ban),
            st(Second, Ban),
            st(Second, Ban),
            st(First, Pick),
        ];
        let ph = phases_of(&steps);
        assert_eq!(ph.len(), 3);
        assert_eq!(phase_label(&ph[1]), "ban ×2");
        assert_eq!(current_phase(&ph, 2), Some(1));
        assert_eq!(current_phase(&ph, 4), None);
    }

    #[test]
    fn error_codes_have_plain_text() {
        assert_eq!(
            friendly_error("NotInPool"),
            "This map is not in the series pool."
        );
        assert!(friendly_error("not_custom").starts_with("not_custom:"));
        assert_eq!(friendly_error("weird"), "weird");
    }

    #[test]
    fn seconds_left_never_goes_negative() {
        assert_eq!(seconds_left(None, 5.0), 0);
        assert_eq!(seconds_left(Some(10_500.0), 10_000.0), 1);
        assert_eq!(seconds_left(Some(1_000.0), 10_000.0), 0);
    }
}
