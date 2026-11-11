//! Session and Matches pages: the operator's games, the last session, form and tallies, and the URL-driven filter of
//! the archive. Port of `session.ts` (page logic) and the filter code of `Matches.tsx`.

use crate::{
    aggregates::form_encode,
    route::{query_pairs, query_value},
    session::{match_operator, MatchPlayer, MatchSummary},
};

/// One game of the operator (`MyGame`), in the order of the API (most recent first).
#[derive(Clone, Debug, PartialEq)]
pub struct MyGame<'a> {
    pub m: &'a MatchSummary,
    pub me: &'a MatchPlayer,
    pub won: bool,
}

/// `myGames`: the matches that contain one of the operator's accounts.
pub fn my_games<'a>(matches: &'a [MatchSummary], names: &[String]) -> Vec<MyGame<'a>> {
    matches
        .iter()
        .filter_map(|m| {
            let me = match_operator(m.players.as_deref().unwrap_or(&[]), names)?;
            Some(MyGame {
                m,
                me,
                won: me.team.is_some() && m.winner == me.team,
            })
        })
        .collect()
}

/// `lastSession`: the games of the most recent evening, chronological. An evening that passes midnight stays one
/// session while two consecutive games are less than 3 h apart. `ts` turns an ISO date into epoch milliseconds
/// (the browser's `Date`); a missing date counts as the epoch.
pub fn last_session<'a>(games: &[MyGame<'a>], ts: impl Fn(&str) -> f64) -> Vec<MyGame<'a>> {
    if games.is_empty() {
        return Vec::new();
    }
    let at = |g: &MyGame| g.m.played_at.as_deref().map_or(0.0, &ts);
    let mut sorted: Vec<MyGame> = games.to_vec();
    sorted.sort_by(|a, b| {
        b.m.played_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.m.played_at.as_deref().unwrap_or(""))
    });
    let mut out = vec![sorted[0].clone()];
    for g in &sorted[1..] {
        let prev = at(&out[out.len() - 1]);
        if prev - at(g) > 3.0 * 3600.0 * 1000.0 {
            break;
        }
        out.push(g.clone());
    }
    out.reverse();
    out
}

/// K / D / A of a player (assists = takedowns - kills, never negative).
pub fn kda(p: &MatchPlayer) -> (u32, u32, u32) {
    let k = p.kills.unwrap_or(0);
    (
        k,
        p.deaths.unwrap_or(0),
        p.takedowns.unwrap_or(0).saturating_sub(k),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tally {
    pub key: String,
    pub games: u32,
    pub wins: u32,
}

fn tally(games: &[MyGame], key: impl Fn(&MyGame) -> Option<String>) -> Vec<Tally> {
    let mut out: Vec<Tally> = Vec::new();
    for g in games {
        let Some(k) = key(g).filter(|k| !k.is_empty()) else {
            continue;
        };
        match out.iter_mut().find(|t| t.key == k) {
            Some(t) => {
                t.games += 1;
                t.wins += g.won as u32;
            }
            None => out.push(Tally {
                key: k,
                games: 1,
                wins: g.won as u32,
            }),
        }
    }
    out.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| a.key.to_lowercase().cmp(&b.key.to_lowercase()))
    });
    out
}

pub fn by_hero(games: &[MyGame]) -> Vec<Tally> {
    tally(games, |g| g.me.hero.clone())
}

pub fn by_mode(games: &[MyGame], label: impl Fn(Option<i64>) -> String) -> Vec<Tally> {
    tally(games, |g| Some(label(g.m.mode)))
}

/// Rounded percentage (0 without games).
pub fn pct_of(wins: u32, games: u32) -> u32 {
    if games == 0 {
        0
    } else {
        ((wins * 100) as f64 / games as f64).round() as u32
    }
}

/// Long name of a mode for the Session page.
pub fn mode_name(mode: Option<i64>) -> String {
    let short = crate::session::mode_badge(mode).0;
    match short {
        "SL" => "Storm League",
        "HL" => "Hero League",
        "TL" => "Team League",
        "UD" => "Unranked",
        "QM" => "Quick Match",
        "ARAM" => "ARAM",
        "CUSTOM" => "Custom",
        other => other,
    }
    .to_string()
}

/// Colour family of a mode name: `sl`, `aram` or `qm`.
pub fn mode_fam(label: &str) -> &'static str {
    match label {
        "ARAM" => "aram",
        "Storm League" | "Hero League" | "Team League" | "Unranked" => "sl",
        _ => "qm",
    }
}

