//! Player profile (`/player/:toon`): wire model of `GET /api/players/{toon}` and `/identity`, plus the
//! pure display helpers ported from `web/src/pages/Player.tsx`, `api.ts` and `components/ds`.

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlayerHero {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
    pub avg_kills: Option<f64>,
    pub avg_deaths: Option<f64>,
    pub avg_takedowns: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlayerGame {
    pub match_id: i64,
    pub hero: Option<String>,
    pub map: Option<String>,
    pub mode: Option<i64>,
    pub win: Option<bool>,
    pub kills: Option<f64>,
    pub deaths: Option<f64>,
    pub takedowns: Option<f64>,
    pub apm: Option<f64>,
    pub played_at: Option<String>,
    pub award: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlayerSummary {
    pub toon: String,
    pub name: Option<String>,
    pub names: Vec<String>,
    pub matches: u32,
    pub wins: u32,
    pub avg_kills: Option<f64>,
    pub avg_deaths: Option<f64>,
    pub avg_takedowns: Option<f64>,
    pub avg_apm: Option<f64>,
    pub apm_games: u32,
    pub heroes: Vec<PlayerHero>,
    pub recent: Vec<PlayerGame>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NameSpan {
    pub name: String,
    pub games: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LinkSuggestion {
    pub toon: String,
    pub name: Option<String>,
    pub level: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Confirmed {
    pub toon: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlayerIdentity {
    pub names: Vec<NameSpan>,
    pub suggestions: Vec<LinkSuggestion>,
    pub confirmed: Vec<Confirmed>,
}

/// Hero reference data (`GET /api/dim/heroes`, replicated from HotsPatchNotes).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DimHero {
    pub icon: Option<String>,
    pub universe: Option<String>,
}

/// Tolerant join key between parser hero names and reference names (`Li-Ming` / `LiMing`, `The Lost Vikings`
/// / `LostVikings`): accents folded, alphanumerics only, lower case, leading "the" dropped. Accent folding
/// is a small Latin-1 table in the spike; the real port can use `unicode-normalization`.
pub fn hero_key(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ç' | 'Ç' => 'c',
            c => c,
        })
        .collect();
    let k: String = folded.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    k.strip_prefix("the").map(str::to_string).unwrap_or(k)
}

/// Reference data indexed both by exact name and by `hero_key`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dim {
    exact: std::collections::HashMap<String, DimHero>,
    by_key: std::collections::HashMap<String, DimHero>,
}

impl Dim {
    pub fn new(exact: std::collections::HashMap<String, DimHero>) -> Dim {
        let by_key = exact.iter().map(|(k, v)| (hero_key(k), v.clone())).collect();
        Dim { exact, by_key }
    }

    pub fn get(&self, hero: &str) -> Option<&DimHero> {
        self.exact.get(hero).or_else(|| self.by_key.get(&hero_key(hero)))
    }

    /// Portrait ring colour: the universe colour, Nexus when unknown.
    pub fn ring(&self, hero: Option<&str>) -> &'static str {
        match hero.and_then(|h| self.get(h)).and_then(|d| d.universe.as_deref()) {
            Some("Warcraft") => "#ef9f27",
            Some("StarCraft") => "#378add",
            Some("Diablo") => "#e24b4a",
            Some("Overwatch") => "#d85a30",
            _ => "#afa9ec",
        }
    }

    pub fn icon(&self, hero: &str) -> Option<&str> {
        self.get(hero).and_then(|d| d.icon.as_deref()).filter(|i| !i.is_empty())
    }
}

/// Win percentage rounded like `Math.round(100 * w / g)`; 0 without games.
pub fn pct(w: u32, g: u32) -> u32 {
    if g == 0 {
        0
    } else {
        (100.0 * w as f64 / g as f64).round() as u32
    }
}

/// `toFixed(1)` of takedowns / max(1, deaths).
pub fn kda_ratio(takedowns: Option<f64>, deaths: Option<f64>) -> String {
    format!("{:.1}", takedowns.unwrap_or(0.0) / deaths.unwrap_or(0.0).max(1.0))
}

/// A stat as JavaScript would print it: integers without a decimal point, "—" when absent.
pub fn num(v: Option<f64>) -> String {
    match v {
        None => "—".into(),
        Some(v) if v.fract() == 0.0 => format!("{}", v as i64),
        Some(v) => format!("{v}"),
    }
}

/// Active tab from the `?tab=` query value: anything but "identity" is the stats tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Stats,
    Identity,
}

