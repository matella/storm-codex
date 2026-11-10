//! Pure cast-overlay logic, ported from `web/src/castState.ts`. Framework-free: both spike apps use it.

use crate::model::{CastSeries, Game, MapStatus, PoolMap, Scene, Side};

pub fn side_label(s: Side) -> &'static str {
    match s {
        Side::Blue => "Blue",
        Side::Red => "Red",
    }
}

/// Team name shown on screen; blank falls back to "Blue" / "Red".
pub fn display_name(name: &str, side: Side) -> String {
    if name.trim().is_empty() {
        side_label(side).to_string()
    } else {
        name.to_string()
    }
}

/// Displayed series score `(blue, red)`: the operator override wins, else the winners count.
pub fn series_score(s: &CastSeries) -> (u32, u32) {
    if let Some(o) = s.score_override {
        return o;
    }
    let n = |side| s.games.iter().filter(|g| g.winner == Some(side)).count() as u32;
    (n(Side::Blue), n(Side::Red))
}

/// Series decided (R22): a side reached ceil(bo/2) wins at the displayed score.
pub fn series_decided(s: &CastSeries) -> bool {
    let (b, r) = series_score(s);
    b.max(r) >= s.bo.div_ceil(2)
}

/// Game number on the badge. R9: game in progress (last one without a winner), else max + 1
/// (1 if none). R22: decided series shows the last game number, never n + 1.
pub fn badge_game_number(s: &CastSeries) -> u32 {
    let Some(last) = s.games.last() else { return 1 };
    let max = s.games.iter().map(|g| g.number).max().unwrap_or(0);
    if last.winner.is_none() || series_decided(s) {
        max
    } else {
        max + 1
    }
}

/// The overlay renders a scene when the series is active, or on the Draft scene (R19).
pub fn overlay_visible(s: &CastSeries) -> bool {
    s.active || s.scene == Scene::Draft
}

/// Maps that "Start game" could start: picked or decider, with no game yet (pool order).
pub fn startable_maps(s: &CastSeries) -> Vec<&str> {
    s.pool
        .iter()
        .filter(|m| matches!(m.status, MapStatus::Picked | MapStatus::Decider) && !s.games.iter().any(|g| g.map == m.name))
        .map(|m| m.name.as_str())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Blue,
    Red,
    Gold,
    Muted,
    None,
}

impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Tone::Blue => "tone-blue",
            Tone::Red => "tone-red",
            Tone::Gold => "tone-gold",
            Tone::Muted => "tone-muted",
            Tone::None => "tone-none",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapCardView {
    pub tone: Tone,
    pub label: String,
    pub struck: bool,
}

/// Card of the "Series" strip. Priority: played (WON / PLAYING) > NEXT > pool status.
pub fn map_card_view(m: &PoolMap, games: &[Game], next_map: Option<&str>, names: (&str, &str)) -> MapCardView {
    let by = |s: Option<Side>| match s {
        Some(Side::Red) => names.1,
        _ => names.0,
    };
    let view = |tone, label: &str, struck| MapCardView { tone, label: label.to_string(), struck };
    if let Some(g) = games.iter().find(|g| g.map == m.name) {
        return match g.winner {
            Some(Side::Blue) => view(Tone::Blue, "WON", false),
            Some(Side::Red) => view(Tone::Red, "WON", false),
            None => view(Tone::Muted, "PLAYING", false),
        };
    }
    if next_map == Some(m.name.as_str()) {
        return view(Tone::Gold, "NEXT", false);
    }
    match m.status {
        MapStatus::Banned => view(Tone::Muted, &format!("banned by {}", by(m.by)), true),
        MapStatus::Picked => view(
            if m.by == Some(Side::Red) { Tone::Red } else { Tone::Blue },
            &format!("picked by {}", by(m.by)),
            false,
        ),
        MapStatus::Decider => view(Tone::None, "decider", false),
        MapStatus::Available => view(Tone::None, "", false),
    }
}