/// One-line verdict of the evening.
pub fn verdict(wins: usize, games: usize) -> &'static str {
    if wins == games {
        "Perfect night"
    } else if wins == 0 {
        "Rough night"
    } else if wins > games - wins {
        "Winning night"
    } else if wins == games - wins {
        "Even night"
    } else {
        "Tough night"
    }
}

/// `1 game` / `3 games`.
pub fn games_label(n: usize) -> String {
    format!("{n} game{}", if n > 1 { "s" } else { "" })
}

/// Filters of the Matches archive, kept in the URL (so they survive the browser's back button).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchFilter {
    pub mode: Option<i64>,
    pub map: String,
    pub hero: String,
    pub account: String,
    /// `""`, `win` or `loss`.
    pub result: String,
    pub mvp: bool,
    /// `YYYY-MM-DD`, as typed.
    pub from: String,
    pub to: String,
}

impl MatchFilter {
    pub fn from_search(search: &str) -> MatchFilter {
        let get = |k: &str| query_value(search, k).unwrap_or_default();
        MatchFilter {
            mode: Some(get("mode"))
                .filter(|m| !m.is_empty())
                .and_then(|m| m.parse::<f64>().ok())
                .map(|n| n as i64),
            map: get("map"),
            hero: get("hero"),
            account: get("account"),
            result: get("result"),
            mvp: get("mvp") == "true",
            from: get("from"),
            to: get("to"),
        }
    }

    /// Query of `GET /api/matches` (`matchesParams`, then `limit`); `from_iso` / `to_iso` are the local-midnight
    /// bounds already converted by the browser (`to` = the day after).
    pub fn api_query(
        &self,
        from_iso: Option<&str>,
        to_iso: Option<&str>,
        extra: &[(&str, &str)],
    ) -> String {
        let mut p: Vec<String> = Vec::new();
        if !self.map.is_empty() {
            p.push(format!("map={}", form_encode(&self.map)));
        }
        if let Some(m) = self.mode {
            p.push(format!("mode={m}"));
        }
        if !self.hero.is_empty() {
            p.push(format!("hero={}", form_encode(&self.hero)));
        }
        if !self.account.is_empty() {
            p.push(format!("account={}", form_encode(&self.account)));
        }
        if !self.result.is_empty() {
            p.push(format!("result={}", form_encode(&self.result)));
        }
        if self.mvp {
            p.push("mvp=true".into());
        }
        if let Some(d) = from_iso {
            p.push(format!("from={}", form_encode(d)));
        }
        if let Some(d) = to_iso {
            p.push(format!("to={}", form_encode(d)));
        }
        for (k, v) in extra {
            p.push(format!("{k}={}", form_encode(v)));
        }
        p.join("&")
    }
}