impl Tab {
    pub fn from_query(v: Option<&str>) -> Tab {
        if v == Some("identity") {
            Tab::Identity
        } else {
            Tab::Stats
        }
    }
}

/// Account worth flagging on the main profile: a confirmed link first, else the first high-confidence
/// suggestion. Never a low-confidence one.
pub fn likely_alias(i: &PlayerIdentity) -> Option<(String, Option<String>)> {
    if let Some(c) = i.confirmed.first() {
        return Some((c.toon.clone(), c.name.clone()));
    }
    i.suggestions.iter().find(|s| s.level == "high").map(|s| (s.toon.clone(), s.name.clone()))
}

/// Mode badge `(short label, long label, family class)`.
pub fn mode_tag(mode: Option<i64>) -> (&'static str, &'static str, &'static str) {
    let short = match mode {
        Some(-1) => "CUSTOM",
        Some(50001) => "QM",
        Some(50021) => "VS IA",
        Some(50031) => "BRAWL",
        Some(50041) => "ENTR.",
        Some(50051) => "UD",
        Some(50061) => "HL",
        Some(50071) => "TL",
        Some(50091) => "SL",
        Some(50101) => "ARAM",
        _ => "—",
    };
    let long = match short {
        "SL" => "Storm League",
        "HL" => "Hero League",
        "TL" => "Team League",
        "UD" => "Unranked",
        "QM" => "Quick Match",
        "ARAM" => "ARAM",
        "CUSTOM" => "Custom",
        other => other,
    };
    let fam = if matches!(short, "UD" | "HL" | "TL" | "SL") {
        "sl"
    } else if short == "ARAM" {
        "aram"
    } else {
        "qm"
    };
    (short, long, fam)
}

/// `(label, is_mvp)` of an end-of-match award code, `None` when absent.
pub fn award_label(raw: Option<&str>) -> Option<(String, bool)> {
    let core = raw?.trim_start_matches("EndOfMatchAward").trim_end_matches("Boolean");
    if core == "MVP" {
        return Some(("MVP".into(), true));
    }
    let mut label = String::new();
    let mut prev_lower = false;
    for c in core.chars() {
        if c.is_uppercase() && prev_lower {
            label.push(' ');
        }
        prev_lower = c.is_lowercase();
        label.push(c);
    }
    let l = label.strip_prefix("Most ").unwrap_or(&label);
    let l = l.strip_suffix(" Done").unwrap_or(l);
    Some((l.to_string(), false))
}

/// Maps that have a baked minimap (ARAM maps do not).
const HAS_MINIMAP: [&str; 15] = [
    "alterac-pass", "battlefield-of-eternity", "blackhearts-bay", "braxis-holdout", "cursed-hollow", "dragon-shire",
    "garden-of-terror", "hanamura-temple", "haunted-mines", "infernal-shrines", "sky-temple", "tomb-of-the-spider-queen",
    "towers-of-doom", "volskaya-foundry", "warhead-junction",
];

pub fn has_map_art(map: Option<&str>) -> bool {
    map.is_some_and(|m| HAS_MINIMAP.contains(&crate::logic::map_slug(m).as_str()))
}

