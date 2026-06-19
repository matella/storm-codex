//! Routes REST du draft. Chaque mutation prend le write-lock, persiste, puis diffuse `draft.updated`
//! sur le WS (les clients refetch `GET /api/draft`). Mode ouvert (auto-hébergement local), pas d'auth.
use crate::draft::{store, DraftState, Format, Side, TeamInfo};
use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value as J};

/// Persiste l'état courant + diffuse l'event temps réel.
async fn persist(s: &AppState) {
    let snapshot = { s.draft.read().await.clone() };
    if let Err(e) = store::save(&s.db, &snapshot).await {
        tracing::error!("draft save: {e}");
    }
    let _ = s.events.send(json!({ "type": "draft.updated" }));
}

/// GET /api/draft — état courant.
pub async fn get_draft(State(s): State<AppState>) -> Json<DraftState> {
    Json(s.draft.read().await.clone())
}

#[derive(Deserialize)]
pub struct ConfigBody {
    format: Format,
    map: String,
    first_pick: Side,
    #[serde(default)]
    blue: Option<TeamInfo>,
    #[serde(default)]
    red: Option<TeamInfo>,
    #[serde(default)]
    bo: Option<u8>,
}

/// POST /api/draft/config — (re)configure et réinitialise le draft (nouvelle série).
pub async fn config(State(s): State<AppState>, Json(b): Json<ConfigBody>) -> Json<DraftState> {
    let mut d = DraftState::new(b.format, b.first_pick, b.map);
    if let Some(t) = b.blue {
        d.blue = t;
    }
    if let Some(t) = b.red {
        d.red = t;
    }
    if let Some(bo) = b.bo {
        d.bo = bo;
    }
    *s.draft.write().await = d;
    persist(&s).await;
    get_draft(State(s)).await
}

#[derive(Deserialize)]
pub struct HeroBody {
    hero: String,
}

/// POST /api/draft/action — assigne un héros à l'étape courante.
pub async fn action(State(s): State<AppState>, Json(b): Json<HeroBody>) -> (StatusCode, Json<J>) {
    let res = { s.draft.write().await.apply(&b.hero) };
    match res {
        Ok(()) => {
            persist(&s).await;
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Err(e) => (StatusCode::CONFLICT, Json(json!({ "error": format!("{e:?}") }))),
    }
}

/// POST /api/draft/undo
pub async fn undo(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    let res = { s.draft.write().await.undo() };
    match res {
        Ok(()) => {
            persist(&s).await;
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Err(e) => (StatusCode::CONFLICT, Json(json!({ "error": format!("{e:?}") }))),
    }
}

/// POST /api/draft/reset
pub async fn reset(State(s): State<AppState>) -> Json<DraftState> {
    s.draft.write().await.reset();
    persist(&s).await;
    get_draft(State(s)).await
}

#[derive(Deserialize)]
pub struct UnavailableBody {
    hero: String,
    value: bool,
}

/// POST /api/draft/unavailable — override manuel de la disponibilité d'un héros.
pub async fn unavailable(State(s): State<AppState>, Json(b): Json<UnavailableBody>) -> Json<DraftState> {
    s.draft.write().await.set_unavailable(&b.hero, b.value);
    persist(&s).await;
    get_draft(State(s)).await
}

#[derive(Deserialize)]
pub struct ScoreBody {
    blue: u8,
    red: u8,
}

/// POST /api/draft/score
pub async fn score(State(s): State<AppState>, Json(b): Json<ScoreBody>) -> Json<DraftState> {
    {
        let mut d = s.draft.write().await;
        d.score = [b.blue, b.red];
    }
    persist(&s).await;
    get_draft(State(s)).await
}

#[derive(Deserialize)]
pub struct TeamsBody {
    blue: TeamInfo,
    red: TeamInfo,
}

/// POST /api/draft/teams — met à jour noms + pseudos SANS réinitialiser le draft en cours.
pub async fn teams(State(s): State<AppState>, Json(b): Json<TeamsBody>) -> Json<DraftState> {
    {
        let mut d = s.draft.write().await;
        d.blue = b.blue;
        d.red = b.red;
    }
    persist(&s).await;
    get_draft(State(s)).await
}

/// POST /api/draft/series/next — clôt la partie courante, cumule les bans fearless, repart.
pub async fn series_next(State(s): State<AppState>) -> Json<DraftState> {
    s.draft.write().await.start_next_game();
    persist(&s).await;
    get_draft(State(s)).await
}

/// POST /api/draft/series/new — nouvelle série (vide l'historique + les series bans).
pub async fn series_new(State(s): State<AppState>) -> Json<DraftState> {
    s.draft.write().await.new_series();
    persist(&s).await;
    get_draft(State(s)).await
}
