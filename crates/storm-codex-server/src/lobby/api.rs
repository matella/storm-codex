//! Live lobby routes. `POST /api/lobby` receives the raw bytes of the lobby file (same
//! contract as `/api/upload-raw`: `Bearer` + binary body), decodes it, enriches it, persists it and
//! broadcasts `lobby.detected`. With the parser living on the server side, a Blizzard format break
//! is fixed by a redeployment of the box, without ever touching the Windows binary again.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value as J};

use crate::lobby::{store, LobbyState};
use crate::AppState;

/// Second line of defence of idempotence, after the hash guard of `ingest` (see
/// [`decider`]): this one absorbs any byte-for-byte identical repost — including on an
/// already linked lobby, which protects the debrief from a simple restart of the watcher that re-posts from
/// disk the same `replay.server.battlelobby`. `meme_lobby` therefore never sees two identical
/// blobs; it only handles the case where the hash changed but the BattleTags could
/// still designate "the same lobby" (the game rewrites the file in several passes around
/// the right read). We compare the set of BattleTags.
fn meme_lobby(a: &LobbyState, b: &LobbyState) -> bool {
    // (c) A lobby already linked to a match (`a.match_id.is_some()`) describes a finished game: a
    // blob whose hash differs (otherwise the `decider` guard would have absorbed it before reaching
    // here) but whose BattleTags coincide is therefore never "the same lobby" as a blob that
    // was just ingested. Real scenario: rematch in a league/scrim between the same line-up (see
    // `FENETRE_LIAISON_HEURES` in `lobby/mod.rs`, which documents that this case is frequent). Without
    // this guard, the blob of game 2 would be judged identical to the — already linked — one of game
    // 1: `unchanged`, no re-enrichment, no `lobby.detected`, and the page would stay
    // shown in the debrief of game 1 during the whole loading of game 2.
    if a.match_id.is_some() {
        return false;
    }
    // (a) Idempotence of unreadable states. `unreadable()` produces `players: []` with
    // `status: "parse_failed"`: without this case, the `players.is_empty()` guard below would
    // judge two unreadable blobs re-posted back to back as "different", so each repost
    // would re-save and re-broadcast `lobby.detected` for nothing. Two `parse_failed` without
    // players are the same lobby.
    if a.status.as_deref() == Some("parse_failed") && b.status.as_deref() == Some("parse_failed") {
        return true;
    }
    if a.players.len() != b.players.len() || a.players.is_empty() {
        return false;
    }
    let mut x: Vec<&str> = a.players.iter().map(|p| p.battletag.as_str()).collect();
    let mut y: Vec<&str> = b.players.iter().map(|p| p.battletag.as_str()).collect();
    x.sort_unstable();
    y.sort_unstable();
    x == y
}

/// (b) A `parse_failed` must never overwrite a readable lobby just detected: the game
/// rewrites the lobby file for a few seconds around the right read, which
/// produces a transient truncated read — not a real end of game. Two real games
/// are separated by at least ten minutes or so, so a 2-minute window covers
/// the transient case widely without ever delaying a really new lobby, including the day
/// Blizzard breaks the format and all the blobs become unreadable (the next lobby, itself
/// `parse_failed` too, will be accepted as soon as `prec` is more than 2 minutes old).
fn parse_failed_transitoire(prec: &LobbyState, nouveau: &LobbyState) -> bool {
    if nouveau.status.as_deref() != Some("parse_failed") || prec.players.is_empty() {
        return false;
    }
    match chrono::DateTime::parse_from_rfc3339(&prec.detected_at) {
        Ok(detecte) => {
            chrono::Utc::now().signed_duration_since(detecte) < chrono::Duration::minutes(2)
        }
        // Unreadable date: we never accept to block a state on a date we cannot
        // interpret — the new state (parse_failed) goes through normally.
        Err(_) => false,
    }
}

/// Idempotence decision of `ingest`, extracted as a pure function to be testable without
/// `AppState` (same pattern as `meme_lobby`/`appliquer_teams`).
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Unchanged,
    Ingerer,
}