/// `(columns, rows)` of the pool strip: up to 6 maps on one row, then rows of five.
pub fn strip_layout(n: usize) -> (usize, usize) {
    if n <= 6 {
        (n.max(1), 1)
    } else {
        (5, n.div_ceil(5))
    }
}

/// Game shown by the Series scene: the pinned one (if it exists), else the last.
pub fn shown_game(s: &CastSeries) -> Option<&Game> {
    match s.slide.game {
        Some(n) => s.games.iter().find(|g| g.number == n),
        None => s.games.last(),
    }
}

/// Number of breakdown tabs after the map strip (damage / healing / siege / takedowns).
pub const BREAKDOWN_TABS: usize = 4;

pub fn slide_count(s: &CastSeries) -> usize {
    if shown_game(s).is_some_and(|g| g.summary.is_some()) {
        1 + BREAKDOWN_TABS
    } else {
        1
    }
}

/// Rotation steps elapsed since the server anchor; 0 outside auto mode.
pub fn slide_tick(s: &CastSeries, now_ms: u64) -> usize {
    let sl = &s.slide;
    if !sl.auto || sl.interval_ms == 0 {
        return 0;
    }
    (now_ms.saturating_sub(sl.since_ms) / sl.interval_ms) as usize
}

/// Slide on screen: manual = server index modulo the slide count; auto = start at the index, advance by `tick`.
pub fn slide_index(s: &CastSeries, tick: usize) -> usize {
    (s.slide.index + if s.slide.auto { tick } else { 0 }) % slide_count(s)
}

/// One layer of the overlay: the scene shown and the series state that renders it.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneLayer {
    pub key: Scene,
    pub s: CastSeries,
    /// Stable identity (the "key" of the DOM node): given when the layer becomes current, kept when it leaves.
    pub id: u64,
}

