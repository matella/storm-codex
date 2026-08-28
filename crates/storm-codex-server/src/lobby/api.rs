//! Routes du lobby live. `POST /api/lobby` reçoit les octets bruts du fichier de lobby (même
//! contrat que `/api/upload-raw` : `Bearer` + corps binaire), le décode, l'enrichit, le persiste et
//! diffuse `lobby.detected`. Le parseur vivant côté serveur, une casse de format Blizzard se
//! corrige par un redéploiement du box, sans jamais retoucher le binaire Windows.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value as J};

use crate::lobby::{store, LobbyState};
use crate::AppState;

/// Idempotence : deux POST du même fichier (le watcher redémarre, le jeu réécrit) ne doivent pas
/// écraser un état déjà enrichi ni rediffuser un événement. On compare l'ensemble des BattleTags.
fn meme_lobby(a: &LobbyState, b: &LobbyState) -> bool {
    // (a) Idempotence des états illisibles. `unreadable()` produit `players: []` avec
    // `status: "parse_failed"` : sans ce cas, le garde `players.is_empty()` ci-dessous ferait
    // juger « différents » deux blobs illisibles reposés coup sur coup, donc chaque repost
    // resauvegarderait et rediffuserait `lobby.detected` pour rien. Deux `parse_failed` sans
    // joueur sont le même lobby.
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

/// (b) Un `parse_failed` ne doit jamais écraser un lobby lisible tout juste détecté : le jeu
/// réécrit le fichier de lobby pendant quelques secondes autour de la bonne lecture, ce qui
/// produit une lecture tronquée transitoire — pas une vraie fin de partie. Deux parties réelles
/// sont séparées d'au moins une dizaine de minutes, donc une fenêtre de 2 minutes couvre
/// largement le cas transitoire sans jamais retarder un lobby réellement neuf, y compris le jour
/// où Blizzard casse le format et où tous les blobs deviennent illisibles (le lobby suivant, lui
/// aussi `parse_failed`, sera accepté dès que `prec` aura plus de 2 minutes).
fn parse_failed_transitoire(prec: &LobbyState, nouveau: &LobbyState) -> bool {
    if nouveau.status.as_deref() != Some("parse_failed") || prec.players.is_empty() {
        return false;
    }
    match chrono::DateTime::parse_from_rfc3339(&prec.detected_at) {
        Ok(detecte) => {
            chrono::Utc::now().signed_duration_since(detecte) < chrono::Duration::minutes(2)
        }
        // Date illisible : on n'accepte jamais de bloquer un état sur une date qu'on ne sait
        // pas interpréter — le nouvel état (parse_failed) passe normalement.
        Err(_) => false,
    }
}

/// `POST /api/lobby` — octets bruts, `Bearer` d'upload.
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

    let detected_at = chrono::Utc::now().to_rfc3339();
    let mut state = match storm_lobby::parse(&bytes) {
        Ok(lobby) => LobbyState::from_lobby(&lobby, detected_at),
        Err(e) => {
            // Pas une erreur HTTP : la page doit s'ouvrir quand même, avec le sélecteur manuel.
            tracing::warn!("lobby illisible ({} octets) : {e}", bytes.len());
            LobbyState::unreadable(detected_at)
        }
    };

    {
        let courant = s.lobby.read().await;
        if let Some(prec) = courant.as_ref() {
            if meme_lobby(prec, &state) || parse_failed_transitoire(prec, &state) {
                return (StatusCode::OK, Json(json!({ "status": "unchanged" })));
            }
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

/// `GET /api/lobby` — l'état enrichi courant, `204` si aucun lobby.
pub async fn get(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    match s.lobby.read().await.as_ref() {
        None => (StatusCode::NO_CONTENT, Json(J::Null)),
        Some(st) => (
            StatusCode::OK,
            Json(serde_json::to_value(st).unwrap_or(J::Null)),
        ),
    }
}

/// `DELETE /api/lobby` — fermer le companion.
pub async fn clear(State(s): State<AppState>) -> Json<J> {
    if let Err(e) = store::clear(&s.db).await {
        tracing::error!("lobby clear: {e}");
    }
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
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
        // Ce cas relève de `meme_lobby` (règle a), pas de la règle b : rien à ignorer ici.
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
}
