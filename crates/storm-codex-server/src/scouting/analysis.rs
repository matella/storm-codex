//! Import de l'analyse du LLM (spec volet E). Module pur : texte brut collé par l'opérateur →
//! JSON extrait → structure validée (tolérante) → preuves résolues contre l'index des faits.
//!
//! L'étape LLM est manuelle et les modèles locaux entourent volontiers leur JSON de prose : on
//! extrait d'abord un bloc ```json, sinon le premier objet JSON équilibré qui se parse.

use super::facts::FactRef;
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::BTreeMap;

/// Version du format de réponse attendu. v2 (2026-09-26) : plan de draft carte par carte — la v1
/// (notes joueurs + plan de jeu général) n'est plus acceptée à l'import.
pub const FORMAT_VERSION: i64 = 2;

/// Un héros à bannir, à prendre, ou attendu en face. `phase` (bans) : `first` / `mid` ;
/// `player` (picks adverses attendus) : `pN`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeroCall {
    #[serde(default)]
    pub hero: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub player: Option<String>,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    #[serde(default)]
    pub point: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapCall {
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// Cartes à choisir / à éviter quand on a le choix de la carte.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapChoice {
    #[serde(default)]
    pub pick: Vec<MapCall>,
    #[serde(default)]
    pub avoid: Vec<MapCall>,
}

/// Plan de draft pour une carte.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapPlan {
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub confidence: Option<String>,
    /// Ce qu'on attend d'eux sur cette carte (preuves dans `evidence`).
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub bans: Vec<HeroCall>,
    #[serde(default)]
    pub picks: Vec<HeroCall>,
    #[serde(default)]
    pub their_picks: Vec<HeroCall>,
    #[serde(default)]
    pub considerations: Vec<Point>,
}

/// Plan valable sur n'importe quelle carte (cartes absentes des replays incluses).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct General {
    #[serde(default)]
    pub bans: Vec<HeroCall>,
    #[serde(default)]
    pub picks: Vec<HeroCall>,
    #[serde(default)]
    pub considerations: Vec<Point>,
}

/// Analyse normalisée, telle que stockée dans `scouting_reports.analysis`. Les champs inconnus
/// sont ignorés, les sections absentes valent vide.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub map_choice: MapChoice,
    #[serde(default)]
    pub maps: Vec<MapPlan>,
    #[serde(default)]
    pub general: General,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ImportError {
    #[error("no JSON object found in the pasted text")]
    NoJson,
    #[error("the JSON does not match the expected format: {0}")]
    Invalid(String),
    #[error("unsupported format_version {0} (expected 2 — copy the pack again: the response format changed)")]
    FormatVersion(String),
    #[error("this analysis is for report {got}, not report {expected}")]
    WrongReport { expected: i64, got: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub analysis: Analysis,
    pub facts_version: Option<i64>,
    pub warnings: Vec<String>,
}

/// Premier bloc ```json … ``` (ou ``` … ``` dont le contenu commence par `{`).
fn fenced(text: &str) -> Option<&str> {
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let body_start = after.find('\n').map_or(after.len(), |i| i + 1);
        let lang = after[..body_start].trim().to_ascii_lowercase();
        let body = &after[body_start..];
        let end = body.find("```")?;
        let inner = body[..end].trim();
        if (lang.is_empty() || lang == "json") && inner.starts_with('{') {
            return Some(inner);
        }
        rest = &body[end + 3..];
    }
    None
}

/// Objet équilibré commençant à `start` (qui pointe sur `{`), accolades des chaînes ignorées.
fn balanced_from(text: &str, start: usize) -> Option<&str> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, c) in text[start..].char_indices() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extrait la valeur JSON de la réponse : bloc fenced, sinon premier objet équilibré qui se parse.
pub fn extract(text: &str) -> Result<J, ImportError> {
    if let Some(block) = fenced(text) {
        return serde_json::from_str(block).map_err(|e| ImportError::Invalid(e.to_string()));
    }
    let mut last_err = None;
    for (i, _) in text.match_indices('{') {
        if let Some(obj) = balanced_from(text, i) {
            match serde_json::from_str::<J>(obj) {
                Ok(v) if v.is_object() => return Ok(v),
                Ok(_) => {}
                Err(e) => last_err = Some(e.to_string()),
            }
        }
    }
    Err(last_err.map_or(ImportError::NoJson, ImportError::Invalid))
}

