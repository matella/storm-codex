//! Match list helpers of the session OBS sources (`/ticker`, `/widget`, `/queue`): wire types, mode badge,
//! awards, operator lookup, streak and the Jarvis line. Port of the helpers of `web/src/api.ts` and of the
//! computations of `Ticker.tsx`, `Widget.tsx` and `Queue.tsx`. No I/O: the operator names and the local day of
//! each game come from the caller.

use serde::Deserialize;

/// One player of `GET /api/matches` (`MatchPlayer` in api.ts, only what the sources read).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MatchPlayer {
    pub name: Option<String>,
    pub hero: Option<String>,
    pub team: Option<i64>,
    pub win: Option<bool>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    pub takedowns: Option<u32>,
    pub award: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MatchSummary {
    pub id: i64,
    pub map: Option<String>,
    pub mode: Option<i64>,
    pub played_at: Option<String>,
    /// Seconds.
    pub length: Option<f64>,
    pub winner: Option<i64>,
    pub players: Option<Vec<MatchPlayer>>,
}

/// `GET /api/settings`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub operator_names: Vec<String>,
}

/// `modeBadge`: short label and CSS class of a game mode (official storm-stats codes).
pub fn mode_badge(mode: Option<i64>) -> (&'static str, &'static str) {
    match mode {
        Some(-1) => ("CUSTOM", "b-qm"),
        Some(50001) => ("QM", "b-qm"),
        Some(50021) => ("VS IA", "b-qm"),
        Some(50031) => ("BRAWL", "b-qm"),
        Some(50041) => ("ENTR.", "b-qm"),
        Some(50051) => ("UD", "b-sl"),
        Some(50061) => ("HL", "b-sl"),
        Some(50071) => ("TL", "b-sl"),
        Some(50091) => ("SL", "b-sl"),
        Some(50101) => ("ARAM", "b-qm"),
        _ => ("—", "b-qm"),
    }
}

/// `fmtDur`: `m:ss`, a dash for a missing or zero duration.
pub fn fmt_dur(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) if s != 0.0 && !s.is_nan() => {
            format!(
                "{}:{:02}",
                (s / 60.0).floor() as i64,
                (s % 60.0).floor() as i64
            )
        }
        _ => "—".into(),
    }
}

/// `mapSlug`: lower case, apostrophes dropped, whitespace runs -> `-`.
pub fn map_slug(map: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in map
        .to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
    {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push('-');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push('-');
    }
    out
}

/// `mapImage`: painted loading-screen art, `None` when there is no map.
pub fn map_image(map: Option<&str>) -> Option<String> {
    map.filter(|m| !m.is_empty())
        .map(|m| format!("/images/battlegrounds/{}.png", map_slug(m)))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Award {
    pub label: String,
    pub icon: &'static str,
    pub mvp: bool,
}

fn award_icon(core: &str) -> &'static str {
    match core {
        "MVP" => "👑",
        "MostHeroDamageDone" => "🗡️",
        "MostTeamfightHeroDamageDone" => "⚔️",
        "MostSiegeDamageDone" => "🏰",
        "MostHealing" => "💚",
        "MostTeamfightHealingDone" => "💗",
        "ClutchHealer" => "🚑",
        "MostDamageTaken" => "🛡️",
        "MostTeamfightDamageTaken" => "🧱",
        "MostMercCampsCaptured" => "🏕️",
        "HighestKillStreak" => "🔥",
        "HatTrick" => "🎩",
        "MostKills" => "💀",
        "MostStuns" => "💫",
        "MostSilences" => "🤐",
        "MostRoots" => "🌿",
        "MostXPContribution" => "📈",
        "MostDamageDoneToZerg" => "🐛",
        "MostAltarDamageDone" => "⛩️",
        "MostImmortalDamage" => "👹",
        "MostCurseDamageDone" => "☠️",
        "MostGemsTurnedIn" => "💎",
        "MostTimeOnPoint" => "📍",
        "MostDamageToMinions" => "🧟",
        "MostEscapes" => "🏃",
        "MostDaredevilEscapes" => "🪂",
        "MostDragonShrinesCaptured" => "🐉",
        "MostTimeInTemple" => "🛕",
        "MostVengeancesPerformed" => "😤",
        "MostProtection" => "🤝",
        "0OutnumberedDeaths" => "🧊",
        "0Deaths" => "😇",
        "MostInterruptedCageUnlocks" => "🔓",
        "MostCoinsPaid" => "🪙",
        "MostNukeDamageDone" => "☢️",
        "MostSeedsCollected" => "🌱",
        _ => "🏅",
    }
}

