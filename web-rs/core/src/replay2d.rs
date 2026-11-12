//! Replay 2D viewer: wire model of `GET /api/matches/{id}/replay2d` and the pure seek logic. Port of
//! `web/src/replay2d.ts`, `usePlayback.ts` (`advance`) and `clipExport.ts` (`clipFrames`), plus the label helpers
//! of `Replay2D.tsx`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Sample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Interval {
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TalentPick {
    pub t: f64,
    pub tier: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HeroTrack {
    pub player_id: i64,
    pub samples: Vec<Sample>,
    pub life: Vec<Interval>,
    pub casts: Vec<f64>,
    pub talents: Vec<TalentPick>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerMeta {
    pub player_id: i64,
    pub name: Option<String>,
    pub hero: Option<String>,
    pub team: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Death {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Structure {
    pub team: i64,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub destroyed_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FeedEvent {
    pub t: f64,
    pub kind: String,
    pub team: Option<i64>,
    pub victim_player_id: Option<i64>,
    pub killer_player_id: Option<i64>,
    pub structure_kind: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LevelTick {
    pub t: f64,
    pub team: i64,
    pub level: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Objective {
    pub t: f64,
    pub kind: String,
    pub team: Option<i64>,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MinionSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub team: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Meta {
    pub map_name: String,
    pub duration_sec: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Replay2D {
    pub meta: Meta,
    pub players: Vec<PlayerMeta>,
    pub heroes: Vec<HeroTrack>,
    pub deaths: Vec<Death>,
    pub structures: Vec<Structure>,
    pub events: Vec<FeedEvent>,
    pub levels: Vec<LevelTick>,
    pub objectives: Vec<Objective>,
    pub warnings: Vec<String>,
    pub minions: Vec<MinionSample>,
}

/// Calibration of a minimap: image position (fractions) of the two cores, and a vertical stretch around the centre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub blue: [f64; 2],
    pub red: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vscale: Option<f64>,
}

/// Defaults baked in for the maps of the game; operator calibrations saved in the settings override them.
pub fn baked_anchor(slug: &str) -> Option<Anchor> {
    let (blue, red) = match slug {
        "cursed-hollow" => ([0.115, 0.6], [0.905, 0.365]),
        "battlefield-of-eternity" => ([0.05, 0.52], [0.94, 0.44]),
        "infernal-shrines" => ([0.06, 0.44], [0.92, 0.44]),
        "alterac-pass" => ([0.06, 0.63], [0.9, 0.4]),
        "dragon-shire" => ([0.06, 0.42], [0.93, 0.44]),
        "garden-of-terror" => ([0.09, 0.72], [0.92, 0.35]),
        "sky-temple" => ([0.06, 0.38], [0.93, 0.42]),
        "volskaya-foundry" => ([0.06, 0.35], [0.93, 0.38]),
        "hanamura-temple" => ([0.07, 0.45], [0.93, 0.45]),
        "warhead-junction" => ([0.06, 0.32], [0.93, 0.32]),
        "tomb-of-the-spider-queen" => ([0.1, 0.35], [0.9, 0.35]),
        "towers-of-doom" => ([0.08, 0.42], [0.92, 0.48]),
        "braxis-holdout" => ([0.08, 0.72], [0.85, 0.38]),
        _ => return None,
    };
    Some(Anchor {
        blue,
        red,
        vscale: None,
    })
}

fn alive_at(life: &[Interval], t: f64) -> bool {
    life.iter().any(|iv| t >= iv.from && t <= iv.to)
}

/// Most recent end of life at or before `t` (= the moment of the death); `None` before the first spawn.
fn last_alive_end(life: &[Interval], t: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for iv in life {
        if iv.to <= t && best.is_none_or(|b| iv.to > b) {
            best = Some(iv.to);
        }
    }
    best
}

/// Linear interpolation of the position at `t`, clamped to the first and last sample.
fn interp(s: &[Sample], t: f64) -> (f64, f64) {
    let last = s.len() - 1;
    if t <= s[0].t {
        return (s[0].x, s[0].y);
    }
    if t >= s[last].t {
        return (s[last].x, s[last].y);
    }
    let (mut lo, mut hi, mut i) = (0i64, last as i64, 0usize);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        if s[mid as usize].t <= t {
            i = mid as usize;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    let a = &s[i];
    let b = s.get(i + 1).unwrap_or(a);
    let f = if b.t == a.t {
        0.0
    } else {
        (t - a.t) / (b.t - a.t)
    };
    (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f)
}

/// Position and alive state of a hero at `t`. Dead: the position is frozen at the moment of death, never
/// interpolated across the dead → respawn gap (the respawn is at the base, far from the place of death).
pub fn sample_at(h: &HeroTrack, t: f64) -> Option<(f64, f64, bool)> {
    if h.samples.is_empty() {
        return None;
    }
    let alive = alive_at(&h.life, t);
    let effective = if alive {
        t
    } else {
        last_alive_end(&h.life, t).unwrap_or(t)
    };
    let (x, y) = interp(&h.samples, effective);
    Some((x, y, alive))
}

/// Recent deaths at `t` (a marker that stays about 4 s).
pub fn deaths_near(deaths: &[Death], t: f64) -> Vec<&Death> {
    deaths
        .iter()
        .filter(|d| t >= d.t && t <= d.t + 4.0)
        .collect()
}

/// Minion/camp samples within ±5 s of `t` (nearest window, no interpolation).
pub fn minions_near(minions: &[MinionSample], t: f64) -> Vec<&MinionSample> {
    minions.iter().filter(|m| (m.t - t).abs() <= 5.0).collect()
}

/// Intensity (0..1) of the ability-cast flash at `t`: linear decay from the latest cast at or before `t` over 0.6 s.
/// `casts` is sorted ascending.
pub fn cast_flash(casts: &[f64], t: f64) -> f64 {
    const WINDOW: f64 = 0.6;
    let (mut lo, mut hi) = (0i64, casts.len() as i64 - 1);
    let mut best: Option<f64> = None;
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        if casts[mid as usize] <= t {
            best = Some(casts[mid as usize]);
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    let Some(best) = best else { return 0.0 };
    let dt = t - best;
    if dt >= WINDOW {
        0.0
    } else {
        (1.0 - dt / WINDOW).clamp(0.0, 1.0)
    }
}

/// Pure advance: `t + dt * speed` clamped to the duration; stops playing at the end.
pub fn advance(t: f64, dt: f64, speed: f64, duration: f64) -> (f64, bool) {
    let next = t + dt * speed;
    if next >= duration {
        (duration, false)
    } else {
        (next, true)
    }
}

/// Frames of a `[start, end]` interval at `fps`; 0 when empty or invalid.
pub fn clip_frames(start: f64, end: f64, fps: f64) -> i64 {
    if end <= start {
        0
    } else {
        ((end - start) * fps + 0.5).floor() as i64
    }
}

/// Similarity (a, b, c, d, e, f for `setTransform`) sending the two image points `s0`/`s1` onto the two canvas points
/// `d0`/`d1`: rotation, uniform scale and translation (complex-number method).
pub fn solve_similarity(
    s0: (f64, f64),
    s1: (f64, f64),
    d0: (f64, f64),
    d1: (f64, f64),
) -> [f64; 6] {
    let (vsx, vsy) = (s1.0 - s0.0, s1.1 - s0.1);
    let (vdx, vdy) = (d1.0 - d0.0, d1.1 - d0.1);
    let mut den = vsx * vsx + vsy * vsy;
    if den == 0.0 {
        den = 1.0;
    }
    let a = (vdx * vsx + vdy * vsy) / den;
    let b = (vdy * vsx - vdx * vsy) / den;
    let (c, d) = (-b, a);
    let e = d0.0 - (a * s0.0 + c * s0.1);
    let f = d0.1 - (b * s0.0 + d * s0.1);
    [a, b, c, d, e, f]
}

/// `m:ss`.
pub fn fmt_clock(t: f64) -> String {
    format!(
        "{}:{:02}",
        (t / 60.0).floor() as i64,
        (t % 60.0).floor() as i64
    )
}

const TEAM_NAME: [&str; 2] = ["Blue", "Red"];

/// Label of a kill-feed event (with emoji), resolved front-side through the players.
pub fn feed_label(e: &FeedEvent, hero_for: &dyn Fn(Option<i64>) -> Option<String>) -> String {
    match e.kind.as_str() {
        "takedown" => {
            let killer = hero_for(e.killer_player_id).unwrap_or_else(|| "Unknown".into());
            let victim = hero_for(e.victim_player_id).unwrap_or_else(|| "Unknown".into());
            format!("💀 {killer} → {victim}")
        }
        "structure" => {
            let team = match e.team {
                Some(1) => TEAM_NAME[1],
                Some(0) => TEAM_NAME[0],
                _ => "",
            };
            format!(
                "🏰 {team} {}",
                e.structure_kind.as_deref().unwrap_or("structure")
            )
            .trim()
            .to_string()
        }
        "camp" => "🏕️ Camp captured".to_string(),
        other => other.to_string(),
    }
}

/// Text label (no emoji): the event and its `m:ss` timecode.
pub fn feed_aria_label(e: &FeedEvent, hero_for: &dyn Fn(Option<i64>) -> Option<String>) -> String {
    let time = fmt_clock(e.t);
    match e.kind.as_str() {
        "takedown" => {
            let killer = hero_for(e.killer_player_id).unwrap_or_else(|| "unknown hero".into());
            let victim = hero_for(e.victim_player_id).unwrap_or_else(|| "unknown hero".into());
            format!("Takedown: {killer} killed {victim} at {time}")
        }
        "structure" => {
            let team = match e.team {
                Some(1) => TEAM_NAME[1],
                Some(0) => TEAM_NAME[0],
                _ => "Unknown",
            };
            format!(
                "Structure destroyed: {team} {} at {time}",
                e.structure_kind.as_deref().unwrap_or("structure")
            )
        }
        "camp" => format!("Jungle camp captured at {time}"),
        other => format!("{other} at {time}"),
    }
}

/// Displayed label of a map objective. Only the Braxis `zerg_wave` has its own wording; any other kind falls back to a
/// generic one.
pub fn objective_label(o: &Objective) -> String {
    if o.kind == "zerg_wave" {
        return match o.value {
            Some(v) => format!("⚡ Zerg wave ({} units)", fmt_js_number(v)),
            None => "⚡ Zerg wave".to_string(),
        };
    }
    format!("📌 {}", o.kind)
}

pub fn objective_aria_label(o: &Objective) -> String {
    let time = fmt_clock(o.t);
    if o.kind == "zerg_wave" {
        let units = o
            .value
            .map(|v| format!(" ({} units)", fmt_js_number(v)))
            .unwrap_or_default();
        return format!("Zerg wave{units} at {time}");
    }
    format!("{} at {time}", o.kind)
}

/// A JS number printed like `String(n)` for the integers the server sends.
fn fmt_js_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// One row of the kill feed: an event or an objective.
#[derive(Clone, Debug, PartialEq)]
pub enum FeedRow {
    Event(FeedEvent),
    Objective(Objective),
}

impl FeedRow {
    pub fn t(&self) -> f64 {
        match self {
            Self::Event(e) => e.t,
            Self::Objective(o) => o.t,
        }
    }
}

/// Events and objectives merged into one list sorted by `t` (stable: events first on a tie).
pub fn feed_rows(data: &Replay2D) -> Vec<FeedRow> {
    let mut rows: Vec<FeedRow> = data
        .events
        .iter()
        .cloned()
        .map(FeedRow::Event)
        .chain(data.objectives.iter().cloned().map(FeedRow::Objective))
        .collect();
    rows.sort_by(|a, b| {
        a.t()
            .partial_cmp(&b.t())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

/// Index of the last row at or before `t`, `None` before the first.
pub fn highlight_idx(rows: &[FeedRow], t: f64) -> Option<usize> {
    let mut idx = None;
    for (i, r) in rows.iter().enumerate() {
        if r.t() <= t {
            idx = Some(i);
        } else {
            break;
        }
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> HeroTrack {
        HeroTrack {
            samples: vec![
                Sample {
                    t: 0.0,
                    x: 0.0,
                    y: 0.0,
                },
                Sample {
                    t: 10.0,
                    x: 1.0,
                    y: 1.0,
                },
                Sample {
                    t: 20.0,
                    x: 0.0,
                    y: 1.0,
                },
            ],
            life: vec![
                Interval {
                    from: 0.0,
                    to: 12.0,
                },
                Interval {
                    from: 18.0,
                    to: 30.0,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn positions_are_interpolated_and_clamped() {
        let t = track();
        assert_eq!(sample_at(&t, 5.0), Some((0.5, 0.5, true)));
        assert_eq!(sample_at(&t, 100.0).map(|p| (p.0, p.1)), Some((0.0, 1.0)));
        assert_eq!(sample_at(&HeroTrack::default(), 1.0), None);
    }

    #[test]
    fn a_dead_hero_stays_where_it_died() {
        let t = track();
        // dead between 12 and 18: frozen at t = 12 (0.8 of the way from sample 10 to 20 on x)
        let (x, y, alive) = sample_at(&t, 15.0).unwrap();
        assert!(!alive);
        assert!((x - 0.8).abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn markers_and_flashes_follow_their_windows() {
        let d = vec![Death {
            t: 10.0,
            ..Default::default()
        }];
        assert_eq!(deaths_near(&d, 13.9).len(), 1);
        assert!(deaths_near(&d, 14.1).is_empty() && deaths_near(&d, 9.9).is_empty());
        let m = vec![MinionSample {
            t: 10.0,
            ..Default::default()
        }];
        assert_eq!(minions_near(&m, 15.0).len(), 1);
        assert!(minions_near(&m, 15.1).is_empty());
        let casts = [1.0, 5.0];
        assert_eq!(cast_flash(&casts, 0.5), 0.0);
        assert!((cast_flash(&casts, 5.3) - 0.5).abs() < 1e-9);
        assert_eq!(cast_flash(&casts, 6.0), 0.0);
        assert_eq!(cast_flash(&[], 1.0), 0.0);
    }

    #[test]
    fn playback_advances_and_stops_at_the_end() {
        assert_eq!(advance(1.0, 0.5, 2.0, 10.0), (2.0, true));
        assert_eq!(advance(9.5, 1.0, 1.0, 10.0), (10.0, false));
        assert_eq!(clip_frames(2.0, 4.0, 30.0), 60);
        assert_eq!(clip_frames(4.0, 2.0, 30.0), 0);
    }

    #[test]
    fn the_similarity_sends_the_anchors_onto_the_cores() {
        let m = solve_similarity((0.1, 0.5), (0.9, 0.5), (0.2, 0.4), (0.6, 0.4));
        let apply = |x: f64, y: f64| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        let (x0, y0) = apply(0.1, 0.5);
        let (x1, y1) = apply(0.9, 0.5);
        assert!((x0 - 0.2).abs() < 1e-9 && (y0 - 0.4).abs() < 1e-9);
        assert!((x1 - 0.6).abs() < 1e-9 && (y1 - 0.4).abs() < 1e-9);
    }

    #[test]
    fn labels_use_the_heroes_of_the_players() {
        let hero = |id: Option<i64>| id.map(|i| format!("H{i}"));
        let td = FeedEvent {
            t: 65.0,
            kind: "takedown".into(),
            killer_player_id: Some(1),
            victim_player_id: Some(2),
            ..Default::default()
        };
        assert_eq!(feed_label(&td, &hero), "💀 H1 → H2");
        assert_eq!(
            feed_aria_label(&td, &hero),
            "Takedown: H1 killed H2 at 1:05"
        );
        let st = FeedEvent {
            kind: "structure".into(),
            team: Some(1),
            ..Default::default()
        };
        assert_eq!(feed_label(&st, &hero), "🏰 Red structure");
        let none = FeedEvent {
            kind: "structure".into(),
            ..Default::default()
        };
        assert_eq!(feed_label(&none, &hero), "🏰  structure".trim());
        let z = Objective {
            kind: "zerg_wave".into(),
            value: Some(12.0),
            ..Default::default()
        };
        assert_eq!(objective_label(&z), "⚡ Zerg wave (12 units)");
        assert_eq!(
            objective_aria_label(&Objective {
                kind: "x".into(),
                ..Default::default()
            }),
            "x at 0:00"
        );
    }

    #[test]
    fn the_feed_merges_events_and_objectives_in_time_order() {
        let data = Replay2D {
            events: vec![
                FeedEvent {
                    t: 5.0,
                    ..Default::default()
                },
                FeedEvent {
                    t: 20.0,
                    ..Default::default()
                },
            ],
            objectives: vec![
                Objective {
                    t: 5.0,
                    ..Default::default()
                },
                Objective {
                    t: 10.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let rows = feed_rows(&data);
        let ts: Vec<f64> = rows.iter().map(FeedRow::t).collect();
        assert_eq!(ts, vec![5.0, 5.0, 10.0, 20.0]);
        assert!(matches!(rows[0], FeedRow::Event(_)));
        assert_eq!(highlight_idx(&rows, 12.0), Some(2));
        assert_eq!(highlight_idx(&rows, 1.0), None);
    }
}
