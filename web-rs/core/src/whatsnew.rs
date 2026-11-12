//! Product changelog of Storm Codex (not the HotS patch notes). `version` grows with the date; the "What's new"
//! panel shows the entries newer than the last seen one. Add an entry at the top for each batch of features.

pub const APP_VERSION: &str = "2026.09.26";

pub struct Entry {
    pub version: &'static str,
    pub title: &'static str,
    pub items: &'static [&'static str],
}

pub const CHANGELOG: [Entry; 2] = [
    Entry {
        version: "2026.09.26",
        title: "Scouting reports",
        items: &[
            "New Scouting page: drop the replays of an opposing team and its trends come out as numbers (draft, maps, players, flow).",
            "LLM pack to copy (Markdown) or download (.md / .xlsx) for ChatGPT, Claude or a local model.",
            "Import of the LLM analysis: each claim shows the numbers behind it, claims without evidence are flagged.",
            "Anchor player to designate the scouted team when the replays are not enough (series against the same opponent).",
            "Draft plan map by map (bans, picks, their likely picks) and a stand-alone HTML export to send to the team.",
        ],
    },
    Entry {
        version: "2026.06.16",
        title: "Patch notes, synergies & timeline",
        items: &[
            "Built-in Patch Notes (list + detail) with a notification for a new patch.",
            "Detailed hero page (by mode), richer player page, Synergies page.",
            "Match timeline: kills / structures / objectives + a pin track.",
            "Rich filters + text search + persistent filters on Matches.",
            "Extended stats, team totals, XP curve, awards by type (emoji).",
        ],
    },
];

/// Entries to show: all of them when forced from the `?` button, else the ones newer than `seen`.
pub fn entries_since(seen: &str, force: bool) -> Vec<&'static Entry> {
    CHANGELOG
        .iter()
        .filter(|c| force || c.version > seen)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_newer_entries_are_shown_unless_forced() {
        assert_eq!(entries_since("", false).len(), 2);
        assert_eq!(entries_since("2026.06.16", false).len(), 1);
        assert!(entries_since(APP_VERSION, false).is_empty());
        assert_eq!(entries_since(APP_VERSION, true).len(), 2);
    }

    #[test]
    fn the_newest_entry_is_the_app_version() {
        assert_eq!(CHANGELOG[0].version, APP_VERSION);
    }
}