/// `awardLabel`: short label, emoji and MVP flag of an end-of-match award (`None` without award).
pub fn award_label(raw: Option<&str>) -> Option<Award> {
    let raw = raw.filter(|r| !r.is_empty())?;
    let core = raw.strip_prefix("EndOfMatchAward").unwrap_or(raw);
    let core = core.strip_suffix("Boolean").unwrap_or(core);
    let icon = award_icon(core);
    if core == "MVP" {
        return Some(Award {
            label: "MVP".into(),
            icon,
            mvp: true,
        });
    }
    // split camel case, then shorten the verbose labels
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in core.chars() {
        if prev.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        spaced.push(c);
        prev = Some(c);
    }
    let label = spaced.strip_prefix("Most ").unwrap_or(&spaced);
    let label = label.strip_suffix(" Done").unwrap_or(label);
    Some(Award {
        label: label.into(),
        icon,
        mvp: false,
    })
}

fn same_name(p: &MatchPlayer, wanted: &str) -> bool {
    p.name.as_deref().unwrap_or("").to_lowercase() == wanted
}

/// `pickOperator`: the `?me=` override first, then any configured operator name, then the first player.
pub fn pick_operator<'a>(
    players: &'a [MatchPlayer],
    over: Option<&str>,
    names: &[String],
) -> Option<&'a MatchPlayer> {
    let candidates = over
        .into_iter()
        .chain(names.iter().map(String::as_str))
        .filter(|n| !n.is_empty())
        .map(str::to_lowercase);
    for c in candidates {
        if let Some(p) = players.iter().find(|p| same_name(p, &c)) {
            return Some(p);
        }
    }
    players.first()
}

/// `matchOperator`: strict, the operator only when a configured name matches (no fallback).
pub fn match_operator<'a>(players: &'a [MatchPlayer], names: &[String]) -> Option<&'a MatchPlayer> {
    let names: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    players.iter().find(|p| {
        let n = p.name.as_deref().unwrap_or("").to_lowercase();
        names.contains(&n)
    })
}

/// `jarvisPhrase`: deterministic French butler line (variant picked from `takedowns + deaths`).
pub fn jarvis_phrase(
    won: bool,
    hero: Option<&str>,
    deaths: u32,
    takedowns: u32,
    streak: i32,
) -> String {
    let seed = (takedowns + deaths) as usize;
    let pick = |v: Vec<String>| v[seed % v.len()].clone();
    let h = hero.unwrap_or("ce héros");
    if won {
        if streak >= 3 {
            return pick(vec![
                format!("Et de {streak}, monsieur. La soirée vous appartient."),
                format!("Série de {streak}. Difficile de faire mieux."),
                format!("{streak} d'affilée — je note la domination."),
            ]);
        }
        if deaths == 0 {
            return pick(vec![
                "Aucune mort. Une partie de maître, monsieur.".into(),
                "Zéro mort — proprement exécuté.".into(),
            ]);
        }
        if takedowns >= 15 {
            return pick(vec![
                format!("Démonstration. {h} était intenable."),
                format!("{takedowns} participations — la carte vous a appartenu."),
            ]);
        }
        return pick(vec![
            format!("Victoire nette. {h} a fait le travail."),
            "Bien joué, monsieur. On enchaîne ?".into(),
            "Une de plus au compteur.".into(),
        ]);
    }
    if streak <= -3 {
        return pick(vec![
            "Série difficile, monsieur. Gardons la tête froide.".into(),
            format!("{} revers d'affilée — une pause, peut-être ?", streak.abs()),
        ]);
    }
    if deaths >= 8 {
        return pick(vec![
            "Trop de morts, monsieur. On respire au prochain.".into(),
            format!("{deaths} morts — la prudence paiera la prochaine fois."),
        ]);
    }
    pick(vec![
        format!("Défaite serrée. {h} méritait mieux."),
        "On rebondit à la prochaine, monsieur.".into(),
        "Pas cette fois — mais la nuit est jeune.".into(),
    ])
}

/// One game of the operator, most recent first in every list.
#[derive(Clone, Debug, PartialEq)]
pub struct OpGame<'a> {
    pub m: &'a MatchSummary,
    pub me: &'a MatchPlayer,
    pub won: bool,
    /// Local calendar day `YYYY-MM-DD`.
    pub day: String,
}

