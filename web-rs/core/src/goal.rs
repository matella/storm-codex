//! Daily goal tracker (`docs/specs/2026-11-20-streamer-widgets.md`, widget 7). Shared vocabulary and
//! pure presentation logic between the Admin "Daily goal" block (`app/src/pages/admin.rs`) and the
//! `/goal` OBS source (`sources/src/goal.rs`). No I/O; the actual progress is computed server-side
//! (`storm-codex-server::goal`) and read here as plain JSON from `GET /api/goal/progress`.

use serde::Deserialize;

/// The two goal kinds the operator can pick. Mirrors `storm-codex-server::goal::GoalKind`'s wire
/// strings (`"wins_today"` / `"games_today"`); kept as a plain string on the wire rather than shared
/// code, since the server and `web-rs` are separate Cargo workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalKind {
    WinsToday,
    GamesToday,
}

impl GoalKind {
    pub const ALL: [GoalKind; 2] = [GoalKind::WinsToday, GoalKind::GamesToday];

    pub fn as_str(self) -> &'static str {
        match self {
            GoalKind::WinsToday => "wins_today",
            GoalKind::GamesToday => "games_today",
        }
    }

    /// Admin UI label for the kind selector.
    pub fn label(self) -> &'static str {
        match self {
            GoalKind::WinsToday => "wins today",
            GoalKind::GamesToday => "games played today",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wins_today" => Some(GoalKind::WinsToday),
            "games_today" => Some(GoalKind::GamesToday),
            _ => None,
        }
    }
}

/// `GET /api/goal/progress` response.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub kind: Option<String>,
    pub target: Option<i64>,
    pub done: i64,
    pub reset_at: Option<String>,
}

/// `true` when there is an active goal to render; the `/goal` source draws a blank transparent frame
/// (no placeholder message) otherwise, since an empty OBS source should not draw a visible box.
pub fn is_active(p: &Progress) -> bool {
    p.kind.is_some() && p.target.is_some()
}

/// The display line for the `/goal` source, e.g. `"2 / 3 wins today"`. `None` when there is nothing
/// to show (no goal set, or the server returned a kind this build does not know).
pub fn display_line(p: &Progress) -> Option<String> {
    let kind = GoalKind::parse(p.kind.as_deref()?)?;
    let target = p.target?;
    Some(format!("{} / {} {}", p.done, target, kind.label()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── no-goal state ────────────────────────────────────────────────────────

    #[test]
    fn no_goal_is_inactive_and_has_no_display_line() {
        let p = Progress::default();
        assert!(!is_active(&p));
        assert_eq!(display_line(&p), None);
    }

    #[test]
    fn a_kind_without_a_target_is_not_active_either() {
        // defensive: the server never sends this shape, but the client should not render on it
        let p = Progress {
            kind: Some("wins_today".into()),
            target: None,
            done: 0,
            reset_at: None,
        };
        assert!(!is_active(&p));
    }

    // ── kind switch ──────────────────────────────────────────────────────────

    #[test]
    fn wins_today_label_and_display_line() {
        let p = Progress {
            kind: Some("wins_today".into()),
            target: Some(3),
            done: 2,
            reset_at: Some("2026-11-21T00:00:00Z".into()),
        };
        assert!(is_active(&p));
        assert_eq!(display_line(&p), Some("2 / 3 wins today".to_string()));
    }

    #[test]
    fn games_today_label_and_display_line() {
        let p = Progress {
            kind: Some("games_today".into()),
            target: Some(5),
            done: 5,
            reset_at: Some("2026-11-21T00:00:00Z".into()),
        };
        assert_eq!(
            display_line(&p),
            Some("5 / 5 games played today".to_string())
        );
    }

    #[test]
    fn unknown_kind_from_a_newer_server_is_treated_as_no_goal() {
        let p = Progress {
            kind: Some("games_this_week".into()),
            target: Some(10),
            done: 1,
            reset_at: Some("2026-11-27T00:00:00Z".into()),
        };
        assert_eq!(display_line(&p), None, "unknown kinds never render");
    }

    // ── midnight reset (client side: the source trusts the server's `reset_at`, it never computes
    // its own boundary — this only checks the field round-trips through deserialization) ──────────

    #[test]
    fn reset_at_round_trips_from_json() {
        let v: Progress = serde_json::from_str(
            r#"{"kind":"wins_today","target":3,"done":1,"reset_at":"2026-11-21T00:00:00+01:00"}"#,
        )
        .unwrap();
        assert_eq!(v.reset_at.as_deref(), Some("2026-11-21T00:00:00+01:00"));
        assert!(is_active(&v));
    }

    #[test]
    fn missing_fields_deserialize_to_the_no_goal_default() {
        let v: Progress = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(v, Progress::default());
        assert!(!is_active(&v));
    }

    #[test]
    fn parse_and_as_str_round_trip_for_every_kind() {
        for k in GoalKind::ALL {
            assert_eq!(GoalKind::parse(k.as_str()), Some(k));
        }
    }
}