/// Rule applied in this order, `prec` being the state currently in memory (`None` if no
/// lobby has been detected yet):
/// 1. **hash identical to `prec`** → `Unchanged`, whatever happens, including if `prec` is already
///    linked to a match. It is the guard that protects the debrief: `replay.server.battlelobby` survives
///    on disk between two games, so a restart of the watcher that re-posts it must
///    never overwrite an already enriched state with `match_id: None`.
/// 2. otherwise, the existing rules (`meme_lobby`, `parse_failed_transitoire`) apply
///    unchanged — the "already linked" guard of `meme_lobby` included: a different blob with the
///    same BattleTags on a linked lobby is a rematch, and must be ingested.
fn decider(prec: Option<&LobbyState>, nouveau: &LobbyState) -> Decision {
    let Some(prec) = prec else {
        return Decision::Ingerer;
    };
    if prec.content_hash == nouveau.content_hash {
        return Decision::Unchanged;
    }
    if meme_lobby(prec, nouveau) || parse_failed_transitoire(prec, nouveau) {
        return Decision::Unchanged;
    }
    Decision::Ingerer
}

/// `POST /api/lobby` — raw bytes, upload `Bearer`.
pub async fn ingest(
    State(s): State<AppState>,
    headers: HeaderMap,
    bytes: axum::body::Bytes,
) -> (StatusCode, Json<J>) {
    if !crate::upload::token_valide(&s.db, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        );
    }

    // Computed before decoding, on the raw bytes: it is the same helper as the replay
    // upload (`crate::upload::sha256_hex`), and it must be set even if the blob turns out
    // unreadable below — a re-posted unreadable blob must also be recognised as a repost.
    let content_hash = crate::upload::sha256_hex(&bytes);
    let detected_at = chrono::Utc::now().to_rfc3339();
    let mut state = match storm_lobby::parse(&bytes) {
        Ok(lobby) => LobbyState::from_lobby(&lobby, detected_at),
        Err(e) => {
            // Not an HTTP error: the page must open anyway, with the manual selector.
            tracing::warn!("unreadable lobby ({} bytes): {e}", bytes.len());
            LobbyState::unreadable(detected_at)
        }
    };
    state.content_hash = content_hash;

    {
        let courant = s.lobby.read().await;
        if decider(courant.as_ref(), &state) == Decision::Unchanged {
            return (StatusCode::OK, Json(json!({ "status": "unchanged" })));
        }
    }

    crate::lobby::enrich::enrich(&s.db, &mut state).await;
    if let Err(e) = store::save(&s.db, &state).await {
        tracing::error!("lobby save: {e}");
    }
    let joueurs = state.players.len();
    *s.lobby.write().await = Some(state);
    let _ = s.events.send(json!({ "type": "lobby.detected" }));

    (
        StatusCode::ACCEPTED,
        Json(json!({ "status": "ok", "players": joueurs })),
    )
}

/// `GET /api/lobby` — the current enriched state, `204` if no lobby.
pub async fn get(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    match s.lobby.read().await.as_ref() {
        None => (StatusCode::NO_CONTENT, Json(J::Null)),
        Some(st) => (
            StatusCode::OK,
            Json(serde_json::to_value(st).unwrap_or(J::Null)),
        ),
    }
}

/// `DELETE /api/lobby` — close the companion.
pub async fn clear(State(s): State<AppState>) -> Json<J> {
    if let Err(e) = store::clear(&s.db).await {
        tracing::error!("lobby clear: {e}");
    }
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
}

#[derive(serde::Deserialize)]
pub struct HeroBody {
    /// `dim_heroes.id` key. `null` to clear the input.
    pub hero: Option<String>,
}

/// `POST /api/lobby/hero` — the hero is never in the blob: it is the only mandatory tap.
pub async fn set_hero(State(s): State<AppState>, Json(b): Json<HeroBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| st.hero = b.hero).await
}

#[derive(serde::Deserialize)]
pub struct MapBody {
    pub map: Option<String>,
}

/// `POST /api/lobby/map` — fallback when the map could not be deduced from the `.s2ma` hashes.
pub async fn set_map(State(s): State<AppState>, Json(b): Json<MapBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| {
        // `map_manual` reflects the real state: true only if a map is actually
        // entered, false if the input is cleared (`map: null`) — never "corrected" on nothing.
        st.map_manual = b.map.is_some();
        st.map = b.map;
    })
    .await
}

#[derive(serde::Deserialize)]
pub struct TeamsBody {
    /// `battletag → team (0 or 1)`. The players missing from the table keep their team.
    pub teams: std::collections::HashMap<String, u8>,
}

