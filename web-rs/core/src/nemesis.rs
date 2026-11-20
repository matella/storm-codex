//! Nemesis/grudge-match badge (`/nemesis` OBS source, spec `docs/specs/2026-11-20-streamer-widgets.md`
//! item 5): pure opponent resolution and the grudge threshold. No I/O: the lobby state and the
//! per-opponent record come from the caller.
//!
//! Opponent toons are resolved from `GET /api/lobby` (the loading-screen companion state, already
//! enriched with `toon_handle` per player for the operator's own build suggestions), not
//! `GET /api/live` — the live feed's view deliberately never serialises toon handles (fair-play
//! design, `live/state.rs`), so it cannot name an opponent. `/api/lobby` already resolves the whole
//! roster during Draft/loading, which is what this badge is shown over.

use serde::Deserialize;

/// One player of `GET /api/lobby` (`LobbyPlayerState` on the server), only what the badge needs.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LobbyPlayer {
    pub team: Option<u8>,
    pub toon_handle: Option<String>,
}

/// `GET /api/lobby` (`LobbyState` on the server), only what the badge needs.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Index of the operator in `players`.
    pub me: Option<usize>,
    pub content_hash: String,
}

/// Resolved, de-duplicated opponent toons, sorted. Empty until the operator's slot and at least
/// one opponent's team *and* toon are known, so a lobby mid-detection (teams not deduced yet, or a
/// player not yet matched to the archive) never names the wrong player or guesses at a badge.
#[must_use]
pub fn enemy_toons(lobby: &Lobby) -> Vec<String> {
    let Some(my_team) = lobby
        .me
        .and_then(|i| lobby.players.get(i))
        .and_then(|p| p.team)
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = lobby
        .players
        .iter()
        .filter(|p| p.team.is_some_and(|t| t != my_team))
        .filter_map(|p| p.toon_handle.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Stable signature of "this lobby's resolved opponents": empty while nothing is resolved yet,
/// else `content_hash` (new lobby) combined with the resolved toons (new names resolved in the
/// same lobby, e.g. after the archive catches up). Comparing this against the last checked
/// signature is what debounces the per-opponent lookups to once per lobby instead of once per
/// `lobby.updated` event.
#[must_use]
pub fn lobby_signature(lobby: &Lobby) -> String {
    let toons = enemy_toons(lobby);
    if toons.is_empty() {
        String::new()
    } else {
        format!("{}|{}", lobby.content_hash, toons.join(","))
    }
}

/// A "grudge match": the operator's record against them is <= 35% over >= 3 games.
#[must_use]
pub fn is_grudge(wins: u32, games: u32) -> bool {
    games >= 3 && 100 * wins <= 35 * games
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn player(team: Option<u8>, toon: Option<&str>) -> LobbyPlayer {
        LobbyPlayer {
            team,
            toon_handle: toon.map(str::to_owned),
        }
    }

    #[test]
    fn enemy_toons_needs_my_team_known() {
        let lobby = Lobby {
            players: vec![player(None, Some("me")), player(Some(1), Some("foe"))],
            me: Some(0),
            content_hash: "h".into(),
        };
        assert_eq!(enemy_toons(&lobby), Vec::<String>::new());
    }

    #[test]
    fn enemy_toons_skips_unresolved_toons_and_allies() {
        let lobby = Lobby {
            players: vec![
                player(Some(0), Some("me")),
                player(Some(0), Some("ally")),
                player(Some(1), Some("foe-b")),
                player(Some(1), None), // enemy team, toon not yet resolved: skipped
                player(Some(1), Some("foe-a")),
            ],
            me: Some(0),
            content_hash: "h".into(),
        };
        assert_eq!(enemy_toons(&lobby), vec!["foe-a", "foe-b"]);
    }

    #[test]
    fn enemy_toons_dedupes() {
        let lobby = Lobby {
            players: vec![
                player(Some(0), Some("me")),
                player(Some(1), Some("foe")),
                player(Some(1), Some("foe")),
            ],
            me: Some(0),
            content_hash: "h".into(),
        };
        assert_eq!(enemy_toons(&lobby), vec!["foe"]);
    }

    #[test]
    fn signature_empty_until_an_opponent_is_resolved() {
        let lobby = Lobby {
            players: vec![player(Some(0), Some("me")), player(Some(1), None)],
            me: Some(0),
            content_hash: "h".into(),
        };
        assert_eq!(lobby_signature(&lobby), "");
    }

    #[test]
    fn signature_changes_with_lobby_or_roster() {
        let a = Lobby {
            players: vec![player(Some(0), Some("me")), player(Some(1), Some("foe"))],
            me: Some(0),
            content_hash: "h1".into(),
        };
        let mut b = Lobby {
            content_hash: "h2".into(),
            ..a.clone()
        };
        assert_ne!(lobby_signature(&a), lobby_signature(&b));
        b.content_hash = a.content_hash.clone();
        assert_eq!(lobby_signature(&a), lobby_signature(&b));
    }

    #[test]
    fn grudge_threshold() {
        assert!(is_grudge(1, 3)); // 33%
        assert!(!is_grudge(2, 3)); // 67%
        assert!(is_grudge(0, 10));
        assert!(!is_grudge(0, 2)); // under 3 games: not enough sample
        assert!(is_grudge(7, 20)); // exactly 35%
        assert!(!is_grudge(8, 20)); // 40%
    }
}
