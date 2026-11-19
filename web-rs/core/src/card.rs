//! Shareable match card (spec `docs/specs/2026-11-19-match-card.md`), pure part: what the card says (`build_card`)
//! and where everything goes on the 1200 × 675 canvas (`layout`, a display list). The browser part
//! (`app/src/card.rs`) only loads the fonts and images, measures text and replays the list on a canvas.
//!
//! Privacy: player names appear only when the operator turns them on, without their `#tag`; toon handles, chat,
//! battle-tags and replay file names never reach the card.

use serde_json::Value;

use crate::{
    insights::MatchInsights,
    matchdetail::{get, is_num, num, score_rows, text},
    session::{award_label, fmt_dur},
};

/// Card size in CSS pixels (16:9, operator decision 2026-11-19).
pub const W: f64 = 1200.0;
pub const H: f64 = 675.0;
/// Left margin; the right margin mirrors it.
pub const PAD: f64 = 48.0;
/// Width of a team column and the gap between the two.
pub const COL_W: f64 = 528.0;
pub const COL_GAP: f64 = W - 2.0 * PAD - 2.0 * COL_W;
/// Top of the team headers, first row and row pitch.
pub const TEAM_Y: f64 = 196.0;
pub const ROW_Y: f64 = 214.0;
pub const ROW_H: f64 = 66.0;
pub const ROW_GAP: f64 = 8.0;
/// Highlights band and footer baselines.
pub const HIGHLIGHT_Y: f64 = 568.0;
pub const HIGHLIGHT_H: f64 = 40.0;
pub const FOOTER_Y: f64 = 650.0;
/// Portrait radius inside a row.
pub const PORTRAIT_R: f64 = 24.0;

const BLUE: &str = "#85b7eb";
const RED: &str = "#f09595";
const TEXT: &str = "#e8eaf2";
const MUTED: &str = "#9aa0b4";
const KICKER: &str = "#5d6275";
const GOLD: &str = "#fac775";
const ACCENT: &str = "#afa9ec";

/// Fonts of the card; `css` is the canvas `font` shorthand (families from `fonts.css`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Title,
    Banner,
    Team,
    Kicker,
    Hero,
    Name,
    Kda,
    Tag,
    Chip,
    Brand,
    Small,
}

impl Font {
    pub fn css(self) -> &'static str {
        match self {
            Font::Title => "900 64px 'Big Shoulders Display', 'Arial Narrow', sans-serif",
            Font::Banner => "900 40px 'Big Shoulders Display', 'Arial Narrow', sans-serif",
            Font::Team => "800 26px 'Big Shoulders Display', 'Arial Narrow', sans-serif",
            Font::Brand => "800 24px 'Big Shoulders Display', 'Arial Narrow', sans-serif",
            Font::Kicker => "600 17px 'JetBrains Mono', Consolas, monospace",
            Font::Kda => "600 22px 'JetBrains Mono', Consolas, monospace",
            Font::Small => "400 14px 'JetBrains Mono', Consolas, monospace",
            Font::Hero => "600 22px Figtree, 'Segoe UI', sans-serif",
            Font::Name => "500 16px Figtree, 'Segoe UI', sans-serif",
            Font::Tag => "600 15px Figtree, 'Segoe UI', sans-serif",
            Font::Chip => "600 18px Figtree, 'Segoe UI', sans-serif",
        }
    }

    /// Every font the card uses, to load them before drawing.
    pub const ALL: [Font; 11] = [
        Font::Title,
        Font::Banner,
        Font::Team,
        Font::Kicker,
        Font::Hero,
        Font::Name,
        Font::Kda,
        Font::Tag,
        Font::Chip,
        Font::Brand,
        Font::Small,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// One drawing instruction. Coordinates are canvas pixels; text `y` is the alphabetic baseline.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// The minimap, scaled to cover the card, at `alpha`.
    Backdrop { alpha: f64 },
    /// Filled rounded rectangle.
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        fill: &'static str,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        font: Font,
        color: &'static str,
        align: Align,
    },
    /// MVP crown drawn as a shape (no emoji font needed), `w` wide, bottom edge at `y`.
    Crown {
        x: f64,
        y: f64,
        w: f64,
        color: &'static str,
    },
    /// Hero portrait in a ring: the image clipped to a disc, or a disc with the initials when it is missing.
    Portrait {
        hero: String,
        cx: f64,
        cy: f64,
        r: f64,
        ring: String,
    },
}

