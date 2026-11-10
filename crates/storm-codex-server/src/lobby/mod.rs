//! Live lobby: current state of a game being loaded. Persisted singleton (modelled on
//! `draft_live`) — the current lobby overwrites the previous one, there is no lobby history: the
//! archived replay stays the source of truth (tier 1 of the three data tiers).
pub mod api;
pub mod enrich;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

/// Schema version of `lobby_live.state` — a state of an unknown version is ignored at
/// startup rather than deserialised wrongly.
pub const LOBBY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayerState {
    pub name: String,
    pub discriminant: String,
    /// `"name#1234"` — the identity key from the blob.
    pub battletag: String,
    /// Team: the one deduced by `storm-lobby`, or the one set by hand (task 5).
    pub team: Option<u8>,
    /// `true` if the team was corrected by hand — the front shows it differently.
    #[serde(default)]
    pub team_manual: bool,
    /// Application identity, resolved against the archive. `None` = player never met.
    pub toon_handle: Option<String>,
    /// Archive aggregates (task 4). `null` as long as not enriched or if the player is unknown.
    #[serde(default)]
    pub history: Option<J>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyState {
    pub schema_version: u32,
    pub detected_at: String,
    pub players: Vec<LobbyPlayerState>,
    /// Map: deduced from the `.s2ma` hashes, or entered by hand.
    pub map: Option<String>,
    #[serde(default)]
    pub map_manual: bool,
    /// Operator hero — never in the blob, always entered (task 5).
    pub hero: Option<String>,
    /// Index in `players` of the player identified as the operator, if found.
    pub me: Option<usize>,
    /// Suggested build + alternatives (task 4).
    #[serde(default)]
    pub build: Option<J>,
    /// Operator stats on this hero / this map (task 4).
    #[serde(default)]
    pub me_stats: Option<J>,
    /// Filled when the matching replay is parsed (task 6) → switches to debrief.
    #[serde(default)]
    pub match_id: Option<i64>,
    /// `parse_failed` when the blob is unreadable: the page then shows the manual selector
    /// rather than an empty page.
    #[serde(default)]
    pub status: Option<String>,
    /// Hexadecimal SHA-256 of the raw bytes received by `POST /api/lobby` (before decoding) —
    /// computed with `crate::upload::sha256_hex`, the same helper as the replay upload. Serves as
    /// first-level idempotence guard in `lobby::api::ingest`: a hash identical to
    /// that of the current state is *always* a repost of the same file, including when the state
    /// is already linked to a match (`match_id.is_some()`) — that is what protects the debrief from a
    /// simple restart of the watcher that re-posts the `replay.server.battlelobby` left on the
    /// disk. `#[serde(default)]`: a state persisted by an earlier version of the server does not have
    /// this field and must stay deserialisable (it is then `""`, which collides with
    /// no real hash).
    #[serde(default)]
    pub content_hash: String,
}

impl LobbyState {
    /// Builds the raw state from a decoded lobby, before enrichment.
    #[must_use]
    pub fn from_lobby(lobby: &storm_lobby::Lobby, detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: lobby
                .players
                .iter()
                .map(|p| LobbyPlayerState {
                    name: p.name.clone(),
                    discriminant: p.discriminant.clone(),
                    battletag: p.battletag(),
                    team: p.team,
                    team_manual: false,
                    toon_handle: None,
                    history: None,
                })
                .collect(),
            map: lobby.map.clone(),
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: None,
            content_hash: String::new(),
        }
    }

    /// Degraded state when the blob could not be decoded (new Blizzard build). The page stays
    /// usable: the operator enters their hero, the build is shown.
    #[must_use]
    pub fn unreadable(detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: Vec::new(),
            map: None,
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: Some("parse_failed".into()),
            content_hash: String::new(),
        }
    }
}

/// Window beyond which an open lobby no longer links to a newly parsed match.
/// Very far beyond the duration of a game (20-30 min) plus the upload time, so no
/// legitimate link is ever lost because of this limit; short enough that a lobby
/// left open by mistake (cancelled game, replay never uploaded) does not capture, the next day or
/// later, a later game between the same ten players — in leagues and scrims the same
/// line-up meets again regularly, and comparing BattleTags alone cannot then
/// tell the two games apart.
const FENETRE_LIAISON_HEURES: i64 = 6;