/// Scene rendered by the overlay (`None` = nothing: hide, or inactive series outside the Draft scene).
pub fn shown_scene(s: Option<&CastSeries>, id: u64) -> Option<SceneLayer> {
    s.filter(|s| overlay_visible(s) && s.scene != Scene::Hide)
        .map(|s| SceneLayer { key: s.scene, s: s.clone(), id })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layers {
    pub cur: Option<SceneLayer>,
    pub leaving: Option<SceneLayer>,
}

/// Fade-out: when the scene changes the old layer stays mounted (a snapshot of the state that rendered
/// it, never the new series in the old scene) for the duration of the fade.
pub fn scene_transition(st: &Layers, s: Option<&CastSeries>) -> Layers {
    let next = shown_scene(s, 1);
    let same_key = next.as_ref().map(|n| n.key) == st.cur.as_ref().map(|c| c.key);
    if same_key {
        return match (next, &st.cur) {
            (Some(n), Some(cur)) if n.s != cur.s => {
                Layers { cur: Some(SceneLayer { id: cur.id, ..n }), leaving: st.leaving.clone() }
            }
            _ => st.clone(),
        };
    }
    let id = st.cur.iter().chain(st.leaving.iter()).map(|l| l.id).max().unwrap_or(0) + 1;
    Layers { cur: shown_scene(s, id), leaving: st.cur.clone() }
}

/// Map slug: lower case, apostrophes removed, whitespace runs -> dashes.
pub fn map_slug(map: &str) -> String {
    let lower = map.to_lowercase().replace(['\'', '\u{2019}'], "");
    lower.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Painted loading-screen art of a map (404 for maps without one, e.g. ARAM).
pub fn map_image(map: &str) -> String {
    format!("/images/battlegrounds/{}.png", map_slug(map))
}

/// Position of an In-game tab, as CSS percentages of the 16:9 frame.
pub struct Anchors {
    pub blue_inner_x: f64,
    pub red_inner_x: f64,
    pub top_y: f64,
    pub tab_h: f64,
    pub max_w: f64,
}

pub const INGAME_ANCHORS: Anchors = Anchors { blue_inner_x: 0.392, red_inner_x: 0.5965, top_y: 0.099, tab_h: 0.046, max_w: 0.26 };

pub fn tab_box(side: Side) -> String {
    let a = &INGAME_ANCHORS;
    let pct = |f: f64| format!("{:.2}%", f * 100.0);
    let base = format!("top:{};height:{};max-width:{};", pct(a.top_y), pct(a.tab_h), pct(a.max_w));
    match side {
        Side::Blue => format!("{base}right:{};", pct(1.0 - a.blue_inner_x)),
        Side::Red => format!("{base}left:{};", pct(a.red_inner_x)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{GameSummary, LineupMode};
    use crate::model::{Slide, Team};

    fn series(scene: Scene, active: bool) -> CastSeries {
        CastSeries {
            active,
            blue: Team::named("Blue team"),
            red: Team::named(" "),
            bo: 3,
            format: "ban".into(),
            score_override: None,
            pool: vec![],
            games: vec![],
            scene,
            slide: Slide { game: None, index: 0, auto: false, interval_ms: 10_000, since_ms: 0 },
            lineup_mode: LineupMode::Auto,
        }
    }

    pub(crate) fn summary() -> GameSummary {
        GameSummary {
            duration_secs: 600.0,
            winner: Side::Blue,
            takedowns: (3, 1),
            team_level: None,
            structures: None,
            merc_camps: None,
            objectives: None,
            bans: (vec![], vec![]),
            players: vec![],
            xp_lead: vec![],
        }
    }

    fn game(number: u32, map: &str, winner: Option<Side>) -> Game {
        Game { number, map: map.into(), winner, takedowns: (0, 0), summary: None }
    }

    #[test]
    fn blank_team_name_falls_back_to_side() {
        assert_eq!(display_name(" ", Side::Red), "Red");
        assert_eq!(display_name("Storm", Side::Blue), "Storm");
    }

    #[test]
    fn score_counts_winners_unless_overridden() {
        let mut s = series(Scene::Series, true);
        s.games = vec![game(1, "A", Some(Side::Blue)), game(2, "B", Some(Side::Red)), game(3, "C", Some(Side::Blue))];
        assert_eq!(series_score(&s), (2, 1));
        s.score_override = Some((0, 1));
        assert_eq!(series_score(&s), (0, 1));
    }

    #[test]
    fn badge_number_follows_r9_and_r22() {
        let mut s = series(Scene::Ingame, true);
        assert_eq!(badge_game_number(&s), 1);
        s.games = vec![game(1, "A", Some(Side::Blue))];
        assert_eq!(badge_game_number(&s), 2); // next game
        s.games.push(game(2, "B", None));
        assert_eq!(badge_game_number(&s), 2); // in progress
        s.games = vec![game(1, "A", Some(Side::Blue)), game(2, "B", Some(Side::Blue))];
        assert_eq!(badge_game_number(&s), 2); // decided: never n + 1
    }

    #[test]
    fn map_card_priority_is_played_then_next_then_pool_status() {
        let m = |status, by| PoolMap { name: "Sky Temple".into(), status, by };
        let names = ("Blue", "Red");
        let played = [game(1, "Sky Temple", Some(Side::Red))];
        let v = map_card_view(&m(MapStatus::Banned, Some(Side::Blue)), &played, Some("Sky Temple"), names);
        assert_eq!((v.tone, v.label.as_str()), (Tone::Red, "WON"));
        let v = map_card_view(&m(MapStatus::Banned, Some(Side::Blue)), &[], Some("Sky Temple"), names);
        assert_eq!(v.tone, Tone::Gold);
        let v = map_card_view(&m(MapStatus::Banned, Some(Side::Red)), &[], None, names);
        assert_eq!((v.label.as_str(), v.struck), ("banned by Red", true));
    }

    #[test]
    fn map_art_url_uses_the_slug() {
        assert_eq!(map_slug("Blackheart's Bay"), "blackhearts-bay");
        assert_eq!(map_image("Tomb of the Spider Queen"), "/images/battlegrounds/tomb-of-the-spider-queen.png");
    }

    #[test]
    fn strip_layout_rows() {
        assert_eq!(strip_layout(0), (1, 1));
        assert_eq!(strip_layout(6), (6, 1));
        assert_eq!(strip_layout(7), (5, 2));
        assert_eq!(strip_layout(11), (5, 3));
    }

    #[test]
    fn slideshow_advances_from_the_server_anchor() {
        let mut s = series(Scene::Series, true);
        s.games = vec![Game { summary: Some(summary()), ..game(1, "A", Some(Side::Blue)) }];
        s.slide = Slide { game: None, index: 1, auto: true, interval_ms: 1000, since_ms: 5_000 };
        assert_eq!(slide_tick(&s, 4_000), 0);
        assert_eq!(slide_tick(&s, 8_500), 3);
        assert_eq!(slide_index(&s, 3), (1 + 3) % 5);
        s.slide.auto = false;
        assert_eq!((slide_tick(&s, 8_500), slide_index(&s, 3)), (0, 1));
    }

    #[test]
    fn scene_change_keeps_the_old_layer_for_the_fade() {
        let a = series(Scene::Ingame, true);
        let st = scene_transition(&Layers::default(), Some(&a));
        assert_eq!(st.cur.as_ref().map(|l| (l.key, l.id)), Some((Scene::Ingame, 1)));
        let b = series(Scene::Series, true);
        let st2 = scene_transition(&st, Some(&b));
        assert_eq!(st2.cur.as_ref().map(|l| (l.key, l.id)), Some((Scene::Series, 2)));
        assert_eq!(st2.leaving.as_ref().map(|l| (l.key, l.s.scene)), Some((Scene::Ingame, Scene::Ingame)));
        // same scene, new data: same id (no remount), leaving untouched
        let mut b2 = b.clone();
        b2.bo = 5;
        let st3 = scene_transition(&st2, Some(&b2));
        assert_eq!(st3.cur.as_ref().map(|l| (l.id, l.s.bo)), Some((2, 5)));
        assert_eq!(st3.leaving, st2.leaving);
        // unchanged input returns an equal state
        assert_eq!(scene_transition(&st3, Some(&b2)), st3);
        // hide / inactive series -> nothing on screen, the current layer leaves
        let st4 = scene_transition(&st3, Some(&series(Scene::Hide, true)));
        assert!(st4.cur.is_none() && st4.leaving.is_some());
        assert!(shown_scene(Some(&series(Scene::Series, false)), 1).is_none());
        assert!(shown_scene(Some(&series(Scene::Draft, false)), 1).is_some());
    }

    #[test]
    fn tab_box_anchors_on_the_inner_corner() {
        assert_eq!(tab_box(Side::Blue), "top:9.90%;height:4.60%;max-width:26.00%;right:60.80%;");
        assert_eq!(tab_box(Side::Red), "top:9.90%;height:4.60%;max-width:26.00%;left:59.65%;");
    }

    #[test]
    fn cast_json_deserialises_with_extra_fields() {
        let j = r#"{"schema_version":1,"active":true,"blue":{"name":"A","logo":null,"players":[]},"red":{"name":"B"},
            "bo":3,"format":"ban","score_override":null,
            "pool":[{"name":"Sky Temple","status":"picked","by":"red"},{"name":"X","status":"available"}],
            "games":[{"number":1,"map":"Sky Temple","winner":null,"summary":null},{"number":2,"map":"X","winner":"blue","summary":{"duration_secs":600,"winner":"blue","takedowns":[3,1]}}],
            "scene":"series","slide":{"game":null,"index":0,"auto":true,"interval_ms":8000,"since_ms":1}}"#;
        let s: CastSeries = serde_json::from_str(j).unwrap();
        assert_eq!(s.pool[0].by, Some(Side::Red));
        assert_eq!(s.pool[1].by, None);
        assert_eq!(s.games.iter().map(|g| g.summary.is_some()).collect::<Vec<_>>(), [false, true]);
    }
}
