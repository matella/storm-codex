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
    if a.players.len() != b.players.len() || a.players.is_empty() {
        return false;
    }
    let mut x: Vec<&str> = a.players.iter().map(|p| p.battletag.as_str()).collect();
    let mut y: Vec<&str> = b.players.iter().map(|p| p.battletag.as_str()).collect();
    x.sort_unstable();
    y.sort_unstable();
    x == y
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
            if meme_lobby(prec, &state) {
                return (StatusCode::OK, Json(json!({ "status": "unchanged" })));
            }
        }
    }

    crate::lobby::enrich::enrich(&s.db, &mut state).await;
    let _ = store::save(&s.db, &state).await;
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
    let _ = store::clear(&s.db).await;
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
}