/// True if `detected_at` is more than [`FENETRE_LIAISON_HEURES`] old. Extracted from `lier_match` as a
/// pure function to be testable without a Postgres pool. Same pattern as `parse_failed_transitoire`
/// in `api.rs`: an unreadable date must never block an otherwise valid link — we
/// prefer losing the time protection on this degenerate case rather than the feature.
fn lobby_trop_ancien(detected_at: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(detected_at) {
        Ok(detecte) => {
            chrono::Utc::now().signed_duration_since(detecte)
                > chrono::Duration::hours(FENETRE_LIAISON_HEURES)
        }
        Err(_) => false,
    }
}

/// Links a freshly parsed match to the current lobby, if it is the same game. Criterion: the set
/// of BattleTags, and the freshness of the lobby (see [`lobby_trop_ancien`]). The full parse
/// rebuilds the same `name#tag` from the blob embedded in the replay (`storm_stats`,
/// `get_battletags`), so both sides carry the same key — without assuming anything about the binary
/// format. Returns `true` if the link happened (the caller then knows it must persist and
/// diffuser).
pub async fn lier_match(db: &sqlx::PgPool, state: &mut LobbyState, match_id: i64) -> bool {
    if state.match_id.is_some() || state.players.is_empty() {
        return false;
    }
    if lobby_trop_ancien(&state.detected_at) {
        return false;
    }
    let tags_match: Vec<String> = sqlx::query_scalar(
        "SELECT name || '#' || (data ->> 'tag')
         FROM match_players
         WHERE match_id = $1 AND name IS NOT NULL AND data ->> 'tag' IS NOT NULL",
    )
    .bind(match_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let lobby_tags: Vec<String> = state.players.iter().map(|p| p.battletag.clone()).collect();
    if !memes_battletags(&tags_match, &lobby_tags) {
        return false;
    }
    state.match_id = Some(match_id);
    true
}

/// Core of `lier_match`, extracted as a pure function to be testable without a Postgres pool (same
/// pattern as `meme_lobby` in `api.rs`): two sets of BattleTags describe the same game
/// if they contain exactly the same tags, regardless of order. An empty set is
/// never considered as matching itself — without players, the comparison proves nothing.
fn memes_battletags(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return false;
    }
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

#[cfg(test)]
mod tests {
    use super::{lobby_trop_ancien, memes_battletags};

    fn tags(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn memes_tags_dans_un_ordre_different_sont_le_meme_ensemble() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["c#3", "a#1", "b#2"]);
        assert!(memes_battletags(&a, &b));
    }

    #[test]
    fn un_tag_different_donne_des_ensembles_differents() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["a#1", "b#2", "quelqu_un_d_autre#9"]);
        assert!(!memes_battletags(&a, &b));
    }

    #[test]
    fn des_tailles_differentes_ne_correspondent_jamais() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["a#1", "b#2"]);
        assert!(!memes_battletags(&a, &b));
    }

    #[test]
    fn deux_ensembles_vides_ne_correspondent_jamais() {
        let a: Vec<String> = Vec::new();
        let b: Vec<String> = Vec::new();
        assert!(!memes_battletags(&a, &b));
    }

    /// Pins down the multiset comparison: same elements, same lengths, different
    /// multiplicities ("a" appears twice on one side, "b" twice on the other) → must never
    /// match. An implementation that would "simplify" to a `HashSet` (which deduplicates)
    /// would see both sets become `{a#1, b#2}` on both sides and would wrongly declare the
    /// same game.
    #[test]
    fn des_multiplicites_differentes_ne_correspondent_jamais() {
        let a = tags(&["a#1", "a#1", "b#2"]);
        let b = tags(&["a#1", "b#2", "b#2"]);
        assert!(!memes_battletags(&a, &b));
    }

    #[test]
    fn un_lobby_detecte_recemment_n_est_pas_trop_ancien() {
        let recent = chrono::Utc::now().to_rfc3339();
        assert!(!lobby_trop_ancien(&recent));
    }

    #[test]
    fn un_lobby_detecte_il_y_a_sept_heures_est_trop_ancien() {
        let il_y_a_sept_heures = (chrono::Utc::now() - chrono::Duration::hours(7)).to_rfc3339();
        assert!(lobby_trop_ancien(&il_y_a_sept_heures));
    }

    #[test]
    fn une_date_illisible_n_est_jamais_trop_ancienne() {
        assert!(!lobby_trop_ancien("pas-une-date"));
    }
}
