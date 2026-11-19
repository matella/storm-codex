//! Replay search filters of `/api/matches` and `/api/matches.csv` (spec
//! `docs/specs/2026-11-19-replay-search.md`): parsing and validation of the perspective-row filters
//! (`my_hero`, `with`, `against`, `min_<stat>` / `max_<stat>`) and game-length bounds, and the SQL
//! fragments they add to the archive query. Every value is bound; only constant fragments are pushed.

use sqlx::{Postgres, QueryBuilder};

/// Rejected filter parameter (the name is returned to the client as `param`).
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("bad filter parameter `{0}`")]
pub struct BadFilter(pub String);

/// Stats that can be bounded: typed columns of the perspective row only (no JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    Kills,
    Deaths,
    Takedowns,
    Assists,
    HeroDamage,
    Healing,
    Xp,
    Apm,
}

impl Stat {
    fn parse(name: &str) -> Option<Stat> {
        Some(match name {
            "kills" => Stat::Kills,
            "deaths" => Stat::Deaths,
            "takedowns" => Stat::Takedowns,
            "assists" => Stat::Assists,
            "hero_damage" => Stat::HeroDamage,
            "healing" => Stat::Healing,
            "xp" => Stat::Xp,
            "apm" => Stat::Apm,
            _ => return None,
        })
    }

    /// SQL expression on the perspective row `me` (a constant, never user text).
    fn sql(self) -> &'static str {
        match self {
            Stat::Kills => "me.kills",
            Stat::Deaths => "me.deaths",
            Stat::Takedowns => "me.takedowns",
            Stat::Assists => "(me.takedowns - me.kills)",
            Stat::HeroDamage => "me.hero_damage",
            Stat::Healing => "me.healing",
            Stat::Xp => "me.experience",
            Stat::Apm => "me.apm",
        }
    }
}

/// Upper clamp of a stat bound (larger than any real total; keeps the bound finite).
const STAT_CAP: f64 = 1e12;
/// Upper clamp of a length bound, in minutes.
const LEN_CAP_MIN: f64 = 600.0;

#[derive(Debug, Default, PartialEq)]
pub struct Search {
    pub my_hero: Option<String>,
    pub with: Option<String>,
    pub against: Option<String>,
    /// `(stat, lower bound?, value)`, inclusive; `true` = minimum.
    pub stats: Vec<(Stat, bool, f64)>,
    /// Game length bounds in seconds (inclusive), converted from the minutes of the query.
    pub min_len_s: Option<f64>,
    pub max_len_s: Option<f64>,
}

fn number(key: &str, value: &str, cap: f64) -> Result<f64, BadFilter> {
    let v: f64 = value
        .trim()
        .parse()
        .map_err(|_| BadFilter(key.to_string()))?;
    if !v.is_finite() {
        return Err(BadFilter(key.to_string()));
    }
    Ok(v.clamp(0.0, cap))
}

/// Parses the replay search parameters out of the raw query pairs; other parameters are ignored
/// (they belong to `MatchFilter`). Empty values count as absent; unknown `min_*` / `max_*` names
/// are rejected, not ignored.
pub fn parse(pairs: &[(String, String)]) -> Result<Search, BadFilter> {
    let mut s = Search::default();
    for (key, value) in pairs {
        if value.trim().is_empty() {
            continue;
        }
        let text = || Some(value.trim().to_string());
        match key.as_str() {
            "my_hero" => s.my_hero = text(),
            "with" => s.with = text(),
            "against" => s.against = text(),
            "min_len" => s.min_len_s = Some(number(key, value, LEN_CAP_MIN)? * 60.0),
            "max_len" => s.max_len_s = Some(number(key, value, LEN_CAP_MIN)? * 60.0),
            _ => {
                let (min, stat) = match (key.strip_prefix("min_"), key.strip_prefix("max_")) {
                    (Some(stat), _) => (true, stat),
                    (None, Some(stat)) => (false, stat),
                    (None, None) => continue,
                };
                let stat = Stat::parse(stat).ok_or_else(|| BadFilter(key.clone()))?;
                s.stats.push((stat, min, number(key, value, STAT_CAP)?));
            }
        }
    }
    if s.with.is_some() && s.with == s.against {
        return Err(BadFilter("against".into()));
    }
    Ok(s)
}

impl Search {
    /// True when a filter reads the perspective row (needs an operator account or `account`).
    pub fn needs_perspective(&self) -> bool {
        self.my_hero.is_some()
            || self.with.is_some()
            || self.against.is_some()
            || !self.stats.is_empty()
    }

