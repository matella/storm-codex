//! Hero reference data (portraits, universe colours) shared by the overlay and the pages.

use serde::Deserialize;

/// Hero reference data (`GET /api/dim/heroes`, replicated from HotsPatchNotes).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DimHero {
    pub icon: Option<String>,
    pub universe: Option<String>,
}

/// Tolerant join key between parser hero names and reference names (`Li-Ming` / `LiMing`, `The Lost Vikings`
/// / `LostVikings`): accents folded, alphanumerics only, lower case, leading "the" dropped. Accent folding
/// is a small Latin-1 table.
pub fn hero_key(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => {
                'a'
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ç' | 'Ç' => 'c',
            c => c,
        })
        .collect();
    let k: String = folded
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
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
        let by_key = exact
            .iter()
            .map(|(k, v)| (hero_key(k), v.clone()))
            .collect();
        Dim { exact, by_key }
    }

    pub fn get(&self, hero: &str) -> Option<&DimHero> {
        self.exact
            .get(hero)
            .or_else(|| self.by_key.get(&hero_key(hero)))
    }

    /// Portrait ring colour: the universe colour, Nexus when unknown.
    pub fn ring(&self, hero: Option<&str>) -> &'static str {
        match hero
            .and_then(|h| self.get(h))
            .and_then(|d| d.universe.as_deref())
        {
            Some("Warcraft") => "#ef9f27",
            Some("StarCraft") => "#378add",
            Some("Diablo") => "#e24b4a",
            Some("Overwatch") => "#d85a30",
            _ => "#afa9ec",
        }
    }

    /// Same colour as a CSS variable of the Nexus Codex theme (`var(--u-warcraft)`), for the session sources.
    pub fn ring_var(&self, hero: Option<&str>) -> &'static str {
        match hero
            .and_then(|h| self.get(h))
            .and_then(|d| d.universe.as_deref())
        {
            Some("Warcraft") => "var(--u-warcraft)",
            Some("StarCraft") => "var(--u-starcraft)",
            Some("Diablo") => "var(--u-diablo)",
            Some("Overwatch") => "var(--u-overwatch)",
            _ => "var(--u-nexus)",
        }
    }

    pub fn icon(&self, hero: &str) -> Option<&str> {
        self.get(hero)
            .and_then(|d| d.icon.as_deref())
            .filter(|i| !i.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hero_lookup_is_tolerant_like_the_react_one() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "LiMing".to_string(),
            DimHero {
                icon: Some("/i/liming.png".into()),
                universe: Some("Warcraft".into()),
            },
        );
        m.insert(
            "LostVikings".to_string(),
            DimHero {
                icon: Some("".into()),
                universe: Some("Nexus".into()),
            },
        );
        let d = Dim::new(m);
        assert_eq!(hero_key("Lúcio"), "lucio");
        assert_eq!(hero_key("The Lost Vikings"), "lostvikings");
        assert_eq!(d.icon("Li-Ming"), Some("/i/liming.png"));
        assert_eq!(d.ring(Some("Li-Ming")), "#ef9f27");
        assert_eq!(d.icon("The Lost Vikings"), None); // empty icon -> initials
        assert_eq!(d.ring(Some("Unknown")), "#afa9ec");
        assert_eq!(d.ring(None), "#afa9ec");
        assert_eq!(d.ring_var(Some("Li-Ming")), "var(--u-warcraft)");
        assert_eq!(d.ring_var(Some("Unknown")), "var(--u-nexus)");
    }
}
