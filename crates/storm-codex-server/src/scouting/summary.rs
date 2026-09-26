//! `Output` storm-stats → `GameSummary` : le strict nécessaire au calcul des faits de scouting,
//! stocké dans `scouting_games.summary`. Le recalcul des faits (à chaque ajout de replay) ne lit
//! que ces résumés — quelques Ko par partie au lieu de l'objet `match` complet.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as J};

/// Phase d'un ban. Le parser écrit `order = 2` pour le ban de milieu de draft, avant comme après
/// le build 66292 (`process.rs:2277-2303`) — c'est le seul critère stable entre les deux formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BanPhase {
    First,
    Mid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ban {
    pub hero: String,
    pub phase: BanPhase,
}

/// Stats d'un joueur retenues pour le scouting (gameStats du parser).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub kills: f64,
    pub deaths: f64,
    pub assists: f64,
    pub hero_damage: f64,
    pub siege_damage: f64,
    pub healing: f64,
    pub self_healing: f64,
    pub damage_taken: f64,
    pub xp_contribution: f64,
    pub time_dead: f64,
    pub merc_captures: f64,
    pub kill_participation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub toon: String,
    pub name: String,
    pub hero: String,
    pub stats: PlayerStats,
}

/// Segment de la chronologie d'avantage de niveau : `diff` = niveau équipe 0 − niveau équipe 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelSegment {
    pub start: f64,
    pub end: f64,
    pub diff: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameSummary {
    pub date: Option<String>,
    pub map: String,
    pub build: Option<i64>,
    pub length_s: f64,
    /// Équipe gagnante (0/1).
    pub winner: Option<u8>,
    pub teams: [Vec<Player>; 2],
    pub bans: [Vec<Ban>; 2],
    /// Héros pickés par équipe, dans l'ordre de pick de l'équipe (lu dans le replay).
    pub picks: [Vec<String>; 2],
    pub first_pick: Option<u8>,
    pub first_fort: Option<u8>,
    pub first_objective: Option<u8>,
    /// Instant (s) où chaque équipe atteint le niveau 10 ; `None` si jamais atteint.
    pub level10: [Option<f64>; 2],
    pub level_diff: Vec<LevelSegment>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SummaryError {
    #[error("sortie de parse sans match/players")]
    Empty,
    #[error("champ manquant : {0}")]
    Missing(&'static str),
}

fn team_of(v: Option<&J>) -> Option<u8> {
    match v.and_then(J::as_i64) {
        Some(0) => Some(0),
        Some(1) => Some(1),
        _ => None,
    }
}

fn gs(p: &J, k: &str) -> f64 {
    p.get("gameStats")
        .and_then(|g| g.get(k))
        .and_then(J::as_f64)
        .unwrap_or(0.0)
}

/// Code attribut de ban (`Crus`) → nom canonique parser (`Johanna`) ; inchangé si inconnu.
pub fn ban_hero_name(code: &str) -> String {
    storm_stats::constants::hero_attribute()
        .get(code)
        .and_then(J::as_str)
        .unwrap_or(code)
        .to_string()
}

fn per_team<T>(m: &Map<String, J>, key: &str, f: impl Fn(&J) -> Vec<T>) -> [Vec<T>; 2] {
    let side = |t: &str| m.get(key).and_then(|o| o.get(t)).map(&f).unwrap_or_default();
    [side("0"), side("1")]
}

pub fn summarize(out: &storm_stats::Output) -> Result<GameSummary, SummaryError> {
    let (Some(m), Some(players)) = (out.match_.as_ref(), out.players.as_ref()) else {
        return Err(SummaryError::Empty);
    };
    let map = m
        .get("map")
        .and_then(J::as_str)
        .ok_or(SummaryError::Missing("map"))?
        .to_string();

    let mut teams: [Vec<Player>; 2] = [Vec::new(), Vec::new()];
    // ordre stable (toon) : la sortie du parser est une map, l'ordre d'insertion n'a pas de sens
    let mut toons: Vec<&String> = players.keys().collect();
    toons.sort();
    for toon in toons {
        let p = &players[toon.as_str()];
        let Some(t) = team_of(p.get("team")) else { continue };
        teams[t as usize].push(Player {
            toon: toon.clone(),
            name: p.get("name").and_then(J::as_str).unwrap_or_default().to_string(),
            hero: p.get("hero").and_then(J::as_str).unwrap_or_default().to_string(),
            stats: PlayerStats {
                kills: gs(p, "SoloKill"),
                deaths: gs(p, "Deaths"),
                assists: gs(p, "Assists"),
                hero_damage: gs(p, "HeroDamage"),
                siege_damage: gs(p, "SiegeDamage"),
                healing: gs(p, "Healing"),
                self_healing: gs(p, "SelfHealing"),
                damage_taken: gs(p, "DamageTaken"),
                xp_contribution: gs(p, "ExperienceContribution"),
                time_dead: gs(p, "TimeSpentDead"),
                merc_captures: gs(p, "MercCampCaptures"),
                kill_participation: gs(p, "KillParticipation"),
            },
        });
    }

    let bans = per_team(m, "bans", |arr| {
        arr.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        let code = b.get("hero").and_then(J::as_str)?;
                        let phase = if b.get("order").and_then(J::as_i64) == Some(2) {
                            BanPhase::Mid
                        } else {
                            BanPhase::First
                        };
                        Some(Ban { hero: ban_hero_name(code), phase })
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    let picks = per_team(m, "picks", |arr| {
        arr.as_array()
            .map(|a| a.iter().filter_map(|h| h.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    });

    let level10 = |t: &str| {
        m.get("levelTimes")
            .and_then(|l| l.get(t))
            .and_then(|l| l.get("10"))
            .and_then(|l| l.get("time"))
            .and_then(J::as_f64)
    };
    let level_diff = m
        .get("levelAdvTimeline")
        .and_then(J::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    Some(LevelSegment {
                        start: s.get("start")?.as_f64()?,
                        end: s.get("end")?.as_f64()?,
                        diff: s.get("levelDiff")?.as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(GameSummary {
        date: m.get("date").and_then(J::as_str).map(str::to_string),
        map,
        build: m
            .get("version")
            .and_then(|v| v.get("m_build"))
            .and_then(J::as_i64),
        length_s: m.get("length").and_then(J::as_f64).unwrap_or(0.0),
        winner: team_of(m.get("winner")),
        teams,
        bans,
        picks,
        first_pick: m.get("picks").and_then(|p| team_of(p.get("first"))),
        first_fort: team_of(m.get("firstFort")),
        first_objective: team_of(m.get("firstObjective")),
        level10: [level10("0"), level10("1")],
        level_diff,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn code_de_ban_resolu_en_nom_de_heros() {
        assert_eq!(ban_hero_name("Crus"), "Johanna");
        assert_eq!(ban_hero_name("DEAT"), "Deathwing");
        assert_eq!(ban_hero_name("????"), "????");
    }

    #[test]
    fn resume_du_replay_committe() {
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
        let out = storm_stats::process_replay(&replay, "x.StormReplay");
        assert_eq!(out.status, 1);
        let s = summarize(&out).unwrap();
        assert_eq!(s.map, "Industrial District");
        assert_eq!(s.teams[0].len() + s.teams[1].len(), 10);
        assert!(s.teams.iter().flatten().all(|p| !p.hero.is_empty() && !p.toon.is_empty()));
        assert!(s.winner.is_some());
        assert!(s.length_s > 0.0);
        assert!(!s.level_diff.is_empty());
        // aller-retour JSON (stockage dans scouting_games.summary)
        let back: GameSummary = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn sortie_vide_refusee() {
        let out = storm_stats::Output { status: 1, match_: None, players: None };
        assert_eq!(summarize(&out), Err(SummaryError::Empty));
    }
}
