//! Patch notes pages: wire types and the pure rules (heading conversion, tag classes).
//! Port of `web/src/patchHtml.ts` and the logic of `Patch.tsx`, `HeroChanges.tsx`, `ds/patch.tsx`.

use std::collections::HashMap;

use serde::Deserialize;

/// `ptagClass`: class of the classification pill (`BUFF`, `NERF`, `MIXED`, `BUGFIX`; anything else is an update).
pub fn ptag_class(c: Option<&str>) -> &'static str {
    match c.unwrap_or("").to_uppercase().as_str() {
        "BUFF" => "buff",
        "NERF" => "nerf",
        "MIXED" => "mixed",
        "BUGFIX" => "fix",
        _ => "upd",
    }
}

/// One heading line `## Title {#anchor}` as `<h2 id="anchor">Title</h2>`; `None` when the line is not a heading.
fn heading(line: &str) -> Option<String> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim().to_string();
    if rest.is_empty() {
        return None;
    }
    // the title is the shortest text followed by an optional ` {#anchor}` and the end of the line
    for (i, _) in rest.match_indices("{#") {
        let after = &rest[i + 2..];
        if i >= 1
            && after.ends_with('}')
            && !after[..after.len() - 1].contains('}')
            && after.len() > 1
        {
            let title = rest[..i].trim();
            let anchor = &after[..after.len() - 1];
            return Some(format!("<h{level} id=\"{anchor}\">{title}</h{level}>"));
        }
    }
    Some(format!("<h{level}>{rest}</h{level}>"))
}

/// `patchHeadings`: the notes are pre-rendered HTML except the headings, which stay markdown
/// (`## Section {#anchor}`); each one becomes a real `<hN id>` (the id feeds the table of contents). The result is
/// always sanitised before it is inserted.
pub fn patch_headings(raw: &str) -> String {
    raw.split('\n')
        .map(|l| heading(l).unwrap_or_else(|| l.to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A section of a patch about a hero (`GET /api/hero/:name/patches`, `GET /api/hero-changes`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HeroPatchSection {
    pub patch_internal_id: String,
    pub patch_name: Option<String>,
    pub live_date: Option<String>,
    pub anchor: String,
    pub hero_name: String,
    pub classification: Option<String>,
    pub short_summary: Option<String>,
    pub content: Option<String>,
}

/// `GET /api/hero-changes/heroes`: one row per hero.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HeroChangeSummary {
    pub hero_name: String,
    pub count: i64,
    pub latest_date: Option<String>,
    pub latest_class: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TocEntry {
    pub heading_level: i64,
    pub title: String,
    pub anchor: String,
    pub section_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PatchSection {
    pub section_type: String,
    pub anchor: String,
    pub classification: String,
}

/// `GET /api/patches/:id` (`null` when the patch is unknown).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PatchDetail {
    pub patch_name: String,
    pub patch_type: String,
    pub live_date: Option<String>,
    pub official_link: Option<String>,
    pub content: Option<String>,
    pub table_of_contents: Vec<TocEntry>,
    pub sections: Vec<PatchSection>,
}

/// Table of contents without the `Quick Navigation` entry (the page itself is the navigation).
pub fn toc(d: &PatchDetail) -> Vec<&TocEntry> {
    d.table_of_contents
        .iter()
        .filter(|t| t.section_type != "Section")
        .collect()
}

/// Classification (`BUFF`, `NERF`...) by anchor, from the sections about a hero.
pub fn hero_classes(d: &PatchDetail) -> HashMap<&str, &str> {
    d.sections
        .iter()
        .filter(|s| s.section_type == "Hero")
        .map(|s| (s.anchor.as_str(), s.classification.as_str()))
        .collect()
}

/// The classes of the Hero changes filter.
pub const CLASSES: [&str; 4] = ["BUFF", "NERF", "REWORK", "MIXED"];

/// Page size of the timeline.
pub const PAGE: usize = 60;

/// Heroes whose name contains `q` (case-insensitive); all of them when `q` is empty.
pub fn filter_heroes<'a>(rows: &'a [HeroChangeSummary], q: &str) -> Vec<&'a HeroChangeSummary> {
    let q = q.to_lowercase();
    rows.iter()
        .filter(|h| q.is_empty() || h.hero_name.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_become_real_tags_with_their_anchor() {
        assert_eq!(
            patch_headings(
                "## Abathur {#abathur-1}\n<p>x</p>\n### Plain  \n#nospace\n####### seven"
            ),
            "<h2 id=\"abathur-1\">Abathur</h2>\n<p>x</p>\n<h3>Plain</h3>\n#nospace\n####### seven"
        );
        assert_eq!(patch_headings("# {#a}"), "<h1>{#a}</h1>");
        assert_eq!(patch_headings("<p>no heading</p>"), "<p>no heading</p>");
        // an anchor with a closing brace inside is not one
        assert_eq!(patch_headings("## T {#a}b}"), "<h2>T {#a}b}</h2>");
    }

    #[test]
    fn the_pill_class_follows_the_classification() {
        assert_eq!(ptag_class(Some("buff")), "buff");
        assert_eq!(ptag_class(Some("BUGFIX")), "fix");
        assert_eq!(ptag_class(Some("REWORK")), "upd");
        assert_eq!(ptag_class(None), "upd");
    }

    #[test]
    fn the_table_of_contents_skips_the_quick_navigation() {
        let e = |t: &str, s: &str| TocEntry {
            title: t.into(),
            section_type: s.into(),
            ..Default::default()
        };
        let d = PatchDetail {
            table_of_contents: vec![e("Quick", "Section"), e("Abathur", "Hero")],
            sections: vec![PatchSection {
                section_type: "Hero".into(),
                anchor: "a".into(),
                classification: "NERF".into(),
            }],
            ..Default::default()
        };
        assert_eq!(toc(&d).len(), 1);
        assert_eq!(hero_classes(&d).get("a"), Some(&"NERF"));
    }

    #[test]
    fn heroes_are_filtered_by_a_fragment_of_the_name() {
        let h = |n: &str| HeroChangeSummary {
            hero_name: n.into(),
            ..Default::default()
        };
        let rows = [h("Abathur"), h("Li-Ming")];
        assert_eq!(filter_heroes(&rows, "").len(), 2);
        assert_eq!(filter_heroes(&rows, "ING")[0].hero_name, "Li-Ming");
    }
}