/// Initials of a hero for the portrait fallback (words are split on spaces only: `Li-Ming` -> `LI`).
pub fn initials(hero: &str) -> String {
    let cleaned: String = hero.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let s: String = if words.len() > 1 {
        words.iter().filter_map(|w| w.chars().next()).collect()
    } else {
        words.first().map(|w| w.to_string()).unwrap_or_default()
    };
    let s: String = s.chars().take(2).collect::<String>().to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_rate_and_kda_match_the_react_page() {
        assert_eq!(pct(1, 3), 33);
        assert_eq!(pct(2, 3), 67);
        assert_eq!(pct(0, 0), 0);
        assert_eq!(kda_ratio(Some(33.0), Some(5.0)), "6.6");
        assert_eq!(kda_ratio(Some(3.0), Some(0.0)), "3.0"); // deaths floor at 1
        assert_eq!(kda_ratio(None, None), "0.0");
    }

    #[test]
    fn numbers_print_like_javascript() {
        assert_eq!(num(Some(8.0)), "8");
        assert_eq!(num(Some(8.5)), "8.5");
        assert_eq!(num(None), "—");
    }

    #[test]
    fn only_identity_selects_the_identity_tab() {
        assert_eq!(Tab::from_query(Some("identity")), Tab::Identity);
        assert_eq!(Tab::from_query(Some("other")), Tab::Stats);
        assert_eq!(Tab::from_query(None), Tab::Stats);
    }

    #[test]
    fn alias_prefers_confirmed_and_ignores_low_confidence() {
        let s = |level: &str| LinkSuggestion { toon: "t2".into(), name: Some("B".into()), level: level.into() };
        let mut i = PlayerIdentity { names: vec![], suggestions: vec![s("low")], confirmed: vec![] };
        assert_eq!(likely_alias(&i), None);
        i.suggestions = vec![s("low"), s("high")];
        assert_eq!(likely_alias(&i), Some(("t2".into(), Some("B".into()))));
        i.confirmed = vec![Confirmed { toon: "t1".into(), name: None }];
        assert_eq!(likely_alias(&i), Some(("t1".into(), None)));
    }

    #[test]
    fn modes_and_awards() {
        assert_eq!(mode_tag(Some(50091)), ("SL", "Storm League", "sl"));
        assert_eq!(mode_tag(Some(50101)).2, "aram");
        assert_eq!(mode_tag(None), ("—", "—", "qm"));
        assert_eq!(award_label(Some("EndOfMatchAwardMVPBoolean")), Some(("MVP".into(), true)));
        assert_eq!(award_label(Some("EndOfMatchAwardMostSiegeDamageDoneBoolean")), Some(("Siege Damage".into(), false)));
        assert_eq!(award_label(Some("EndOfMatchAwardClutchHealerBoolean")), Some(("Clutch Healer".into(), false)));
        assert_eq!(award_label(None), None);
    }

    #[test]
    fn hero_lookup_is_tolerant_like_the_react_one() {
        let mut m = std::collections::HashMap::new();
        m.insert("LiMing".to_string(), DimHero { icon: Some("/i/liming.png".into()), universe: Some("Warcraft".into()) });
        m.insert("LostVikings".to_string(), DimHero { icon: Some("".into()), universe: Some("Nexus".into()) });
        let d = Dim::new(m);
        assert_eq!(hero_key("Lúcio"), "lucio");
        assert_eq!(hero_key("The Lost Vikings"), "lostvikings");
        assert_eq!(d.icon("Li-Ming"), Some("/i/liming.png"));
        assert_eq!(d.ring(Some("Li-Ming")), "#ef9f27");
        assert_eq!(d.icon("The Lost Vikings"), None); // empty icon -> initials
        assert_eq!(d.ring(Some("Unknown")), "#afa9ec");
        assert_eq!(d.ring(None), "#afa9ec");
    }

    #[test]
    fn map_art_and_initials() {
        assert!(has_map_art(Some("Blackheart's Bay")));
        assert!(!has_map_art(Some("Braxis Outpost")));
        assert_eq!(initials("Zagara"), "ZA");
        assert_eq!(initials("The Lost Vikings"), "TL");
        assert_eq!(initials("Li-Ming"), "LI");
        assert_eq!(initials("!!"), "?");
    }

    #[test]
    fn real_payload_shape_deserialises() {
        let j = r#"{"name":"BadSté","toon":"2-Hero-1-895508","wins":1,"names":["BadSté"],
          "heroes":[{"hero":"Zagara","wins":1,"games":1,"avg_kills":8.0,"avg_deaths":5.0,"avg_takedowns":33.0}],
          "recent":[{"apm":null,"map":"Braxis Outpost","win":true,"hero":"Zagara","mode":50101,"award":"EndOfMatchAwardMVPBoolean",
          "kills":8,"deaths":5,"match_id":2746,"played_at":"2026-10-25T13:32:20.325+00:00","takedowns":33}],
          "avg_apm":null,"matches":1,"apm_games":0,"avg_kills":8.0,"avg_deaths":5.0,"avg_takedowns":33.0}"#;
        let p: PlayerSummary = serde_json::from_str(j).unwrap();
        assert_eq!((p.matches, p.recent[0].kills), (1, Some(8.0)));
    }
}
