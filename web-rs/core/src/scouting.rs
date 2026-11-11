//! Scouting reports: list types and helpers. Port of the list part of `web/src/scouting.ts`.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    #[default]
    Empty,
    Ready,
    Analyzed,
    Stale,
}

impl ReportStatus {
    pub fn class(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Ready => "ready",
            Self::Analyzed => "analyzed",
            Self::Stale => "stale",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "no replays",
            Self::Ready => "pack ready",
            Self::Analyzed => "draft plan ready",
            Self::Stale => "plan outdated",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct Rate {
    pub k: i64,
    pub n: i64,
}

/// One line of `GET /api/scouting`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScoutingListItem {
    pub id: i64,
    pub title: String,
    pub target_name: Option<String>,
    pub created_at: String,
    pub games: i64,
    pub record: Option<Rate>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub status: ReportStatus,
}

/// `day`: the date part of an ISO timestamp, `?` when missing.
pub fn day(iso: Option<&str>) -> String {
    iso.map_or("?".to_string(), |s| s.chars().take(10).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_cuts_the_iso_date_and_defaults_to_a_question_mark() {
        assert_eq!(day(Some("2026-11-02T10:00:00Z")), "2026-11-02");
        assert_eq!(day(None), "?");
    }

    #[test]
    fn list_items_decode_with_a_lowercase_status() {
        let v: ScoutingListItem = serde_json::from_str(
            r#"{"id":3,"title":"W4","target_name":null,"created_at":"2026-11-02T10:00:00Z","games":2,"record":{"k":1,"n":2},"first_date":null,"last_date":null,"status":"stale"}"#,
        )
        .unwrap();
        assert_eq!(v.status.label(), "plan outdated");
        assert_eq!(v.record, Some(Rate { k: 1, n: 2 }));
    }
}