/// One player line of the card.
#[derive(Clone, Debug, PartialEq)]
pub struct CardRow {
    pub hero: String,
    /// In-game name without `#tag`; `None` unless names are shown.
    pub name: Option<String>,
    pub kills: f64,
    pub deaths: f64,
    pub assists: f64,
    pub mvp: bool,
    /// Short award label (MVP excluded), e.g. `Bulwark`.
    pub award: Option<String>,
    /// The perspective player's row (highlighted).
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardTeam {
    /// 0 = blue, 1 = red.
    pub team: u8,
    pub label: String,
    pub level: u32,
    pub won: bool,
    pub rows: Vec<CardRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Win,
    Loss,
    /// No operator account played: BLUE / RED TEAM WINS.
    Neutral,
}

/// What the card says, independent of where it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub map: String,
    pub mode: &'static str,
    /// `YYYY-MM-DD` (empty when unknown).
    pub day: String,
    pub duration: String,
    pub banner: String,
    pub verdict: Verdict,
    /// Perspective team first, else blue first.
    pub teams: Vec<CardTeam>,
    /// At most 3.
    pub highlights: Vec<String>,
    /// `Heroes of the Storm 2.55.3.91756`, empty without a version.
    pub version: String,
    pub file_name: String,
}

/// Long mode name (`ModeTag`).
pub fn mode_name(mode: Option<i64>) -> &'static str {
    match crate::session::mode_badge(mode).0 {
        "SL" => "Storm League",
        "HL" => "Hero League",
        "TL" => "Team League",
        "UD" => "Unranked",
        "QM" => "Quick Match",
        "ARAM" => "ARAM",
        "CUSTOM" => "Custom",
        c => c,
    }
}

/// In-game name without its `#tag` (and without surrounding spaces).
pub fn display_name(raw: &str) -> String {
    raw.split('#').next().unwrap_or("").trim().to_string()
}

/// `YYYY-MM-DD` of an ISO date-time (its own calendar day, no time zone conversion), empty when malformed.
pub fn iso_day(iso: &str) -> String {
    let day: String = iso.chars().take(10).collect();
    let ok = day.len() == 10
        && day.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    if ok {
        day
    } else {
        String::new()
    }
}

/// `storm-codex-<map-slug>-<YYYY-MM-DD>-<id>.png`; the slug and the day are dropped when empty.
pub fn card_file_name(map: &str, day: &str, id: i64) -> String {
    let plain: String = map
        .to_lowercase()
        .chars()
        .map(crate::reportexport::deaccent)
        .collect();
    let slug = crate::reportexport::slug(&plain);
    let mut parts = vec!["storm-codex".to_string()];
    if !slug.is_empty() {
        parts.push(slug);
    }
    if !day.is_empty() {
        parts.push(day.to_string());
    }
    parts.push(id.to_string());
    format!("{}.png", parts.join("-"))
}

/// `12/3/9`.
pub fn fmt_kda(r: &CardRow) -> String {
    format!("{}/{}/{}", r.kills, r.deaths, r.assists)
}

/// `Heroes of the Storm 2.55.3.91756` from `match.version` (`m_major`…`m_build`), empty when absent.
pub fn version_label(v: &Value) -> String {
    let parts: Vec<String> = ["m_major", "m_minor", "m_revision", "m_build"]
        .iter()
        .map_while(|k| get(v, k).as_i64().map(|n| n.to_string()))
        .collect();
    if parts.len() == 4 {
        format!("Heroes of the Storm {}", parts.join("."))
    } else {
        String::new()
    }
}

