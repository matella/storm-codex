//! Pure scoring of a pair of accounts (no DB, no axum): "are these two accounts
//! probably the same person?". Always an **estimate**: each suggestion carries its
//! readable reasons. Weights and thresholds are constants at the top of the file, to be tuned on real
//! cases (plan, task 8) — the tests below pin the behaviours, not the values.

/// Minimum games (of both accounts) for a suggestion to be HIGH.
pub const MIN_GAMES_FOR_HIGH: u32 = 15;
/// Minimum score to show a suggestion (LOW).
pub const LOW_MIN_SCORE: u8 = 25;
/// Minimum score for HIGH (on top of the conditions: 2 signals including a strong/medium one, enough games).
pub const HIGH_MIN_SCORE: u8 = 50;

/// Points: same non-generic hotkey profile name (strong signal, enough alone for HIGH).
const PROFILE_POINTS: f32 = 60.0;
/// Points: owner of the same token (strong signal).
const TOKEN_POINTS: f32 = 50.0;
/// Max points: common recurring teammates (medium signal).
const TEAMMATES_MAX: f32 = 40.0;
/// Minimum common teammates for the signal to count.
const TEAMMATES_MIN: u32 = 2;
/// Max points: same talent choices on the same heroes (medium signal); activation threshold.
const TALENTS_MAX: f32 = 25.0;
const TALENTS_MIN: f32 = 0.7;
/// Max points: close hero pool (weak signal); cosine activation threshold.
const HEROES_MAX: f32 = 20.0;
const HEROES_MIN: f32 = 0.6;
/// Max points: close time slots (weak signal); cosine activation threshold.
const HOURS_MAX: f32 = 12.0;
const HOURS_MIN: f32 = 0.7;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PairFeatures {
    pub games_a: u32,
    pub games_b: u32,
    /// Cosine of the shares of games per hero, 0..1.
    pub hero_cosine: f32,
    /// Common teammates excluding the operator and the token owners.
    pub shared_teammates: u32,
    /// Cumulative games with these common teammates (bounds the signal strength).
    pub shared_teammate_games: u32,
    /// Talent agreement on the heroes played ≥ 3 times by both, 0..1; `None` if none.
    pub talent_agreement: Option<f32>,
    /// Cosine of the hourly histograms (24 h UTC), 0..1.
    pub hour_cosine: f32,
    /// Both accounts in the same game (or overlapping games): time exclusion.
    pub overlapping_games: bool,
    /// Both accounts are owners of the same upload token.
    pub same_token_owner: bool,
    /// Shared hotkey profile names (not ignored, not generic).
    pub shared_profiles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Reason {
    /// `profile` | `token` | `teammates` | `talents` | `heroes` | `hours`.
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Suggestion {
    pub level: Level,
    pub score: u8,
    pub reasons: Vec<Reason>,
}

/// `None` = no suggestion (score too low, or time exclusion).
pub fn score(f: &PairFeatures) -> Option<Suggestion> {
    if f.overlapping_games {
        return None;
    }
    let pct = |x: f32| (x * 100.0).round() as i32;
    let ramp = |x: f32, min: f32, max: f32| max * ((x - min) / (1.0 - min)).clamp(0.0, 1.0);
    let mut points = 0.0f32;
    let mut reasons = Vec::new();
    let mut strong_or_moderate = false;

    if !f.shared_profiles.is_empty() {
        points += PROFILE_POINTS;
        strong_or_moderate = true;
        reasons.push(Reason {
            kind: "profile",
            text: format!(
                "Same custom hotkey profile name: {}",
                f.shared_profiles.join(", ")
            ),
        });
    }
    if f.same_token_owner {
        points += TOKEN_POINTS;
        strong_or_moderate = true;
        reasons.push(Reason {
            kind: "token",
            text: "Both accounts own the same upload token (same machine)".into(),
        });
    }
    if f.shared_teammates >= TEAMMATES_MIN {
        let p = (7.0 * f.shared_teammates.min(5) as f32
            + (f.shared_teammate_games / 3).min(10) as f32)
            .min(TEAMMATES_MAX);
        points += p;
        strong_or_moderate = true;
        reasons.push(Reason {
            kind: "teammates",
            text: format!(
                "{} recurring teammates in common ({} games together)",
                f.shared_teammates, f.shared_teammate_games
            ),
        });
    }
    if let Some(a) = f.talent_agreement.filter(|a| *a >= TALENTS_MIN) {
        points += TALENTS_MAX * a.min(1.0);
        strong_or_moderate = true;
        reasons.push(Reason {
            kind: "talents",
            text: format!(
                "Same talent choices on shared heroes ({}% agreement)",
                pct(a)
            ),
        });
    }
    if f.hero_cosine >= HEROES_MIN {
        points += ramp(f.hero_cosine, HEROES_MIN, HEROES_MAX);
        reasons.push(Reason {
            kind: "heroes",
            text: format!("Similar hero pool ({}% overlap)", pct(f.hero_cosine)),
        });
    }
    if f.hour_cosine >= HOURS_MIN {
        points += ramp(f.hour_cosine, HOURS_MIN, HOURS_MAX);
        reasons.push(Reason {
            kind: "hours",
            text: format!("Plays at similar hours ({}% match)", pct(f.hour_cosine)),
        });
    }

    let raw = points.round().clamp(0.0, 100.0) as u8;
    if raw < LOW_MIN_SCORE {
        return None;
    }
    // A profile name chosen by hand and shared is enough on its own (operator decision 2026-11-08).
    let high = raw >= HIGH_MIN_SCORE
        && (!f.shared_profiles.is_empty()
            || (reasons.len() >= 2
                && strong_or_moderate
                && f.games_a.min(f.games_b) >= MIN_GAMES_FOR_HIGH));
    // A displayed score below the HIGH threshold cannot be labelled HIGH, and vice versa.
    let (level, score) = if high {
        (Level::High, raw)
    } else {
        (Level::Low, raw.min(HIGH_MIN_SCORE - 1))
    };
    Some(Suggestion {
        level,
        score,
        reasons,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn base() -> PairFeatures {
        PairFeatures {
            games_a: 100,
            games_b: 100,
            ..Default::default()
        }
    }
    fn level(f: &PairFeatures) -> Option<Level> {
        score(f).map(|s| s.level)
    }

    #[test]
    fn profil_partage_suffit_seul_pour_haut_mais_pas_malgre_chevauchement() {
        let f = PairFeatures {
            games_a: 3,
            games_b: 3,
            shared_profiles: vec!["Kure1".into()],
            ..Default::default()
        };
        let s = score(&f).expect("suggestion");
        assert_eq!(s.level, Level::High);
        assert!(s
            .reasons
            .iter()
            .any(|r| r.kind == "profile" && r.text.contains("Kure1")));
        let overlapping = PairFeatures {
            overlapping_games: true,
            ..f
        };
        assert!(
            score(&overlapping).is_none(),
            "exclusion temporelle prioritaire"
        );
        assert!(level(&base()).is_none(), "no signal: nothing");
    }

    #[test]
    fn chevauchement_exclut_meme_avec_tous_les_signaux() {
        let f = PairFeatures {
            overlapping_games: true,
            same_token_owner: true,
            shared_teammates: 5,
            shared_teammate_games: 40,
            talent_agreement: Some(1.0),
            hero_cosine: 1.0,
            hour_cosine: 1.0,
            ..base()
        };
        assert_eq!(score(&f), None);
    }

    #[test]
    fn heroes_seuls_jamais_haut() {
        let f = PairFeatures {
            hero_cosine: 0.99,
            ..base()
        };
        assert_ne!(level(&f), Some(Level::High));
        // heroes + hours (two weak signals): never HIGH either, a medium/strong signal is missing
        let f = PairFeatures {
            hero_cosine: 1.0,
            hour_cosine: 1.0,
            ..base()
        };
        assert_eq!(level(&f), Some(Level::Low));
    }

    #[test]
    fn peu_de_parties_jamais_haut_malgre_des_signaux_forts() {
        let f = PairFeatures {
            games_a: MIN_GAMES_FOR_HIGH - 1,
            same_token_owner: true,
            shared_teammates: 5,
            shared_teammate_games: 40,
            talent_agreement: Some(1.0),
            ..base()
        };
        let s = score(&f).unwrap();
        assert_eq!(s.level, Level::Low);
        assert!(
            s.score < HIGH_MIN_SCORE,
            "the displayed score must not exceed the HIGH threshold"
        );
    }

    #[test]
    fn token_plus_heures_donne_haut() {
        let f = PairFeatures {
            same_token_owner: true,
            hour_cosine: 0.95,
            ..base()
        };
        let s = score(&f).unwrap();
        assert_eq!(s.level, Level::High);
        assert!(s.reasons.iter().any(|r| r.kind == "token"));
        assert!(s.reasons.iter().any(|r| r.kind == "hours"));
    }

    #[test]
    fn token_seul_nest_pas_haut() {
        let f = PairFeatures {
            same_token_owner: true,
            ..base()
        };
        assert_ne!(level(&f), Some(Level::High));
    }

    #[test]
    fn coequipiers_plus_talents_donne_haut() {
        let f = PairFeatures {
            shared_teammates: 4,
            shared_teammate_games: 14,
            talent_agreement: Some(0.9),
            ..base()
        };
        assert_eq!(level(&f), Some(Level::High));
    }

    #[test]
    fn aucun_signal_ne_suggere_rien() {
        assert_eq!(score(&base()), None);
    }

    #[test]
    fn deux_comptes_ne_partageant_que_loperateur_aucun_lien() {
        // The operator is excluded upstream: 0 common teammates, no common token; only a
        // medium hero pool remains.
        let f = PairFeatures {
            hero_cosine: 0.5,
            hour_cosine: 0.5,
            ..base()
        };
        assert_eq!(score(&f), None);
    }

    #[test]
    fn raisons_lisibles_et_une_par_signal_actif() {
        let f = PairFeatures {
            same_token_owner: true,
            shared_teammates: 3,
            shared_teammate_games: 9,
            talent_agreement: Some(0.8),
            hero_cosine: 0.9,
            hour_cosine: 0.9,
            ..base()
        };
        let s = score(&f).unwrap();
        let kinds: Vec<_> = s.reasons.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["token", "teammates", "talents", "heroes", "hours"]);
        assert!(s.reasons.iter().all(|r| !r.text.trim().is_empty()));
    }

    #[test]
    fn score_monotone_en_chaque_signal() {
        let sc = |f: &PairFeatures| score(f).map_or(0, |s| s.score);
        let steps = [0.0f32, 0.3, 0.6, 0.8, 0.95, 1.0];
        let mut prev = [0u8; 4];
        for x in steps {
            let probes = [
                PairFeatures {
                    hero_cosine: x,
                    ..base()
                },
                PairFeatures {
                    hour_cosine: x,
                    ..base()
                },
                PairFeatures {
                    talent_agreement: Some(x),
                    ..base()
                },
                PairFeatures {
                    shared_teammates: (x * 6.0) as u32,
                    shared_teammate_games: (x * 30.0) as u32,
                    ..base()
                },
            ];
            for (i, p) in probes.iter().enumerate() {
                let s = sc(p);
                assert!(
                    s >= prev[i],
                    "signal {i}: the score drops ({} → {s}) at x={x}",
                    prev[i]
                );
                prev[i] = s;
            }
        }
    }
}