    /// Appends the `AND …` conditions (matches `m`, perspective row `me`).
    pub fn push_where(&self, qb: &mut QueryBuilder<Postgres>) {
        if let Some(h) = &self.my_hero {
            qb.push(" AND me.hero = ").push_bind(h.clone());
        }
        for (toon, same_team) in [(&self.with, true), (&self.against, false)] {
            if let Some(t) = toon {
                qb.push(
                    " AND EXISTS (SELECT 1 FROM match_players o WHERE o.match_id = m.id AND o.toon_handle = ",
                )
                .push_bind(t.clone())
                .push(if same_team {
                    " AND o.team = me.team AND o.toon_handle <> me.toon_handle)"
                } else {
                    " AND o.team <> me.team)"
                });
            }
        }
        for &(stat, min, v) in &self.stats {
            qb.push(" AND ")
                .push(stat.sql())
                .push(if min { " >= " } else { " <= " })
                .push_bind(v)
                .push("::float8");
        }
        if let Some(v) = self.min_len_s {
            qb.push(" AND m.length >= ").push_bind(v).push("::float8");
        }
        if let Some(v) = self.max_len_s {
            qb.push(" AND m.length <= ").push_bind(v).push("::float8");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> Result<Search, BadFilter> {
        let v: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        parse(&v)
    }

    #[test]
    fn bounds_are_parsed_clamped_and_inclusive() {
        let s = q(&[
            ("map", "Braxis Holdout"),
            ("min_deaths", "9"),
            ("max_kills", "-3"),
            ("min_apm", "120.5"),
            ("min_hero_damage", "1e15"),
            ("min_len", "10"),
            ("max_len", "15.5"),
        ])
        .unwrap();
        assert_eq!(
            s.stats,
            vec![
                (Stat::Deaths, true, 9.0),
                (Stat::Kills, false, 0.0),
                (Stat::Apm, true, 120.5),
                (Stat::HeroDamage, true, STAT_CAP),
            ]
        );
        assert_eq!((s.min_len_s, s.max_len_s), (Some(600.0), Some(930.0)));
        assert!(s.needs_perspective());
        let mut qb = QueryBuilder::<Postgres>::new("SELECT 1 FROM matches m WHERE true");
        s.push_where(&mut qb);
        let sql = qb.sql().as_str().to_owned();
        assert!(sql.contains("me.deaths >= $1::float8"), "{sql}");
        assert!(sql.contains("me.kills <= $2::float8"), "{sql}");
        assert!(sql.contains("m.length <= $6::float8"), "{sql}");
    }

    #[test]
    fn assists_are_derived_and_xp_maps_to_experience() {
        let s = q(&[("min_assists", "5"), ("max_xp", "20000")]).unwrap();
        let mut qb = QueryBuilder::<Postgres>::new("");
        s.push_where(&mut qb);
        let sql = qb.sql().as_str().to_owned();
        assert!(sql.contains("(me.takedowns - me.kills) >= $1"), "{sql}");
        assert!(sql.contains("me.experience <= $2"), "{sql}");
    }

    #[test]
    fn unknown_stats_and_bad_numbers_name_the_parameter() {
        assert_eq!(q(&[("min_siege", "1")]), Err(BadFilter("min_siege".into())));
        assert_eq!(q(&[("max_", "1")]), Err(BadFilter("max_".into())));
        assert_eq!(
            q(&[("min_kills", "lots")]),
            Err(BadFilter("min_kills".into()))
        );
        assert_eq!(q(&[("min_len", "NaN")]), Err(BadFilter("min_len".into())));
        assert_eq!(
            q(&[("max_deaths", "inf")]),
            Err(BadFilter("max_deaths".into()))
        );
    }

    #[test]
    fn empty_values_are_absent_and_length_alone_needs_no_perspective() {
        let s = q(&[("with", ""), ("min_kills", " "), ("min_len", "12")]).unwrap();
        assert!(!s.needs_perspective());
        assert_eq!(s.min_len_s, Some(720.0));
        assert_eq!(q(&[]).unwrap(), Search::default());
    }

    #[test]
    fn with_and_against_combine_but_not_on_the_same_toon() {
        let s = q(&[
            ("with", "1-Hero-1-2"),
            ("against", "1-Hero-1-3"),
            ("my_hero", "Jaina"),
        ])
        .unwrap();
        assert_eq!(s.with.as_deref(), Some("1-Hero-1-2"));
        assert_eq!(s.against.as_deref(), Some("1-Hero-1-3"));
        let mut qb = QueryBuilder::<Postgres>::new("");
        s.push_where(&mut qb);
        let sql = qb.sql().as_str().to_owned();
        assert!(sql.contains("me.hero = $1"), "{sql}");
        assert!(sql.contains("o.team = me.team"), "{sql}");
        assert!(sql.contains("o.team <> me.team"), "{sql}");
        assert_eq!(
            q(&[("with", "1-Hero-1-2"), ("against", "1-Hero-1-2")]),
            Err(BadFilter("against".into()))
        );
    }
}