/// `POST /api/lobby/teams` — per-player reassignment, not a swap button. Measured on
/// 3,322 real games (plan 1): the team deduction is 100% reliable in matchmaking but
/// is often wrong in custom games, and among the failures only 5.3% are a swap
/// of the two sides — the remaining 94.7% are orders that carry no team
/// information. A "swap" button would therefore be useless 19 times out of 20: only an explicit
/// per-player entry can rebuild the correct team in the general case.
pub async fn set_teams(
    State(s): State<AppState>,
    Json(b): Json<TeamsBody>,
) -> (StatusCode, Json<J>) {
    muter(s, |st| appliquer_teams(&mut st.players, &b.teams)).await
}

/// Core of `set_teams`, extracted as a pure function to be testable without `AppState` (no Postgres
/// pool, no `RwLock`). Strictly additive: a player missing from `teams`, or whose
/// value is not 0/1, keeps their original team and `team_manual` intact.
fn appliquer_teams(
    players: &mut [crate::lobby::LobbyPlayerState],
    teams: &std::collections::HashMap<String, u8>,
) {
    for p in players {
        if let Some(t) = teams.get(&p.battletag) {
            if *t <= 1 {
                p.team = Some(*t);
                p.team_manual = true;
            }
        }
    }
}

/// Mutation + re-enrichment + persistence + broadcast, factored: the three routes
/// `set_hero`/`set_map`/`set_teams` above only differ by the mutation itself.
///
/// The write-lock is held during `enrich` (SQL) and `save`: this is deliberate, not an oversight, **for
/// these three input routes**. The server has a single user so contention is
/// nil — but the lock stays their correctness boundary: two close inputs from
/// the UI must never interleave in the middle of an enrichment. `ingest` (the `POST
/// /api/lobby` that receives the blob) is also a mutation but follows a different pattern: it
/// only enriches after releasing the read lock used to detect duplicates — a
/// new blob does not need to be prevented from interleaving with the enrichment of another
/// since by construction there is only one lobby being detected at a time on the game side. The
/// WebSocket broadcast, for its part, happens after the explicit `drop(guard)` below: it does not
/// need the lock and must not delay its release.
async fn muter<F>(s: AppState, f: F) -> (StatusCode, Json<J>)
where
    F: FnOnce(&mut LobbyState),
{
    let mut guard = s.lobby.write().await;
    let Some(st) = guard.as_mut() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "aucun lobby courant" })),
        );
    };
    f(st);
    crate::lobby::enrich::enrich(&s.db, st).await;
    if let Err(e) = store::save(&s.db, st).await {
        tracing::error!("lobby save: {e}");
    }
    let out = serde_json::to_value(&*st).unwrap_or(J::Null);
    drop(guard);
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    (StatusCode::OK, Json(out))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::lobby::LobbyPlayerState;

    fn joueur(battletag: &str) -> LobbyPlayerState {
        LobbyPlayerState {
            name: battletag.split('#').next().unwrap_or_default().to_string(),
            discriminant: String::new(),
            battletag: battletag.to_string(),
            team: None,
            team_manual: false,
            toon_handle: None,
            history: None,
        }
    }

    fn lobby_avec(battletags: &[&str]) -> LobbyState {
        LobbyState {
            schema_version: crate::lobby::LOBBY_SCHEMA_VERSION,
            detected_at: chrono::Utc::now().to_rfc3339(),
            players: battletags.iter().map(|b| joueur(b)).collect(),
            map: None,
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

    fn dix_battletags() -> [&'static str; 10] {
        [
            "a#1", "b#2", "c#3", "d#4", "e#5", "f#6", "g#7", "h#8", "i#9", "j#10",
        ]
    }

    #[test]
    fn memes_battletags_ordre_different_sont_le_meme_lobby() {
        let mut mélange = dix_battletags();
        mélange.reverse();
        let a = lobby_avec(&dix_battletags());
        let b = lobby_avec(&mélange);
        assert!(meme_lobby(&a, &b));
    }

    #[test]
    fn un_battletag_different_donne_des_lobbys_differents() {
        let a = lobby_avec(&dix_battletags());
        let mut autre = dix_battletags();
        autre[9] = "quelqu_un_d_autre#99";
        let b = lobby_avec(&autre);
        assert!(!meme_lobby(&a, &b));
    }

    #[test]
    fn deux_parse_failed_sont_le_meme_lobby() {
        let a = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        let b = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(meme_lobby(&a, &b));
    }

    #[test]
    fn un_lobby_deja_lie_n_est_jamais_le_meme_qu_un_blob_entrant() {
        // Rematch between the same ten players (league/scrim): game 1 is already linked to its
        // replay (`match_id: Some(...)`) when the lobby of game 2, same line-up, arrives.
        // Without guard (c) of `meme_lobby`, this test would fail: the identical BattleTags
        // would make the two lobbies be judged identical, and game 2 would never be ingested.
        let mut a = lobby_avec(&dix_battletags());
        a.match_id = Some(42);
        let b = lobby_avec(&dix_battletags());
        assert!(!meme_lobby(&a, &b));
    }

    #[test]
    fn meme_hash_sur_lobby_deja_lie_est_unchanged() {
        // This is the case that protects the debrief: `replay.server.battlelobby` survives on disk
        // between two games, a restart of the watcher re-posts it identically. Without the hash
        // guard at the head of `decider`, `meme_lobby` would judge (rightly, given guard (c)) that this
        // is *not* the same lobby since `match_id.is_some()` — and would overwrite the debrief.
        let mut prec = lobby_avec(&dix_battletags());
        prec.match_id = Some(42);
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "abc123".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Unchanged);
    }

    #[test]
    fn hash_different_memes_battletags_sur_lobby_lie_est_ingere() {
        // The rematch between the same ten players must still be ingested: this test
        // checks that the hash guard does not reintroduce the bug that guard (c) of
        // `meme_lobby` had fixed.
        let mut prec = lobby_avec(&dix_battletags());
        prec.match_id = Some(42);
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "xyz789".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Ingerer);
    }

    #[test]
    fn meme_hash_sur_lobby_non_lie_est_unchanged() {
        // Ordinary repost (watcher restarted, lobby not linked yet): case already covered before
        // the introduction of the hash, not to regress.
        let mut prec = lobby_avec(&dix_battletags());
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "abc123".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Unchanged);
    }

    #[test]
    fn aucun_lobby_courant_est_toujours_ingere() {
        let nouveau = lobby_avec(&dix_battletags());
        assert_eq!(decider(None, &nouveau), Decision::Ingerer);
    }

    #[test]
    fn lobby_lisible_et_parse_failed_sont_differents() {
        let a = lobby_avec(&dix_battletags());
        let b = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!meme_lobby(&a, &b));
        assert!(!meme_lobby(&b, &a));
    }

    #[test]
    fn parse_failed_juste_apres_un_lobby_lisible_est_transitoire() {
        let prec = lobby_avec(&dix_battletags());
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn parse_failed_apres_plus_de_deux_minutes_n_est_plus_transitoire() {
        let vieux = chrono::Utc::now() - chrono::Duration::minutes(3);
        let mut prec = lobby_avec(&dix_battletags());
        prec.detected_at = vieux.to_rfc3339();
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn parse_failed_sur_lobby_deja_illisible_n_est_pas_transitoire() {
        // This case belongs to `meme_lobby` (rule a), not to rule b: nothing to ignore here.
        let prec = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn date_illisible_n_est_jamais_transitoire() {
        let mut prec = lobby_avec(&dix_battletags());
        prec.detected_at = "pas-une-date".to_string();
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn appliquer_teams_reassigne_le_joueur_nomme_et_le_marque_manuel() {
        let mut joueurs = vec![joueur("a#1"), joueur("b#2")];
        let teams = std::collections::HashMap::from([("a#1".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, Some(1));
        assert!(joueurs[0].team_manual);
    }

    #[test]
    fn appliquer_teams_est_additive_les_absents_gardent_leur_etat() {
        // Most important property: a player missing from the table must be touched neither
        // in their team nor in their `team_manual`, even if another player of the same call is
        // reassigned. This test would fail if `appliquer_teams` reset the missing players
        // (e.g. `p.team = None` by default before the loop).
        let mut joueurs = vec![joueur("a#1"), joueur("b#2")];
        joueurs[1].team = Some(0);
        joueurs[1].team_manual = true;
        let teams = std::collections::HashMap::from([("a#1".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[1].team, Some(0));
        assert!(joueurs[1].team_manual);
    }

    #[test]
    fn appliquer_teams_ignore_une_valeur_hors_0_1() {
        // This test would fail if the `<= 1` filter were removed: `team` would become `Some(2)` and
        // `team_manual` `true` instead of staying intact.
        let mut joueurs = vec![joueur("a#1")];
        let teams = std::collections::HashMap::from([("a#1".to_string(), 2u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, None);
        assert!(!joueurs[0].team_manual);
    }

    #[test]
    fn appliquer_teams_battletag_inconnu_n_a_aucun_effet_et_ne_panique_pas() {
        let mut joueurs = vec![joueur("a#1")];
        let teams = std::collections::HashMap::from([("inconnu#99".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, None);
        assert!(!joueurs[0].team_manual);
    }
}
