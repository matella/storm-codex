//! Pack LLM au format Markdown (spec volet D) — format principal, lisible par tous les modèles y
//! compris locaux : prompt, joueurs, faits en tableaux (colonne `id` à citer), parties, format de
//! réponse imposé. Module pur.

use super::facts::{ordered_index, Facts};

pub const PROMPT: &str = include_str!("prompt.md");
pub const SCHEMA: &str = include_str!("analysis.schema.json");

pub struct PackMeta<'a> {
    pub report_id: i64,
    pub facts_version: i32,
    pub title: &'a str,
    pub target_name: Option<&'a str>,
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn fmt_len(s: f64) -> String {
    format!("{}:{:02}", (s / 60.0) as u32, (s % 60.0) as u32)
}

/// Section d'un id de fait : `p3.hero.x` → joueurs, `map.x.y` → cartes, etc.
fn section(id: &str) -> &'static str {
    let head = id.split('.').next().unwrap_or("");
    match head {
        "overview" => "Overview",
        "map" => "Maps",
        "draft" => "Draft",
        "flow" => "Game flow",
        "game" => "Games",
        h if h.starts_with('p') && h[1..].chars().all(|c| c.is_ascii_digit()) => "Players",
        _ => "Other",
    }
}

fn example(meta: &PackMeta, f: &Facts) -> String {
    let (map, map_id) = f.maps.first().map_or(("<map>".to_string(), "map.<map>.record".to_string()), |m| {
        (m.map.clone(), format!("{}.record", m.id))
    });
    serde_json::to_string_pretty(&serde_json::json!({
        "format_version": crate::scouting::analysis::FORMAT_VERSION,
        "report_id": meta.report_id,
        "facts_version": meta.facts_version,
        "model": "<your model name>",
        "summary": "<3-5 sentences: their draft identity>",
        "map_choice": {
            "pick": [{ "map": "<map to choose against them>", "why": "<reason>", "evidence": ["<fact id>"] }],
            "avoid": [{ "map": "<map to avoid>", "why": "<reason>", "evidence": ["<fact id>"] }]
        },
        "maps": [{
            "map": map,
            "confidence": "low",
            "overview": "<what to expect from them on this map>",
            "evidence": [map_id],
            "bans": [{ "hero": "<hero>", "phase": "first", "why": "<reason>", "evidence": ["<fact id>"] }],
            "picks": [{ "hero": "<hero for us>", "why": "<reason>", "evidence": ["<fact id>"] }],
            "their_picks": [{ "hero": "<hero>", "player": "p1", "why": "<reason>", "evidence": ["<fact id>"] }],
            "considerations": [{ "point": "<draft consideration>", "evidence": ["<fact id>"] }]
        }],
        "general": {
            "bans": [{ "hero": "<hero>", "phase": "mid", "why": "<reason>", "evidence": ["<fact id>"] }],
            "picks": [{ "hero": "<hero>", "why": "<reason>", "evidence": ["<fact id>"] }],
            "considerations": [{ "point": "<applies on any map>", "evidence": ["<fact id>"] }]
        }
    }))
    .unwrap_or_default()
}