/// End level of a team: highest key of `levelTimes[team]`.
fn end_level(m: &Value, team: u8) -> u32 {
    get(get(m, "levelTimes"), &team.to_string())
        .as_object()
        .map(|o| {
            o.keys()
                .filter_map(|k| k.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// Time a team reached `level`, from `levelTimes[team][level].time`.
fn level_time(m: &Value, team: u8, level: u32) -> Option<f64> {
    get(
        get(get(m, "levelTimes"), &team.to_string()),
        &level.to_string(),
    )
    .get("time")
    .and_then(Value::as_f64)
}

/// Team that reached level 10 first, if any did.
pub fn first_to_ten(m: &Value) -> Option<u8> {
    match (level_time(m, 0, 10), level_time(m, 1, 10)) {
        (Some(a), Some(b)) if a <= b => Some(0),
        (Some(_), Some(_)) => Some(1),
        (Some(_), None) => Some(0),
        (None, Some(_)) => Some(1),
        (None, None) => None,
    }
}

/// `Your team` / `Enemy team` from the perspective team, `Blue team` / `Red team` without one.
fn team_label(team: u8, my_team: Option<u8>) -> &'static str {
    match my_team {
        Some(t) if t == team => "Your team",
        Some(_) => "Enemy team",
        None if team == 0 => "Blue team",
        None => "Red team",
    }
}

/// Inputs of `build_card` beyond the match JSON.
pub struct CardInput<'a> {
    /// Body of `GET /api/matches/{id}` (`id`, `match`, `players`).
    pub detail: &'a Value,
    /// Whole-match insights (`GET /api/matches/{id}/insights`), for the team-fight highlight.
    pub insights: Option<&'a MatchInsights>,
    /// Perspective player (operator account in this game), used only to highlight a row and pick the side.
    pub me_toon: Option<&'a str>,
    pub show_names: bool,
    /// Local calendar day of the match when the browser could compute it; otherwise the ISO date is used.
    pub local_day: Option<String>,
}

/// What the card says. Heroes and names come from the match detail; nothing identifying beyond the optional
/// in-game names is read.
pub fn build_card(input: &CardInput) -> Card {
    let d = input.detail;
    let m = get(d, "match");
    let id = num(get(d, "id")) as i64;
    let players: Vec<&Value> = get(d, "players")
        .as_object()
        .map(|o| o.values().collect())
        .unwrap_or_default();
    let my_team: Option<u8> = input.me_toon.and_then(|me| {
        players
            .iter()
            .find(|p| text(get(p, "ToonHandle")) == Some(me))
            .and_then(|p| get(p, "team").as_f64())
            .filter(|t| *t == 0.0 || *t == 1.0)
            .map(|t| t as u8)
    });
    let winner: Option<u8> = if is_num(get(m, "winner"), 0.0) {
        Some(0)
    } else if is_num(get(m, "winner"), 1.0) {
        Some(1)
    } else {
        None
    };
    let order: [u8; 2] = match my_team {
        Some(t) => [t, 1 - t],
        None => [0, 1],
    };
    let teams: Vec<CardTeam> = order
        .iter()
        .map(|&t| CardTeam {
            team: t,
            label: team_label(t, my_team).to_string(),
            level: end_level(m, t),
            won: winner == Some(t),
            rows: score_rows(&players, t as i64)
                .into_iter()
                .map(|r| {
                    let aw = award_label(r.award.as_deref());
                    CardRow {
                        me: input.me_toon.is_some_and(|me| me == r.toon),
                        name: input
                            .show_names
                            .then(|| display_name(&r.name))
                            .filter(|n| !n.is_empty()),
                        hero: r.hero,
                        kills: r.kills,
                        deaths: r.deaths,
                        assists: r.assists,
                        mvp: aw.as_ref().is_some_and(|a| a.mvp),
                        award: aw.filter(|a| !a.mvp).map(|a| a.label),
                    }
                })
                .collect(),
        })
        .collect();
    let (banner, verdict) = match (my_team, winner) {
        (Some(t), Some(w)) if t == w => ("VICTORY".to_string(), Verdict::Win),
        (Some(_), Some(_)) => ("DEFEAT".to_string(), Verdict::Loss),
        (None, Some(w)) => (
            format!("{} TEAM WINS", if w == 1 { "RED" } else { "BLUE" }),
            Verdict::Neutral,
        ),
        (_, None) => ("Result unavailable".to_string(), Verdict::Neutral),
    };
    let side = |t: u8| team_label(t, my_team);
    let mut highlights = Vec::new();
    if let Some(r) = teams.iter().flat_map(|t| &t.rows).find(|r| r.mvp) {
        highlights.push(format!("MVP · {}", r.hero));
    }
    if let Some(ins) = input.insights.filter(|i| !i.fights.is_empty()) {
        let total = ins.fights.len();
        let won = |t: u8| ins.fights.iter().filter(|f| f.winner == Some(t)).count();
        highlights.push(match my_team {
            Some(t) => format!("Team fights won {} of {total}", won(t)),
            None => format!("Blue won {} of {total} team fights", won(0)),
        });
    }
    if let Some(t) = first_to_ten(m) {
        highlights.push(format!("{} first to level 10", side(t)));
    } else if let Some(t) = get(m, "firstFort")
        .as_f64()
        .filter(|t| *t == 0.0 || *t == 1.0)
    {
        highlights.push(format!("First fort: {}", side(t as u8).to_lowercase()));
    }
    highlights.truncate(3);
    let map = text(get(m, "map")).unwrap_or("").to_string();
    let day = input
        .local_day
        .clone()
        .filter(|d| !iso_day(d).is_empty())
        .unwrap_or_else(|| iso_day(text(get(m, "date")).unwrap_or("")));
    let length = num(get(m, "length"));
    Card {
        file_name: card_file_name(&map, &day, id),
        mode: mode_name(get(m, "mode").as_i64()),
        duration: fmt_dur(Some(length)),
        version: version_label(get(m, "version")),
        map,
        day,
        banner,
        verdict,
        teams,
        highlights,
    }
}

/// Longest prefix of `s` that fits in `max` once `…` is appended; `s` itself when it fits.
pub fn fit_text(s: &str, max: f64, font: Font, measure: &dyn Fn(&str, Font) -> f64) -> String {
    if measure(s, font) <= max {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut n = chars.len();
    while n > 0 {
        n -= 1;
        let cut: String = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
        if measure(&cut, font) <= max {
            return cut;
        }
    }
    String::new()
}

/// Left edge of team column `i` (0 = left).
pub fn column_x(i: usize) -> f64 {
    PAD + i as f64 * (COL_W + COL_GAP)
}

/// Top of row `i` in a column.
pub fn row_y(i: usize) -> f64 {
    ROW_Y + i as f64 * ROW_H
}

fn team_color(team: u8) -> &'static str {
    if team == 0 {
        BLUE
    } else {
        RED
    }
}

fn text_op(x: f64, y: f64, text: String, font: Font, color: &'static str, align: Align) -> Op {
    Op::Text {
        x,
        y,
        text,
        font,
        color,
        align,
    }
}

/// The display list of a card. `measure` returns the width of a text in a font (canvas `measureText`);
/// `ring` the universe colour of a hero (reference data).
pub fn layout(
    card: &Card,
    measure: &dyn Fn(&str, Font) -> f64,
    ring: &dyn Fn(&str) -> String,
) -> Vec<Op> {
    let mut ops = vec![
        Op::Rect {
            x: 0.0,
            y: 0.0,
            w: W,
            h: H,
            r: 0.0,
            fill: "#06070b",
        },
        Op::Backdrop { alpha: 0.28 },
        Op::Rect {
            x: 0.0,
            y: 0.0,
            w: W,
            h: H,
            r: 0.0,
            fill: "rgba(6,7,11,0.55)",
        },
    ];
    // header: kicker, map, verdict banner
    let kicker = [
        card.mode.to_uppercase(),
        card.day.clone(),
        card.duration.clone(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("  ·  ");
    ops.push(text_op(PAD, 64.0, kicker, Font::Kicker, MUTED, Align::Left));
    let (banner_fg, banner_bg) = match card.verdict {
        Verdict::Win => ("#5dcaa5", "rgba(93,202,165,0.16)"),
        Verdict::Loss => ("#f7c1c1", "rgba(226,75,74,0.20)"),
        Verdict::Neutral => ("#cfd3e0", "rgba(18,20,29,0.85)"),
    };
    let banner_w = measure(&card.banner, Font::Banner) + 40.0;
    let banner_x = W - PAD - banner_w;
    ops.push(Op::Rect {
        x: banner_x,
        y: 74.0,
        w: banner_w,
        h: 58.0,
        r: 10.0,
        fill: banner_bg,
    });
    ops.push(text_op(
        banner_x + banner_w / 2.0,
        118.0,
        card.banner.clone(),
        Font::Banner,
        banner_fg,
        Align::Center,
    ));
    let title_max = banner_x - PAD - 24.0;
    ops.push(text_op(
        PAD,
        128.0,
        fit_text(&card.map, title_max, Font::Title, measure),
        Font::Title,
        TEXT,
        Align::Left,
    ));
    // team columns
    for (i, team) in card.teams.iter().take(2).enumerate() {
        let x = column_x(i);
        let color = team_color(team.team);
        ops.push(text_op(
            x,
            TEAM_Y,
            format!(
                "{}{}",
                team.label.to_uppercase(),
                if team.won { " · WIN" } else { "" }
            ),
            Font::Team,
            color,
            Align::Left,
        ));
        if team.level > 0 {
            ops.push(text_op(
                x + COL_W,
                TEAM_Y,
                format!("LEVEL {}", team.level),
                Font::Kicker,
                MUTED,
                Align::Right,
            ));
        }
        for (j, r) in team.rows.iter().take(5).enumerate() {
            ops.extend(row_ops(r, x, row_y(j), team.team, measure, ring));
        }
    }
    // highlights
    let mut hx = PAD;
    for h in card.highlights.iter().take(3) {
        let room = W - PAD - hx - 28.0;
        if room < 60.0 {
            break;
        }
        let label = fit_text(h, room, Font::Chip, measure);
        let w = measure(&label, Font::Chip) + 28.0;
        let gold = h.starts_with("MVP");
        ops.push(Op::Rect {
            x: hx,
            y: HIGHLIGHT_Y,
            w,
            h: HIGHLIGHT_H,
            r: 20.0,
            fill: if gold {
                "rgba(250,199,117,0.16)"
            } else {
                "rgba(127,119,221,0.18)"
            },
        });
        ops.push(text_op(
            hx + 14.0,
            HIGHLIGHT_Y + 27.0,
            label,
            Font::Chip,
            if gold { GOLD } else { TEXT },
            Align::Left,
        ));
        hx += w + 12.0;
    }
    // footer
    ops.push(text_op(
        PAD,
        FOOTER_Y,
        "STORM CODEX".into(),
        Font::Brand,
        ACCENT,
        Align::Left,
    ));
    if !card.version.is_empty() {
        ops.push(text_op(
            W - PAD,
            FOOTER_Y,
            card.version.clone(),
            Font::Small,
            KICKER,
            Align::Right,
        ));
    }
    ops
}

fn row_ops(
    r: &CardRow,
    x: f64,
    y: f64,
    team: u8,
    measure: &dyn Fn(&str, Font) -> f64,
    ring: &dyn Fn(&str) -> String,
) -> Vec<Op> {
    let h = ROW_H - ROW_GAP;
    let mut ops = vec![Op::Rect {
        x,
        y,
        w: COL_W,
        h,
        r: 10.0,
        fill: if r.me {
            "rgba(127,119,221,0.30)"
        } else {
            "rgba(14,16,22,0.78)"
        },
    }];
    if r.me {
        ops.push(Op::Rect {
            x,
            y,
            w: 5.0,
            h,
            r: 2.0,
            fill: team_color(team),
        });
    }
    let cy = y + h / 2.0;
    ops.push(Op::Portrait {
        hero: r.hero.clone(),
        cx: x + 16.0 + PORTRAIT_R,
        cy,
        r: PORTRAIT_R,
        ring: ring(&r.hero),
    });
    let right = x + COL_W - 16.0;
    let kda = fmt_kda(r);
    let kda_w = measure(&kda, Font::Kda);
    ops.push(text_op(right, cy + 8.0, kda, Font::Kda, TEXT, Align::Right));
    let mut text_right = right - kda_w - 16.0;
    let tag = if r.mvp {
        Some(("MVP".to_string(), GOLD))
    } else {
        r.award.clone().map(|a| (a, MUTED))
    };
    if let Some((tag, color)) = tag {
        let tag = fit_text(&tag, 120.0, Font::Tag, measure);
        let tw = measure(&tag, Font::Tag);
        ops.push(text_op(
            text_right,
            cy + 6.0,
            tag,
            Font::Tag,
            color,
            Align::Right,
        ));
        text_right -= tw + 14.0;
        if r.mvp {
            ops.push(Op::Crown {
                x: text_right - 6.0,
                y: cy + 5.0,
                w: 18.0,
                color: GOLD,
            });
            text_right -= 18.0 + 8.0;
        }
    }
    let tx = x + 16.0 + 2.0 * PORTRAIT_R + 16.0;
    let max = (text_right - tx).max(0.0);
    let hero = fit_text(&r.hero, max, Font::Hero, measure);
    match &r.name {
        Some(name) => {
            ops.push(text_op(tx, cy - 3.0, hero, Font::Hero, TEXT, Align::Left));
            ops.push(text_op(
                tx,
                cy + 18.0,
                fit_text(name, max, Font::Name, measure),
                Font::Name,
                MUTED,
                Align::Left,
            ));
        }
        None => ops.push(text_op(tx, cy + 8.0, hero, Font::Hero, TEXT, Align::Left)),
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::Fight;
    use serde_json::json;

    /// Monospace stand-in for `measureText`: 10 px per character.
    fn mono(s: &str, _: Font) -> f64 {
        s.chars().count() as f64 * 10.0
    }

    fn ring(_: &str) -> String {
        "#afa9ec".into()
    }

    fn player(toon: &str, name: &str, hero: &str, team: i64, k: i64, award: Option<&str>) -> Value {
        json!({
            "ToonHandle": toon, "name": name, "hero": hero, "team": team,
            "gameStats": {"SoloKill": k, "Deaths": 2, "Assists": 7,
                          "awards": award.map(|a| vec![a]).unwrap_or_default()}
        })
    }

    fn detail() -> Value {
        let mut players = serde_json::Map::new();
        let heroes = ["Valla", "Muradin", "Uther", "Jaina", "Sonya"];
        for (i, h) in heroes.iter().enumerate() {
            let toon = format!("2-Hero-1-{}", 100 + i);
            let award = (i == 0).then_some("EndOfMatchAwardMVPBoolean");
            players.insert(
                toon.clone(),
                player(&toon, &format!("Blue{i}#1234"), h, 0, 5 - i as i64, award),
            );
        }
        let reds = ["Zeratul", "Johanna", "Rehgar", "Li-Ming", "Thrall"];
        for (i, h) in reds.iter().enumerate() {
            let toon = format!("2-Hero-1-{}", 200 + i);
            let award = (i == 1).then_some("EndOfMatchAwardMostProtectionBoolean");
            players.insert(
                toon.clone(),
                player(&toon, &format!("Red{i}"), h, 1, i as i64, award),
            );
        }
        json!({
            "id": 42,
            "match": {
                "map": "Braxis Holdout", "mode": 50091, "date": "2026-11-12T21:34:00Z", "length": 1234,
                "winner": 0, "firstFort": 1,
                "version": {"m_major": 2, "m_minor": 55, "m_revision": 3, "m_build": 91756},
                "levelTimes": {
                    "0": {"1": {"level": 1, "time": 0}, "10": {"level": 10, "time": 600}, "20": {"level": 20, "time": 1200}},
                    "1": {"1": {"level": 1, "time": 0}, "10": {"level": 10, "time": 640}, "18": {"level": 18, "time": 1100}}
                }
            },
            "players": players
        })
    }

    fn fight(winner: Option<u8>) -> Fight {
        serde_json::from_value(json!({
            "start": 0.0, "end": 1.0, "lost": [0, 0], "winner": winner, "deaths": [],
            "structures_after": [0, 0], "camps_after": [0, 0], "objectives_after": [0, 0]
        }))
        .unwrap()
    }

    fn insights() -> MatchInsights {
        MatchInsights {
            fights: vec![fight(Some(0)), fight(Some(0)), fight(Some(1)), fight(None)],
            camps: vec![],
            isolated_deaths: vec![],
            window: None,
            teamfight_totals: vec![],
        }
    }

    fn input(d: &Value, me: Option<&'static str>, names: bool) -> Card {
        let ins = insights();
        build_card(&CardInput {
            detail: d,
            insights: Some(&ins),
            me_toon: me,
            show_names: names,
            local_day: None,
        })
    }

    fn texts(ops: &[Op]) -> Vec<String> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn win_from_the_perspective_team_puts_it_first() {
        let d = detail();
        let c = input(&d, Some("2-Hero-1-102"), false);
        assert_eq!(c.verdict, Verdict::Win);
        assert_eq!(c.banner, "VICTORY");
        assert_eq!(c.teams[0].team, 0);
        assert_eq!(c.teams[0].label, "Your team");
        assert_eq!(c.teams[0].level, 20);
        assert_eq!(c.teams[1].level, 18);
        assert!(c.teams[0].rows.iter().any(|r| r.me && r.hero == "Uther"));
        assert_eq!(c.teams[0].rows.iter().filter(|r| r.me).count(), 1);
        assert_eq!(
            c.highlights,
            vec![
                "MVP · Valla",
                "Team fights won 2 of 4",
                "Your team first to level 10"
            ]
        );
        assert_eq!(c.mode, "Storm League");
        assert_eq!(c.duration, "20:34");
        assert_eq!(c.version, "Heroes of the Storm 2.55.3.91756");
    }

    #[test]
    fn loss_puts_the_red_perspective_team_first() {
        let d = detail();
        let c = input(&d, Some("2-Hero-1-201"), false);
        assert_eq!((c.verdict, c.banner.as_str()), (Verdict::Loss, "DEFEAT"));
        assert_eq!(c.teams[0].team, 1);
        assert_eq!(c.teams[1].label, "Enemy team");
        assert_eq!(c.highlights[1], "Team fights won 1 of 4");
        assert_eq!(c.highlights[2], "Enemy team first to level 10");
        let johanna = c.teams[0]
            .rows
            .iter()
            .find(|r| r.hero == "Johanna")
            .unwrap();
        assert_eq!(johanna.award.as_deref(), Some("Protection"));
        assert!(!johanna.mvp);
    }

    #[test]
    fn without_operator_the_card_is_neutral_blue_first() {
        let d = detail();
        let c = input(&d, None, false);
        assert_eq!(
            (c.verdict, c.banner.as_str()),
            (Verdict::Neutral, "BLUE TEAM WINS")
        );
        assert_eq!(c.teams[0].label, "Blue team");
        assert!(c.teams.iter().flat_map(|t| &t.rows).all(|r| !r.me));
        assert_eq!(c.highlights[1], "Blue won 2 of 4 team fights");
    }

    #[test]
    fn unavailable_winner_does_not_claim_either_team_won() {
        let mut d = detail();
        for unknown in [Value::Null, json!(2)] {
            d["match"]["winner"] = unknown;
            for me in [None, Some("2-Hero-1-201")] {
                let c = input(&d, me, false);
                assert_eq!(
                    (c.verdict, c.banner.as_str()),
                    (Verdict::Neutral, "Result unavailable")
                );
                assert!(c.teams.iter().all(|t| !t.won));
            }
        }
    }

    #[test]
    fn first_fort_replaces_level_ten_when_nobody_reached_it() {
        let mut d = detail();
        d["match"]["levelTimes"] = json!({"0": {"9": {"level": 9, "time": 500}}, "1": {}});
        let c = input(&d, None, false);
        assert_eq!(c.highlights.last().unwrap(), "First fort: red team");
        assert_eq!(c.teams[0].level, 9);
        assert_eq!(c.teams[1].level, 0);
    }

    #[test]
    fn names_are_hidden_by_default_and_never_carry_a_tag_or_toon() {
        let d = detail();
        let measure: &dyn Fn(&str, Font) -> f64 = &mono;
        let hidden = layout(&input(&d, Some("2-Hero-1-100"), false), measure, &ring);
        let all = texts(&hidden).join("\n");
        assert!(!all.contains("Blue0") && !all.contains("Red1"), "{all}");
        assert!(!all.contains("2-Hero-1"), "{all}");
        let shown = layout(&input(&d, Some("2-Hero-1-100"), true), measure, &ring);
        let all = texts(&shown);
        assert!(all.contains(&"Blue0".to_string()));
        assert!(all.contains(&"Red1".to_string()));
        assert!(all
            .iter()
            .all(|t| !t.contains('#') && !t.contains("2-Hero-1")));
    }

    #[test]
    fn display_name_strips_the_tag() {
        assert_eq!(display_name("Matella#2211"), "Matella");
        assert_eq!(display_name(" Plain "), "Plain");
        assert_eq!(display_name("#123"), "");
    }

    #[test]
    fn file_name_is_slugged_and_dated() {
        assert_eq!(
            card_file_name("Garden of Terror", "2026-11-12", 7),
            "storm-codex-garden-of-terror-2026-11-12-7.png"
        );
        assert_eq!(
            card_file_name("Hanamura Temple", "", 9),
            "storm-codex-hanamura-temple-9.png"
        );
        assert_eq!(
            card_file_name("Tomb of the Spider Queen", "2026-01-02", 3),
            "storm-codex-tomb-of-the-spider-queen-2026-01-02-3.png"
        );
        assert_eq!(
            card_file_name("", "2026-01-02", 3),
            "storm-codex-2026-01-02-3.png"
        );
        let d = detail();
        assert_eq!(
            input(&d, None, false).file_name,
            "storm-codex-braxis-holdout-2026-11-12-42.png"
        );
    }

    #[test]
    fn iso_day_and_local_day() {
        assert_eq!(iso_day("2026-11-12T21:34:00Z"), "2026-11-12");
        assert_eq!(iso_day("12/11/2026"), "");
        assert_eq!(iso_day(""), "");
        let d = detail();
        let c = build_card(&CardInput {
            detail: &d,
            insights: None,
            me_toon: None,
            show_names: false,
            local_day: Some("2026-11-13".into()),
        });
        assert_eq!(c.day, "2026-11-13");
        assert_eq!(c.file_name, "storm-codex-braxis-holdout-2026-11-13-42.png");
        // no insights: no fight highlight
        assert_eq!(
            c.highlights,
            vec!["MVP · Valla", "Blue team first to level 10"]
        );
    }

    #[test]
    fn fit_text_truncates_with_an_ellipsis() {
        assert_eq!(fit_text("Valla", 50.0, Font::Hero, &mono), "Valla");
        assert_eq!(
            fit_text("The Lost Vikings", 80.0, Font::Hero, &mono),
            "The Los…"
        );
        assert_eq!(fit_text("Cho Gall", 50.0, Font::Hero, &mono), "Cho…");
        assert_eq!(fit_text("Abc", 0.0, Font::Hero, &mono), "");
    }

    #[test]
    fn columns_and_rows_stay_on_the_canvas_without_overlap() {
        assert_eq!(column_x(0), PAD);
        assert!((column_x(1) + COL_W - (W - PAD)).abs() < 1e-9);
        const { assert!(COL_GAP > 0.0) };
        assert!(row_y(0) > TEAM_Y);
        assert!(row_y(4) + ROW_H - ROW_GAP < HIGHLIGHT_Y);
        const { assert!(HIGHLIGHT_Y + HIGHLIGHT_H < FOOTER_Y - 20.0) };
        const { assert!(FOOTER_Y < H) };
    }

    #[test]
    fn every_drawn_element_fits_inside_the_card() {
        let d = detail();
        let measure: &dyn Fn(&str, Font) -> f64 = &mono;
        for names in [false, true] {
            for op in layout(&input(&d, Some("2-Hero-1-100"), names), measure, &ring) {
                match op {
                    Op::Rect { x, y, w, h, .. } => {
                        assert!(x >= 0.0 && y >= 0.0 && x + w <= W && y + h <= H)
                    }
                    Op::Text {
                        x,
                        y,
                        text,
                        align,
                        font,
                        ..
                    } => {
                        let w = mono(&text, font);
                        let left = match align {
                            Align::Left => x,
                            Align::Right => x - w,
                            Align::Center => x - w / 2.0,
                        };
                        assert!(left >= 0.0 && left + w <= W && y <= H, "{text} at {x}");
                    }
                    Op::Portrait { cx, cy, r, .. } => {
                        assert!(cx - r >= 0.0 && cy + r <= H)
                    }
                    Op::Crown { x, y, w, .. } => {
                        assert!(x >= 0.0 && x + w <= W && y <= H)
                    }
                    Op::Backdrop { .. } => {}
                }
            }
        }
    }

    #[test]
    fn the_mvp_row_gets_a_crown_and_no_emoji() {
        let d = detail();
        let measure: &dyn Fn(&str, Font) -> f64 = &mono;
        let ops = layout(&input(&d, None, false), measure, &ring);
        let crowns: Vec<f64> = ops
            .iter()
            .filter_map(|o| match o {
                Op::Crown { x, .. } => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(crowns.len(), 1);
        let valla_end = ops
            .iter()
            .find_map(|o| match o {
                Op::Text {
                    text,
                    x,
                    font: Font::Hero,
                    ..
                } if text == "Valla" => Some(x + mono(text, Font::Hero)),
                _ => None,
            })
            .unwrap();
        assert!(valla_end < crowns[0]);
        assert!(texts(&ops)
            .iter()
            .all(|t| t.chars().all(|c| c.is_ascii() || c == '·' || c == '…')));
    }

    #[test]
    fn long_hero_text_never_runs_into_the_kda() {
        let mut d = detail();
        d["players"]["2-Hero-1-100"]["hero"] =
            json!("An Extremely Long Hero Name For Testing Purposes");
        d["players"]["2-Hero-1-100"]["name"] = json!("AVeryVeryLongPlayerNameIndeedYesYes#1");
        let measure: &dyn Fn(&str, Font) -> f64 = &mono;
        let ops = layout(&input(&d, None, true), measure, &ring);
        let kda_left = ops
            .iter()
            .find_map(|o| match o {
                Op::Text {
                    text,
                    x,
                    font: Font::Kda,
                    ..
                } if text == "5/2/7" => Some(x - mono(text, Font::Kda)),
                _ => None,
            })
            .unwrap();
        for o in &ops {
            if let Op::Text {
                text,
                x,
                font: Font::Hero | Font::Name,
                ..
            } = o
            {
                if text.starts_with("An Ext") || text.starts_with("AVery") {
                    assert!(text.ends_with('…'));
                    assert!(x + mono(text, Font::Hero) < kda_left, "{text}");
                }
            }
        }
    }
}
