//! Scoring pur d'une paire de comptes (aucune DB, aucun axum) : « ces deux comptes sont-ils
//! probablement la même personne ? ». Toujours une **estimation** : chaque suggestion porte ses
//! raisons lisibles. Poids et seuils sont des constantes en tête de fichier, à régler sur des cas
//! réels (plan, tâche 8) — les tests ci-dessous épinglent les comportements, pas les valeurs.

#![allow(dead_code)] // consommé par `identity` (tâche 4)

/// Parties minimales (des deux comptes) pour qu'une suggestion puisse être HAUTE.
pub const MIN_GAMES_FOR_HIGH: u32 = 15;
/// Score minimal pour afficher une suggestion (BAS).
pub const LOW_MIN_SCORE: u8 = 25;
/// Score minimal pour HAUT (en plus des conditions : 2 signaux dont un fort/moyen, assez de parties).
pub const HIGH_MIN_SCORE: u8 = 50;

/// Points : propriétaire du même token (signal fort).
const TOKEN_POINTS: f32 = 50.0;
/// Points max : co-équipiers récurrents communs (signal moyen).
const TEAMMATES_MAX: f32 = 40.0;
/// Co-équipiers communs minimum pour que le signal compte.
const TEAMMATES_MIN: u32 = 2;
/// Points max : mêmes choix de talents sur les mêmes héros (signal moyen) ; seuil d'activation.
const TALENTS_MAX: f32 = 25.0;
const TALENTS_MIN: f32 = 0.7;
/// Points max : pool de héros proche (signal faible) ; seuil d'activation du cosinus.
const HEROES_MAX: f32 = 20.0;
const HEROES_MIN: f32 = 0.6;
/// Points max : plages horaires proches (signal faible) ; seuil d'activation du cosinus.
const HOURS_MAX: f32 = 12.0;
const HOURS_MIN: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PairFeatures {
    pub games_a: u32,
    pub games_b: u32,
    /// Cosinus des parts de parties par héros, 0..1.
    pub hero_cosine: f32,
    /// Co-équipiers communs hors opérateur et propriétaires de token.
    pub shared_teammates: u32,
    /// Parties cumulées avec ces co-équipiers communs (borne la force du signal).
    pub shared_teammate_games: u32,
    /// Accord des talents sur les héros joués ≥ 3 fois par les deux, 0..1 ; `None` si aucun.
    pub talent_agreement: Option<f32>,
    /// Cosinus des histogrammes horaires (24 h UTC), 0..1.
    pub hour_cosine: f32,
    /// Les deux comptes dans la même partie (ou parties chevauchantes) : exclusion temporelle.
    pub overlapping_games: bool,
    /// Les deux comptes sont propriétaires du même token d'upload.
    pub same_token_owner: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Reason {
    /// `token` | `teammates` | `talents` | `heroes` | `hours`.
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Suggestion {
    pub level: Level,
    pub score: u8,
    pub reasons: Vec<Reason>,
}

/// `None` = pas de suggestion (score trop bas, ou exclusion temporelle).
pub fn score(f: &PairFeatures) -> Option<Suggestion> {
    if f.overlapping_games {
        return None;
    }
    let pct = |x: f32| (x * 100.0).round() as i32;
    let ramp = |x: f32, min: f32, max: f32| max * ((x - min) / (1.0 - min)).clamp(0.0, 1.0);
    let mut points = 0.0f32;
    let mut reasons = Vec::new();
    let mut strong_or_moderate = false;

    if f.same_token_owner {
        points += TOKEN_POINTS;
        strong_or_moderate = true;
        reasons.push(Reason { kind: "token", text: "Both accounts own the same upload token (same machine)".into() });
    }
    if f.shared_teammates >= TEAMMATES_MIN {
        let p = (7.0 * f.shared_teammates.min(5) as f32 + (f.shared_teammate_games / 3).min(10) as f32).min(TEAMMATES_MAX);
        points += p;
        strong_or_moderate = true;
        reasons.push(Reason {
            kind: "teammates",
            text: format!("{} recurring teammates in common ({} games together)", f.shared_teammates, f.shared_teammate_games),
        });
    }
    if let Some(a) = f.talent_agreement.filter(|a| *a >= TALENTS_MIN) {
        points += TALENTS_MAX * a.min(1.0);
        strong_or_moderate = true;
        reasons.push(Reason { kind: "talents", text: format!("Same talent choices on shared heroes ({}% agreement)", pct(a)) });
    }
    if f.hero_cosine >= HEROES_MIN {
        points += ramp(f.hero_cosine, HEROES_MIN, HEROES_MAX);
        reasons.push(Reason { kind: "heroes", text: format!("Similar hero pool ({}% overlap)", pct(f.hero_cosine)) });
    }
    if f.hour_cosine >= HOURS_MIN {
        points += ramp(f.hour_cosine, HOURS_MIN, HOURS_MAX);
        reasons.push(Reason { kind: "hours", text: format!("Plays at similar hours ({}% match)", pct(f.hour_cosine)) });
    }

    let raw = points.round().clamp(0.0, 100.0) as u8;
    if raw < LOW_MIN_SCORE {
        return None;
    }
    let high = raw >= HIGH_MIN_SCORE
        && reasons.len() >= 2
        && strong_or_moderate
        && f.games_a.min(f.games_b) >= MIN_GAMES_FOR_HIGH;
    // Un score affiché sous le seuil HAUT ne peut pas être étiqueté HAUT, et réciproquement.
    let (level, score) = if high { (Level::High, raw) } else { (Level::Low, raw.min(HIGH_MIN_SCORE - 1)) };
    Some(Suggestion { level, score, reasons })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn base() -> PairFeatures {
        PairFeatures { games_a: 100, games_b: 100, ..Default::default() }
    }
    fn level(f: &PairFeatures) -> Option<Level> {
        score(f).map(|s| s.level)
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
        let f = PairFeatures { hero_cosine: 0.99, ..base() };
        assert_ne!(level(&f), Some(Level::High));
        // héros + heures (deux signaux faibles) : jamais HAUT non plus, il manque un signal moyen/fort
        let f = PairFeatures { hero_cosine: 1.0, hour_cosine: 1.0, ..base() };
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
        assert!(s.score < HIGH_MIN_SCORE, "le score affiché ne doit pas dépasser le seuil HAUT");
    }

    #[test]
    fn token_plus_heures_donne_haut() {
        let f = PairFeatures { same_token_owner: true, hour_cosine: 0.95, ..base() };
        let s = score(&f).unwrap();
        assert_eq!(s.level, Level::High);
        assert!(s.reasons.iter().any(|r| r.kind == "token"));
        assert!(s.reasons.iter().any(|r| r.kind == "hours"));
    }

    #[test]
    fn token_seul_nest_pas_haut() {
        let f = PairFeatures { same_token_owner: true, ..base() };
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
        // L'opérateur est exclu en amont : 0 co-équipier commun, pas de token commun ; seul un
        // pool de héros moyen subsiste.
        let f = PairFeatures { hero_cosine: 0.5, hour_cosine: 0.5, ..base() };
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
                PairFeatures { hero_cosine: x, ..base() },
                PairFeatures { hour_cosine: x, ..base() },
                PairFeatures { talent_agreement: Some(x), ..base() },
                PairFeatures { shared_teammates: (x * 6.0) as u32, shared_teammate_games: (x * 30.0) as u32, ..base() },
            ];
            for (i, p) in probes.iter().enumerate() {
                let s = sc(p);
                assert!(s >= prev[i], "signal {i} : le score baisse ({} → {s}) à x={x}", prev[i]);
                prev[i] = s;
            }
        }
    }
}