/// Entier tolérant : `12` ou `"12"`.
fn int(v: Option<&J>) -> Option<i64> {
    match v? {
        J::Number(n) => n.as_i64(),
        J::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Texte collé → analyse validée. `current_facts_version` sert à avertir d'une analyse faite sur
/// un état antérieur du rapport ; elle est tout de même acceptée.
pub fn import(text: &str, report_id: i64, current_facts_version: i64) -> Result<Imported, ImportError> {
    let v = extract(text)?;
    match int(v.get("format_version")) {
        Some(FORMAT_VERSION) => {}
        _ => {
            let got = v.get("format_version").map_or("missing".to_string(), J::to_string);
            return Err(ImportError::FormatVersion(got));
        }
    }
    let mut warnings = Vec::new();
    match int(v.get("report_id")) {
        Some(got) if got != report_id => {
            return Err(ImportError::WrongReport { expected: report_id, got })
        }
        Some(_) => {}
        None => warnings.push("report_id missing — assumed to be this report".to_string()),
    }
    let facts_version = int(v.get("facts_version"));
    match facts_version {
        Some(fv) if fv < current_facts_version => warnings.push(format!(
            "analysis made on facts v{fv}; the report is now at v{current_facts_version} (replays or roster changed since)"
        )),
        None => warnings.push("facts_version missing".to_string()),
        _ => {}
    }
    let analysis: Analysis =
        serde_json::from_value(v).map_err(|e| ImportError::Invalid(e.to_string()))?;
    Ok(Imported { analysis, facts_version, warnings })
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tally {
    pub claims: u32,
    pub unsupported: u32,
    pub unknown_ids: u32,
}

/// Remplace chaque `evidence: [id]` par `[{id, known, label, text, n, low}]` et ajoute
/// `unsupported` (aucune preuve connue) — contre les faits **courants**. Renvoie aussi le décompte.
pub fn resolve(analysis: &Analysis, index: &BTreeMap<String, FactRef>) -> (J, Tally) {
    let mut v = serde_json::to_value(analysis).unwrap_or(J::Null);
    let mut tally = Tally::default();
    walk(&mut v, index, &mut tally);
    (v, tally)
}

fn walk(v: &mut J, index: &BTreeMap<String, FactRef>, t: &mut Tally) {
    match v {
        J::Array(a) => a.iter_mut().for_each(|x| walk(x, index, t)),
        J::Object(o) => {
            if let Some(J::Array(ids)) = o.get("evidence") {
                let mut known = 0;
                let resolved: Vec<J> = ids
                    .iter()
                    .filter_map(J::as_str)
                    .map(|id| match index.get(id.trim()) {
                        Some(f) => {
                            known += 1;
                            serde_json::json!({"id": id, "known": true, "label": f.label,
                                               "text": f.text, "n": f.n, "low": f.low})
                        }
                        None => {
                            t.unknown_ids += 1;
                            serde_json::json!({"id": id, "known": false})
                        }
                    })
                    .collect();
                t.claims += 1;
                if known == 0 {
                    t.unsupported += 1;
                }
                o.insert("evidence".into(), J::Array(resolved));
                o.insert("unsupported".into(), J::Bool(known == 0));
            }
            for (k, x) in o.iter_mut() {
                if k != "evidence" {
                    walk(x, index, t);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const OK: &str = r#"{"format_version":2,"report_id":7,"facts_version":3,"summary":"s",
        "map_choice":{"pick":[{"map":"Braxis Holdout","why":"w","evidence":["map.braxis_holdout.record"]}]},
        "maps":[{"map":"Braxis Holdout","confidence":"low","overview":"o","evidence":["map.braxis_holdout.record"],
                 "bans":[{"hero":"Tyrael","phase":"first","why":"w","evidence":["p1.record"],"extra":1}],
                 "their_picks":[{"hero":"Junkrat","player":"p3","why":"w","evidence":["nope"]}]}],
        "general":{"considerations":[{"point":"x","evidence":[]}]}}"#;

    #[test]
    fn bloc_fenced_avec_prose_autour() {
        let text = format!("Here is my analysis:\n\n```json\n{OK}\n```\nGood luck!");
        let r = import(&text, 7, 3).unwrap();
        assert_eq!(r.analysis.summary, "s");
        assert_eq!(r.analysis.maps[0].bans[0].phase.as_deref(), Some("first"));
        assert_eq!(r.analysis.maps[0].their_picks[0].player.as_deref(), Some("p3"));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn objet_nu_au_milieu_de_prose_avec_accolades_dans_les_chaines() {
        let text = r#"Sure! {not json} then {"format_version":"2","report_id":"7","facts_version":3,"summary":"a } in {text"} bye"#;
        let r = import(text, 7, 3).unwrap();
        assert_eq!(r.analysis.summary, "a } in {text");
    }

    #[test]
    fn json_casse_refuse() {
        let e = import("```json\n{\"format_version\": 2, \"summary\": }\n```", 7, 3).unwrap_err();
        assert!(matches!(e, ImportError::Invalid(_)), "{e:?}");
        assert_eq!(import("no json here", 7, 3).unwrap_err(), ImportError::NoJson);
    }

    #[test]
    fn mauvais_rapport_ou_ancien_format_refuses() {
        assert_eq!(import(OK, 8, 3).unwrap_err(), ImportError::WrongReport { expected: 8, got: 7 });
        let v1 = OK.replace("\"format_version\":2", "\"format_version\":1");
        assert!(matches!(import(&v1, 7, 3).unwrap_err(), ImportError::FormatVersion(_)));
    }

    #[test]
    fn faits_anciens_acceptes_avec_avertissement() {
        let r = import(OK, 7, 5).unwrap();
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("v3"));
    }

    #[test]
    fn preuves_resolues_et_id_inconnu_signale() {
        let r = import(OK, 7, 3).unwrap();
        let mut ix = BTreeMap::new();
        for id in ["p1.record", "map.braxis_holdout.record"] {
            ix.insert(id.to_string(), FactRef { label: "rec".into(), text: "2/3 (67%)".into(), n: 3, low: false });
        }
        let (v, t) = resolve(&r.analysis, &ix);
        assert_eq!(v["maps"][0]["bans"][0]["evidence"][0]["text"], "2/3 (67%)");
        assert_eq!(v["maps"][0]["unsupported"], false);
        assert_eq!(v["maps"][0]["their_picks"][0]["unsupported"], true);
        assert_eq!(v["maps"][0]["their_picks"][0]["evidence"][0]["known"], false);
        assert_eq!(v["general"]["considerations"][0]["unsupported"], true);
        // map_choice.pick, maps[0], maps[0].bans, maps[0].their_picks, general.considerations
        assert_eq!(t.claims, 5);
        assert_eq!(t.unsupported, 2);
        assert_eq!(t.unknown_ids, 1);
    }
}