pub fn markdown(meta: &PackMeta, f: &Facts) -> String {
    let mut out = String::new();
    let push = |out: &mut String, s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    push(&mut out, &format!("# Scouting pack — {}", meta.title));
    push(&mut out, "");
    push(
        &mut out,
        &format!(
            "report_id: **{}** · facts_version: **{}** · target team: **{}** · games analysed: **{}**{}",
            meta.report_id,
            meta.facts_version,
            meta.target_name.unwrap_or("(unnamed)"),
            f.overview.games,
            if f.overview.excluded > 0 {
                format!(" ({} replay(s) excluded: target team not found)", f.overview.excluded)
            } else {
                String::new()
            }
        ),
    );
    push(&mut out, "");
    push(&mut out, "## Instructions");
    push(&mut out, "");
    push(&mut out, PROMPT.trim());
    push(&mut out, "");

    push(&mut out, "## Players of the target team");
    push(&mut out, "");
    push(&mut out, "| id | name | status | games |");
    push(&mut out, "|---|---|---|---|");
    for p in &f.players {
        push(
            &mut out,
            &format!(
                "| {} | {} | {} | {} |",
                p.pid,
                cell(&p.name),
                if p.core { "main roster" } else { "substitute" },
                p.record.n
            ),
        );
    }
    push(&mut out, "");

    push(&mut out, "## Facts");
    push(&mut out, "");
    push(&mut out, "Cite the `id` column in your `evidence` arrays. `n` = number of games behind the fact.");
    let mut current = "";
    for (id, r) in ordered_index(f) {
        let sec = section(&id);
        if sec == "Games" {
            continue; // détaillées dans la table des parties
        }
        if sec != current {
            current = sec;
            push(&mut out, "");
            push(&mut out, &format!("### {sec}"));
            push(&mut out, "");
            push(&mut out, "| id | fact | value | n |");
            push(&mut out, "|---|---|---|---|");
        }
        let low = if r.low { " — LOW SAMPLE" } else { "" };
        push(
            &mut out,
            &format!("| `{id}` | {} | {}{low} | {} |", cell(&r.label), cell(&r.text), r.n),
        );
    }
    push(&mut out, "");

    push(&mut out, "## Games");
    push(&mut out, "");
    push(&mut out, "Picks are listed in the team's own pick order; players are given by id (`p1`…) for the target team.");
    push(&mut out, "");
    push(
        &mut out,
        "| id | date | map | result | length | first pick | their bans | bans against them | their picks | opponent picks |",
    );
    push(&mut out, "|---|---|---|---|---|---|---|---|---|---|");
    for g in &f.games {
        let picks = |ps: &[super::facts::GamePick]| {
            ps.iter().map(|p| format!("{} ({})", p.hero, p.player)).collect::<Vec<_>>().join(", ")
        };
        push(
            &mut out,
            &format!(
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                g.id,
                g.date.as_deref().and_then(|d| d.get(..10)).unwrap_or("?"),
                cell(&g.map),
                if g.won { "win" } else { "loss" },
                fmt_len(g.length_s),
                match g.first_pick {
                    Some(true) => "them",
                    Some(false) => "opponent",
                    None => "?",
                },
                cell(&g.bans.join(", ")),
                cell(&g.bans_against.join(", ")),
                cell(&picks(&g.picks)),
                cell(&picks(&g.opp_picks)),
            ),
        );
    }
    push(&mut out, "");

    push(&mut out, "## Response format");
    push(&mut out, "");
    push(&mut out, "Answer with ONE ```json code block containing an object like this example — one `maps` entry per map present in the facts (required fields: `format_version`, `report_id`, `facts_version`, `summary`, `maps`):");
    push(&mut out, "");
    push(&mut out, "```json");
    push(&mut out, &example(meta, f));
    push(&mut out, "```");
    push(&mut out, "");
    push(&mut out, "JSON Schema of the response:");
    push(&mut out, "");
    push(&mut out, "```json");
    push(&mut out, SCHEMA.trim());
    push(&mut out, "```");
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::scouting::facts::{compute, GameInput};
    use crate::scouting::side::tests::game;
    use std::collections::HashMap;

    #[test]
    fn le_pack_cite_chaque_fait_et_les_identifiants_du_rapport() {
        let g = game(&["r1", "r2", "r3", "r4", "r5"], &["o1", "o2", "o3", "o4", "o5"]);
        let gi = [GameInput { gid: 42, summary: &g, side: 0 }];
        let f = compute(&gi, &[], 0, &HashMap::new());
        let md = markdown(&PackMeta { report_id: 9, facts_version: 4, title: "Scout", target_name: Some("Team X") }, &f);
        for id in f.index.keys() {
            assert!(md.contains(&format!("`{id}`")), "id absent du pack : {id}");
        }
        assert!(md.contains("report_id: **9**"));
        assert!(md.contains("\"facts_version\": 4"));
        assert!(md.contains("\"format_version\": 2"));
        // ordre orienté draft : cartes et draft avant les joueurs
        let (maps, draft, players) = (md.find("### Maps").unwrap(), md.find("### Draft").unwrap(), md.find("### Players").unwrap());
        assert!(maps < draft && draft < players);
        assert!(md.contains("LOW SAMPLE"));
        assert!(md.contains("```json"));
        // l'exemple embarqué est lui-même importable
        let ex = example(&PackMeta { report_id: 9, facts_version: 4, title: "", target_name: None }, &f);
        assert!(crate::scouting::analysis::import(&ex, 9, 4).is_ok());
    }
}
