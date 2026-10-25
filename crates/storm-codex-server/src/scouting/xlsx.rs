//! Pack au format Excel (spec volet D) : les mêmes faits que le Markdown, un onglet par famille,
//! pour la lecture humaine et les LLM qui lisent l'Excel. Module pur (écrit en mémoire).

use super::facts::{ordered_index, Count, Facts, GamePick, Rate};
use super::pack::PackMeta;
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};

fn pct(r: Rate) -> f64 {
    r.pct().map_or(0.0, |p| (p * 10.0).round() / 10.0)
}

fn header(ws: &mut Worksheet, cols: &[&str], bold: &Format) -> Result<(), XlsxError> {
    for (i, c) in cols.iter().enumerate() {
        ws.write_with_format(0, i as u16, *c, bold)?;
        ws.set_column_width(i as u16, (c.len() as f64 + 4.0).max(12.0))?;
    }
    ws.set_freeze_panes(1, 0)?;
    Ok(())
}

fn picks(ps: &[GamePick]) -> String {
    ps.iter().map(|p| format!("{} ({})", p.hero, p.player)).collect::<Vec<_>>().join(", ")
}

pub fn workbook(meta: &PackMeta, f: &Facts) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();

    // README
    let ws = wb.add_worksheet().set_name("README")?;
    ws.set_column_width(0, 110)?;
    let lines: Vec<String> = vec![
        format!("Scouting pack — {}", meta.title),
        format!(
            "report_id {} · facts_version {} · target team {} · {} games analysed, {} excluded (target team not found)",
            meta.report_id,
            meta.facts_version,
            meta.target_name.unwrap_or("(unnamed)"),
            f.overview.games,
            f.overview.excluded
        ),
        String::new(),
        "All figures are computed by Storm Codex from the replays; they are seen from the TARGET team.".into(),
        "Rates are 'k/n (%)'; n = number of games behind a fact; 'low sample' = fewer than 3 games.".into(),
        "Draft facts are by phase (first-phase bans, mid-draft ban, their first/last pick), not by global draft position.".into(),
        String::new(),
        "Sheets: Facts (every citable fact with its id), Players, Heroes, Maps, Draft, Games.".into(),
        "For an LLM analysis, prefer the Markdown pack: it contains the instructions and the required response format.".into(),
    ];
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            ws.write_with_format(i as u32, 0, l.as_str(), &bold)?;
        } else {
            ws.write(i as u32, 0, l.as_str())?;
        }
    }

    // Facts
    let ws = wb.add_worksheet().set_name("Facts")?;
    header(ws, &["id", "fact", "value", "n", "low sample"], &bold)?;
    ws.set_column_width(0, 34)?;
    ws.set_column_width(1, 60)?;
    ws.set_column_width(2, 50)?;
    for (i, (id, r)) in ordered_index(f).iter().enumerate() {
        let row = i as u32 + 1;
        ws.write(row, 0, id.as_str())?;
        ws.write(row, 1, r.label.as_str())?;
        ws.write(row, 2, r.text.as_str())?;
        ws.write(row, 3, r.n)?;
        ws.write(row, 4, if r.low { "yes" } else { "" })?;
    }

    // Players
    let ws = wb.add_worksheet().set_name("Players")?;
    header(
        ws,
        &[
            "id", "name", "status", "games", "wins", "win %", "kills", "deaths", "assists", "KP %",
            "hero dmg/min", "siege/min", "healing/min", "dmg taken/min", "XP/min", "time dead %",
        ],
        &bold,
    )?;
    for (i, p) in f.players.iter().enumerate() {
        let row = i as u32 + 1;
        let s = &p.stats;
        ws.write(row, 0, p.pid.as_str())?;
        ws.write(row, 1, p.name.as_str())?;
        ws.write(row, 2, if p.core { "main roster" } else { "substitute" })?;
        ws.write(row, 3, p.record.n)?;
        ws.write(row, 4, p.record.k)?;
        ws.write(row, 5, pct(p.record))?;
        let nums = [
            s.kills, s.deaths, s.assists, s.kill_participation_pct, s.hero_damage_pm,
            s.siege_damage_pm, s.healing_pm, s.damage_taken_pm, s.xp_pm, s.time_dead_pct,
        ];
        for (j, v) in nums.iter().enumerate() {
            ws.write(row, 6 + j as u16, (v * 10.0).round() / 10.0)?;
        }
    }

    // Heroes
    let ws = wb.add_worksheet().set_name("Heroes")?;
    header(ws, &["player id", "player", "hero", "games", "wins", "win %"], &bold)?;
    let mut row = 1;
    for p in &f.players {
        for h in &p.heroes {
            ws.write(row, 0, p.pid.as_str())?;
            ws.write(row, 1, p.name.as_str())?;
            ws.write(row, 2, h.hero.as_str())?;
            ws.write(row, 3, h.record.n)?;
            ws.write(row, 4, h.record.k)?;
            ws.write(row, 5, pct(h.record))?;
            row += 1;
        }
    }

    // Maps
    let ws = wb.add_worksheet().set_name("Maps")?;
    header(ws, &["map", "games", "wins", "win %", "95% CI low", "95% CI high", "their bans", "bans against them", "their picks"], &bold)?;
    let list = |cs: &[Count]| cs.iter().map(|c| format!("{} ×{}", c.key, c.count.k)).collect::<Vec<_>>().join(", ");
    for (i, m) in f.maps.iter().enumerate() {
        let row = i as u32 + 1;
        ws.write(row, 0, m.map.as_str())?;
        ws.write(row, 1, m.record.n)?;
        ws.write(row, 2, m.record.k)?;
        ws.write(row, 3, pct(m.record))?;
        if let Some((lo, hi)) = m.wilson {
            ws.write(row, 4, lo.round())?;
            ws.write(row, 5, hi.round())?;
        }
        ws.write(row, 6, list(&m.bans))?;
        ws.write(row, 7, list(&m.bans_against))?;
        let pk = m.picks.iter().map(|h| format!("{} ×{}", h.hero, h.picks.k)).collect::<Vec<_>>().join(", ");
        ws.write(row, 8, pk)?;
    }

    // Draft
    let ws = wb.add_worksheet().set_name("Draft")?;
    header(ws, &["category", "hero / role", "count", "out of games", "%"], &bold)?;
    let d = &f.draft;
    let mut row = 1;
    for (cat, r) in [
        ("had first pick", d.first_pick),
        ("record with first pick", d.first_pick_record),
        ("record with second pick", d.second_pick_record),
    ] {
        ws.write(row, 0, cat)?;
        ws.write(row, 2, r.k)?;
        ws.write(row, 3, r.n)?;
        ws.write(row, 4, pct(r))?;
        row += 1;
    }
    for (cat, cs) in [
        ("first-phase ban", &d.bans_first),
        ("mid-draft ban", &d.bans_mid),
        ("banned against them", &d.bans_against),
        ("their first pick", &d.openers),
        ("role of their first pick", &d.opener_roles),
        ("their last pick", &d.last_picks),
    ] {
        for c in cs {
            ws.write(row, 0, cat)?;
            ws.write(row, 1, c.key.as_str())?;
            ws.write(row, 2, c.count.k)?;
            ws.write(row, 3, c.count.n)?;
            ws.write(row, 4, pct(c.count))?;
            row += 1;
        }
    }

    // Games
    let ws = wb.add_worksheet().set_name("Games")?;
    header(
        ws,
        &["date", "map", "result", "length (min)", "first pick", "their bans", "bans against them", "their picks", "opponent picks", "opponents"],
        &bold,
    )?;
    for (i, g) in f.games.iter().enumerate() {
        let row = i as u32 + 1;
        ws.write(row, 0, g.date.as_deref().and_then(|d| d.get(..16)).unwrap_or("?").replace('T', " "))?;
        ws.write(row, 1, g.map.as_str())?;
        ws.write(row, 2, if g.won { "win" } else { "loss" })?;
        ws.write(row, 3, (g.length_s / 6.0).round() / 10.0)?;
        ws.write(row, 4, match g.first_pick {
            Some(true) => "them",
            Some(false) => "opponent",
            None => "?",
        })?;
        ws.write(row, 5, g.bans.join(", "))?;
        ws.write(row, 6, g.bans_against.join(", "))?;
        ws.write(row, 7, picks(&g.picks))?;
        ws.write(row, 8, picks(&g.opp_picks))?;
        ws.write(row, 9, g.opponents.join(", "))?;
    }

    wb.save_to_buffer()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::scouting::facts::{compute, GameInput};
    use crate::scouting::side::tests::game;
    use std::collections::HashMap;

    #[test]
    fn classeur_genere() {
        let g = game(&["r1", "r2", "r3", "r4", "r5"], &["o1", "o2", "o3", "o4", "o5"]);
        let gi = [GameInput { gid: 1, summary: &g, side: 0 }];
        let f = compute(&gi, &[], 0, &HashMap::new());
        let bytes = workbook(&PackMeta { report_id: 1, facts_version: 1, title: "t", target_name: None }, &f).unwrap();
        assert!(bytes.len() > 1000);
        assert_eq!(&bytes[..2], b"PK"); // .xlsx = archive ZIP
    }
}