/// `setParam`: the search string with `key` set (or removed when `value` is empty), the other pairs kept in order.
pub fn set_param(search: &str, key: &str, value: &str) -> String {
    let mut pairs = query_pairs(search);
    match pairs.iter().position(|(k, _)| k == key) {
        Some(i) if value.is_empty() => {
            pairs.remove(i);
        }
        Some(i) => pairs[i].1 = value.to_string(),
        None if !value.is_empty() => pairs.push((key.to_string(), value.to_string())),
        None => {}
    }
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The archive row of a match from the operator's side (`ownPlayer`): the match's first player when no account
/// is configured to match (as `pickOperator` does without names).
pub fn own_player<'a>(m: &'a MatchSummary, names: &[String]) -> Option<&'a MatchPlayer> {
    crate::session::pick_operator(m.players.as_deref().unwrap_or(&[]), None, names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, team: i64, hero: &str) -> MatchPlayer {
        MatchPlayer {
            name: Some(name.into()),
            team: Some(team),
            hero: Some(hero.into()),
            ..MatchPlayer::default()
        }
    }

    fn game(id: i64, at: &str, winner: i64, mode: i64, hero: &str) -> MatchSummary {
        MatchSummary {
            id,
            played_at: Some(at.into()),
            winner: Some(winner),
            mode: Some(mode),
            players: Some(vec![player("me", 0, hero), player("foe", 1, "Zeratul")]),
            ..MatchSummary::default()
        }
    }

    // a stand-in for Date.parse on `2026-11-09T21:05:00Z` strings: hours and minutes of the day only
    fn ts(iso: &str) -> f64 {
        let day: f64 = iso[8..10].parse().unwrap();
        let h: f64 = iso[11..13].parse().unwrap();
        let m: f64 = iso[14..16].parse().unwrap();
        ((day * 24.0 + h) * 60.0 + m) * 60_000.0
    }

    #[test]
    fn the_session_stops_at_a_gap_of_three_hours() {
        let ms = vec![
            game(4, "2026-11-10T00:30:00Z", 0, 50091, "Valla"),
            game(3, "2026-11-09T23:10:00Z", 1, 50091, "Valla"),
            game(2, "2026-11-09T21:30:00Z", 0, 50001, "Li-Ming"),
            game(1, "2026-11-09T17:00:00Z", 0, 50001, "Li-Ming"),
        ];
        let names = vec!["Me".to_string()];
        let all = my_games(&ms, &names);
        assert_eq!(all.len(), 4);
        assert!(all[0].won && !all[1].won);
        let s = last_session(&all, ts);
        // oldest first; the 17:00 game is 4 h before the 21:30 one
        assert_eq!(s.iter().map(|g| g.m.id).collect::<Vec<_>>(), [2, 3, 4]);
        assert!(last_session(&[], ts).is_empty());
        assert!(my_games(&ms, &["nobody".into()]).is_empty());
    }

    #[test]
    fn tallies_are_sorted_by_games_then_name() {
        let ms = vec![
            game(3, "2026-11-09T23:10:00Z", 1, 50091, "Valla"),
            game(2, "2026-11-09T21:30:00Z", 0, 50001, "Li-Ming"),
            game(1, "2026-11-09T17:00:00Z", 0, 50091, "Valla"),
        ];
        let all = my_games(&ms, &["me".into()]);
        let h = by_hero(&all);
        assert_eq!((h[0].key.as_str(), h[0].games, h[0].wins), ("Valla", 2, 1));
        let m = by_mode(&all, mode_name);
        assert_eq!(m[0].key, "Storm League");
        assert_eq!(m[1].key, "Quick Match");
        assert_eq!(pct_of(1, 3), 33);
        assert_eq!(pct_of(0, 0), 0);
    }

    #[test]
    fn verdicts_and_labels() {
        assert_eq!(verdict(3, 3), "Perfect night");
        assert_eq!(verdict(0, 2), "Rough night");
        assert_eq!(verdict(3, 4), "Winning night");
        assert_eq!(verdict(2, 4), "Even night");
        assert_eq!(verdict(1, 4), "Tough night");
        assert_eq!(games_label(1), "1 game");
        assert_eq!(games_label(2), "2 games");
        assert_eq!(mode_fam("ARAM"), "aram");
        assert_eq!(mode_fam("Unranked"), "sl");
        assert_eq!(mode_fam("Quick Match"), "qm");
        assert_eq!(mode_name(Some(99)), "—");
    }

    #[test]
    fn kda_takes_assists_from_takedowns() {
        let p = MatchPlayer {
            kills: Some(3),
            deaths: Some(2),
            takedowns: Some(10),
            ..MatchPlayer::default()
        };
        assert_eq!(kda(&p), (3, 2, 7));
        assert_eq!(kda(&MatchPlayer::default()), (0, 0, 0));
    }

    #[test]
    fn the_archive_filter_round_trips_through_the_url() {
        let f = MatchFilter::from_search("?mode=50091&map=Cursed+Hollow&mvp=true&from=2026-11-01");
        assert_eq!(f.mode, Some(50091));
        assert_eq!(f.map, "Cursed Hollow");
        assert!(f.mvp);
        assert_eq!(
            f.api_query(Some("2026-10-31T23:00:00.000Z"), None, &[("limit", "200")]),
            "map=Cursed+Hollow&mode=50091&mvp=true&from=2026-10-31T23%3A00%3A00.000Z&limit=200"
        );
        assert_eq!(MatchFilter::from_search(""), MatchFilter::default());
    }

    #[test]
    fn set_param_keeps_order_and_removes_empty_values() {
        let s = set_param("?a=1&b=2", "b", "x y");
        assert_eq!(s, "a=1&b=x+y");
        assert_eq!(set_param(&s, "a", ""), "b=x+y");
        assert_eq!(set_param(&s, "c", "3"), "a=1&b=x+y&c=3");
        assert_eq!(set_param("", "c", ""), "");
    }
}