/// The operator's games (any configured account), dropping matches without a date or without the operator.
/// `day_of` turns the ISO date into the local day (browser time zone).
pub fn operator_games<'a>(
    matches: &'a [MatchSummary],
    names: &[String],
    day_of: impl Fn(&str) -> String,
) -> Vec<OpGame<'a>> {
    matches
        .iter()
        .filter_map(|m| {
            let me = match_operator(m.players.as_deref().unwrap_or(&[]), names)?;
            let at = m.played_at.as_deref()?;
            Some(OpGame {
                m,
                me,
                won: me.team.is_some() && m.winner == me.team,
                day: day_of(at),
            })
        })
        .collect()
}

/// The session: today's games, else the last day played. Returns `(games, is_today)`.
pub fn session_games<'a>(all: &[OpGame<'a>], today: &str) -> (Vec<OpGame<'a>>, bool) {
    let todays: Vec<OpGame> = all.iter().filter(|g| g.day == today).cloned().collect();
    if !todays.is_empty() {
        return (todays, true);
    }
    match all.first() {
        Some(first) => (
            all.iter().filter(|g| g.day == first.day).cloned().collect(),
            false,
        ),
        None => (vec![], false),
    }
}

/// `currentStreak`: +n wins / -n losses in a row from the most recent game.
pub fn current_streak(won: &[bool]) -> i32 {
    let Some(&first) = won.first() else { return 0 };
    let n = won.iter().take_while(|w| **w == first).count() as i32;
    if first {
        n
    } else {
        -n
    }
}

/// `streakLabel` of the queue panel.
pub fn streak_label(s: i32) -> String {
    match s {
        0 => "no streak".into(),
        s if s > 0 => format!("W{s} streak ▲"),
        s => format!("L{} streak ▼", -s),
    }
}

/// JS `toFixed(1)` for a non-negative number. Rust rounds an exact tie to even (`2.25 -> "2.2"`), JS rounds it
/// up (`"2.3"`). With one decimal an exact tie is a number whose fraction is `.25` or `.75`: the only ones
/// exactly representable in binary (a x4 is exact, so the test has no rounding of its own).
pub fn to_fixed1(x: f64) -> String {
    let tie = (x * 4.0).fract() == 0.0 && (x * 2.0).fract() != 0.0;
    if tie {
        format!("{:.1}", (x * 10.0).round() / 10.0)
    } else {
        format!("{x:.1}")
    }
}

/// K/A/D of a player: assists = takedowns - kills (never negative).
pub fn kad(p: &MatchPlayer) -> (u32, u32, u32) {
    let k = p.kills.unwrap_or(0);
    let td = p.takedowns.unwrap_or(0);
    (k, td.saturating_sub(k), p.deaths.unwrap_or(0))
}

/// Kill participation of `me` in `m`: takedowns / opposing deaths, capped at 100. `None` without opponent deaths.
pub fn kill_participation(players: &[MatchPlayer], me: &MatchPlayer) -> Option<u32> {
    let team_kills: u32 = players
        .iter()
        .filter(|p| me.team.is_some() && p.team.is_some() && p.team != me.team)
        .map(|p| p.deaths.unwrap_or(0))
        .sum();
    if team_kills == 0 {
        return None;
    }
    let td = me.takedowns.unwrap_or(0) as f64;
    Some(((td / team_kills as f64 * 100.0).round() as u32).min(100))
}

/// Win-loss record per key, in order of first appearance sorted by games played (stable, descending).
pub fn record_by<K: Clone + PartialEq>(
    games: &[OpGame],
    key: impl Fn(&OpGame) -> K,
) -> Vec<(K, u32, u32)> {
    let mut out: Vec<(K, u32, u32)> = vec![];
    for g in games {
        let k = key(g);
        let i = match out.iter().position(|(x, _, _)| *x == k) {
            Some(i) => i,
            None => {
                out.push((k, 0, 0));
                out.len() - 1
            }
        };
        if g.won {
            out[i].1 += 1;
        } else {
            out[i].2 += 1;
        }
    }
    out.sort_by_key(|(_, w, l)| std::cmp::Reverse(w + l));
    out
}

/// Aggregates of the session block of the queue panel.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTotals {
    pub kills: u32,
    pub assists: u32,
    pub deaths: u32,
    pub takedowns: u32,
    pub kda: f64,
    pub avg_len: Option<f64>,
}

pub fn session_totals(games: &[OpGame]) -> SessionTotals {
    let sum = |f: fn(&MatchPlayer) -> u32| games.iter().map(|g| f(g.me)).sum::<u32>();
    let kills = sum(|p| p.kills.unwrap_or(0));
    let deaths = sum(|p| p.deaths.unwrap_or(0));
    let takedowns = sum(|p| p.takedowns.unwrap_or(0));
    let assists = takedowns.saturating_sub(kills);
    let ka = (kills + assists) as f64;
    let kda = if deaths > 0 { ka / deaths as f64 } else { ka };
    let lengths: Vec<f64> = games.iter().filter_map(|g| g.m.length).collect();
    let avg_len = (!lengths.is_empty()).then(|| lengths.iter().sum::<f64>() / lengths.len() as f64);
    SessionTotals {
        kills,
        assists,
        deaths,
        takedowns,
        kda,
        avg_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, team: i64, hero: &str, k: u32, td: u32, d: u32) -> MatchPlayer {
        MatchPlayer {
            name: Some(name.into()),
            hero: Some(hero.into()),
            team: Some(team),
            kills: Some(k),
            takedowns: Some(td),
            deaths: Some(d),
            award: None,
            win: None,
        }
    }

    fn m(id: i64, day: &str, winner: i64, me: MatchPlayer) -> MatchSummary {
        MatchSummary {
            id,
            map: Some("Cursed Hollow".into()),
            mode: Some(50091),
            played_at: Some(day.into()),
            length: Some(600.0),
            winner: Some(winner),
            players: Some(vec![me, p("enemy", 1, "Zeratul", 3, 5, 6)]),
        }
    }

    #[test]
    fn small_formatters_match_the_react_ones() {
        assert_eq!(fmt_dur(Some(754.4)), "12:34");
        assert_eq!(fmt_dur(Some(0.0)), "—");
        assert_eq!(fmt_dur(None), "—");
        assert_eq!(mode_badge(Some(50091)), ("SL", "b-sl"));
        assert_eq!(mode_badge(Some(7)), ("—", "b-qm"));
        assert_eq!(mode_badge(None), ("—", "b-qm"));
        assert_eq!(
            map_image(Some("Garden of Terror")).as_deref(),
            Some("/images/battlegrounds/garden-of-terror.png")
        );
        assert_eq!(map_slug("Dragon  Shire's"), "dragon-shires");
        assert_eq!(map_image(None), None);
    }

    #[test]
    fn awards_are_decamelised_and_mvp_is_flagged() {
        let mvp = award_label(Some("EndOfMatchAwardMVPBoolean")).unwrap();
        assert!(mvp.mvp && mvp.label == "MVP" && mvp.icon == "👑");
        let a = award_label(Some("EndOfMatchAwardMostHeroDamageDoneBoolean")).unwrap();
        assert_eq!(
            (a.label.as_str(), a.icon, a.mvp),
            ("Hero Damage", "🗡️", false)
        );
        let z = award_label(Some("EndOfMatchAward0DeathsBoolean")).unwrap();
        assert_eq!((z.label.as_str(), z.icon), ("0Deaths", "😇"));
        assert_eq!(award_label(None), None);
        assert_eq!(award_label(Some("")), None);
        assert_eq!(
            award_label(Some("EndOfMatchAwardNewThingBoolean"))
                .unwrap()
                .icon,
            "🏅"
        );
    }

    #[test]
    fn operator_lookup_prefers_the_override_then_names_then_first() {
        let ps = vec![p("Alpha", 0, "A", 0, 0, 0), p("Matella", 1, "B", 0, 0, 0)];
        let names = vec!["matella".to_string()];
        assert_eq!(
            pick_operator(&ps, Some("alpha"), &names)
                .unwrap()
                .hero
                .as_deref(),
            Some("A")
        );
        assert_eq!(
            pick_operator(&ps, None, &names).unwrap().hero.as_deref(),
            Some("B")
        );
        assert_eq!(
            pick_operator(&ps, Some("nobody"), &[])
                .unwrap()
                .hero
                .as_deref(),
            Some("A")
        );
        assert_eq!(match_operator(&ps, &[]), None); // strict: no fallback
        assert!(match_operator(&ps, &names).is_some());
    }

    #[test]
    fn jarvis_is_deterministic_and_follows_the_context() {
        assert_eq!(
            jarvis_phrase(true, Some("Valla"), 0, 4, 0),
            "Aucune mort. Une partie de maître, monsieur."
        );
        assert_eq!(
            jarvis_phrase(true, Some("Valla"), 0, 5, 0),
            "Zéro mort — proprement exécuté."
        );
        assert_eq!(
            jarvis_phrase(true, None, 3, 15, 0),
            "Démonstration. ce héros était intenable."
        );
        assert_eq!(
            jarvis_phrase(true, None, 3, 4, 4),
            "Série de 4. Difficile de faire mieux."
        );
        assert_eq!(
            jarvis_phrase(false, None, 9, 3, 0),
            "Trop de morts, monsieur. On respire au prochain."
        );
        assert_eq!(
            jarvis_phrase(false, Some("Valla"), 1, 3, -3),
            "Série difficile, monsieur. Gardons la tête froide."
        );
        assert_eq!(
            jarvis_phrase(false, Some("Valla"), 1, 2, 0),
            "Défaite serrée. Valla méritait mieux."
        );
    }

    #[test]
    fn session_is_today_else_the_last_day_played() {
        let names = vec!["me".to_string()];
        let ms = vec![
            m(3, "2026-11-10T20:00:00Z", 0, p("me", 0, "A", 5, 8, 2)),
            m(2, "2026-11-10T19:00:00Z", 1, p("me", 0, "B", 1, 2, 6)),
            m(1, "2026-11-09T19:00:00Z", 0, p("me", 0, "C", 0, 0, 0)),
            m(0, "2026-11-09T18:00:00Z", 0, p("someone", 0, "C", 0, 0, 0)), // not the operator
        ];
        let day = |s: &str| s[..10].to_string();
        let all = operator_games(&ms, &names, day);
        assert_eq!(all.len(), 3);
        assert!(all[0].won && !all[1].won);
        let (today, is_today) = session_games(&all, "2026-11-10");
        assert!(is_today && today.len() == 2);
        let (last, is_today) = session_games(&all, "2026-11-12");
        assert!(!is_today && last.len() == 2 && last[0].day == "2026-11-10");
        assert_eq!(session_games(&[], "2026-11-12"), (vec![], false));
    }

    #[test]
    fn streaks_records_and_totals() {
        assert_eq!(current_streak(&[]), 0);
        assert_eq!(current_streak(&[true, true, false, true]), 2);
        assert_eq!(current_streak(&[false, false, false, true]), -3);
        assert_eq!(streak_label(0), "no streak");
        assert_eq!(streak_label(2), "W2 streak ▲");
        assert_eq!(streak_label(-3), "L3 streak ▼");

        let names = vec!["me".to_string()];
        let ms = vec![
            m(3, "2026-11-10", 0, p("me", 0, "A", 5, 8, 2)),
            m(2, "2026-11-10", 1, p("me", 0, "B", 1, 2, 6)),
            m(1, "2026-11-10", 0, p("me", 0, "A", 4, 6, 0)),
        ];
        let all = operator_games(&ms, &names, |s| s.to_string());
        let heroes = record_by(&all, |g| g.me.hero.clone().unwrap_or_default());
        assert_eq!(
            heroes,
            vec![("A".to_string(), 2, 0), ("B".to_string(), 0, 1)]
        );
        let t = session_totals(&all);
        assert_eq!((t.kills, t.assists, t.deaths, t.takedowns), (10, 6, 8, 16));
        assert!((t.kda - 2.0).abs() < 1e-9);
        assert_eq!(t.avg_len, Some(600.0));
    }

    #[test]
    fn to_fixed1_rounds_ties_up_like_javascript() {
        assert_eq!(to_fixed1(2.25), "2.3");
        assert_eq!(to_fixed1(0.15), "0.1"); // below the tie in binary
        assert_eq!(to_fixed1(2.0), "2.0");
        assert_eq!(to_fixed1(2.5), "2.5");
        assert_eq!(to_fixed1(3.46), "3.5");
        assert_eq!(to_fixed1(0.0), "0.0");
        assert_eq!(to_fixed1(12.75), "12.8");
        assert_eq!(to_fixed1(0.125), "0.1"); // not a tie at one decimal
    }

    #[test]
    fn kad_and_kill_participation() {
        let me = p("me", 0, "A", 3, 10, 2);
        assert_eq!(kad(&me), (3, 7, 2));
        assert_eq!(kad(&p("x", 0, "A", 5, 3, 1)), (5, 0, 1)); // never negative
        let ps = vec![
            me.clone(),
            p("e1", 1, "B", 0, 0, 8),
            p("e2", 1, "C", 0, 0, 4),
        ];
        assert_eq!(kill_participation(&ps, &me), Some(83));
        assert_eq!(kill_participation(&ps[..1], &me), None);
        let big = p("me", 0, "A", 3, 40, 2);
        assert_eq!(kill_participation(&ps, &big), Some(100));
    }
}
