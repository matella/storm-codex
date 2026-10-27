//! Routes REST du draft. Chaque mutation prend le write-lock, persiste, puis diffuse `draft.updated`
//! sur le WS (les clients refetch `GET /api/draft`). Mode ouvert (auto-hébergement local), pas d'auth.
use crate::draft::{store, DraftState, Format, Side, TeamInfo};
use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value as J};

/// Persiste l'état courant + diffuse l'event temps réel.
pub(crate) async fn persist(s: &AppState) {
    let snapshot = { s.draft.read().await.clone() };
    if let Err(e) = store::save(&s.db, &snapshot).await {
        tracing::error!("draft save: {e}");
    }
    let _ = s.events.send(json!({ "type": "draft.updated" }));
}

/// Format imposé par la série de cast quand elle est active (`None` sinon). Le verrou cast est
/// relâché avant tout verrou draft (jamais les deux en même temps).
pub(crate) async fn series_format(s: &AppState) -> Option<Format> {
    let c = s.cast.read().await;
    c.active.then_some(c.format)
}

fn series_active() -> (StatusCode, Json<J>) {
    (StatusCode::CONFLICT, Json(json!({ "error": "series_active" })))
}

/// GET /api/draft — état courant.
pub async fn get_draft(State(s): State<AppState>) -> Json<DraftState> {
    // vue publique : sans scripts de replay ni pseudos des picks non révélés
    Json(s.draft.read().await.public())
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

/// POST /api/draft/config — (re)configure la partie courante. PRÉSERVE le contexte de série
/// (fearless), les prébans manuels, le score et les équipes : changer la map/format ne doit pas
/// effacer les prébans ni le score. Seuls format/map/first_pick/bo + picks de la game sont remis.
///
/// Série de cast active : équipes, pseudos, Bo (copiés depuis la série) et format appartiennent à la
/// série — `blue`/`red`/`bo` du corps sont ignorés et `format` forcé ; map et first_pick restent libres.
pub async fn config(State(s): State<AppState>, Json(b): Json<ConfigBody>) -> Json<DraftState> {
    let forced = series_format(&s).await;
    let prev = { s.draft.read().await.clone() };
    let mut d = DraftState::new(forced.unwrap_or(b.format), b.first_pick, b.map);
    if forced.is_some() {
        d.blue = prev.blue;
        d.red = prev.red;
        d.bo = prev.bo;
    } else {
        d.blue = b.blue.unwrap_or(prev.blue);
        d.red = b.red.unwrap_or(prev.red);
        d.bo = b.bo.unwrap_or(prev.bo);
    }
    d.score = prev.score;
    d.series_games = prev.series_games;
    d.series_bans = prev.series_bans;
    d.manual_unavailable = prev.manual_unavailable;
    // la file de replays survit à une reconfiguration (partie courante désélectionnée, lecture stoppée)
    d.replay = super::ReplayState { current: None, playing: false, next_at_ms: None, ..prev.replay };
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

/// POST /api/draft/score — 409 `series_active` tant qu'une série de cast est active.
pub async fn score(State(s): State<AppState>, Json(b): Json<ScoreBody>) -> Result<Json<DraftState>, (StatusCode, Json<J>)> {
    if series_format(&s).await.is_some() {
        return Err(series_active());
    }
    {
        let mut d = s.draft.write().await;
        d.score = [b.blue, b.red];
    }
    persist(&s).await;
    Ok(get_draft(State(s)).await)
}

#[derive(Deserialize)]
pub struct TeamsBody {
    blue: TeamInfo,
    red: TeamInfo,
}

/// POST /api/draft/teams — met à jour noms + pseudos SANS réinitialiser le draft en cours.
/// 409 `series_active` tant qu'une série de cast est active (la série est la source de vérité).
pub async fn teams(State(s): State<AppState>, Json(b): Json<TeamsBody>) -> Result<Json<DraftState>, (StatusCode, Json<J>)> {
    if series_format(&s).await.is_some() {
        return Err(series_active());
    }
    {
        let mut d = s.draft.write().await;
        d.blue = b.blue;
        d.red = b.red;
    }
    persist(&s).await;
    Ok(get_draft(State(s)).await)
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::api_tests::{call, test_state};
    use axum::http::StatusCode;
    use serde_json::json;

    fn team(n: &str) -> serde_json::Value {
        json!({"name": n, "players": ["a", "b", "c", "d", "e"]})
    }

    #[tokio::test]
    async fn draft_teams_route_returns_409_when_series_active() {
        let Some(state) = test_state(None).await else { return };
        state.cast.write().await.active = true;
        let before = state.draft.read().await.blue.name.clone();
        for (path, body) in [
            ("/api/draft/teams", json!({"blue": team("X"), "red": team("Y")})),
            ("/api/draft/score", json!({"blue": 2, "red": 1})),
        ] {
            let (st, v) = call(&state, "POST", path, Some(body)).await;
            assert_eq!(st, StatusCode::CONFLICT, "{path}");
            assert_eq!(v["error"], "series_active", "{path}");
        }
        let d = state.draft.read().await;
        assert_eq!(d.blue.name, before);
        assert_eq!(d.score, [0, 0]);
    }

    #[tokio::test]
    async fn draft_teams_route_returns_200_when_series_inactive() {
        let Some(state) = test_state(None).await else { return };
        assert!(!state.cast.read().await.active);
        let (st, _) = call(&state, "POST", "/api/draft/teams", Some(json!({"blue": team("X"), "red": team("Y")}))).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = call(&state, "POST", "/api/draft/score", Some(json!({"blue": 2, "red": 1}))).await;
        assert_eq!(st, StatusCode::OK);
        let d = state.draft.read().await;
        assert_eq!((d.blue.name.as_str(), d.score), ("X", [2, 1]));
    }

    #[tokio::test]
    async fn draft_config_in_active_series_keeps_series_owned_fields() {
        let Some(state) = test_state(None).await else { return };
        {
            let mut c = state.cast.write().await;
            c.active = true;
            c.format = crate::draft::Format::Fearless;
        }
        {
            let mut d = state.draft.write().await;
            d.blue.name = "Synced".into();
            d.bo = 7;
        }
        let body = json!({"format": "standard", "map": "Cursed Hollow", "first_pick": "red",
            "blue": team("Hijack"), "red": team("Hijack"), "bo": 1});
        let (st, v) = call(&state, "POST", "/api/draft/config", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["format"], "fearless");
        assert_eq!(v["blue"]["name"], "Synced");
        assert_eq!(v["bo"], 7);
        assert_eq!(v["map"], "Cursed Hollow");
        assert_eq!(v["first_pick"], "red");
    }

    #[tokio::test]
    async fn draft_config_inactive_series_behaves_as_before() {
        let Some(state) = test_state(None).await else { return };
        let body = json!({"format": "fearless", "map": "Sky Temple", "first_pick": "blue",
            "blue": team("Mine"), "bo": 3});
        let (st, v) = call(&state, "POST", "/api/draft/config", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!((v["format"].as_str(), v["blue"]["name"].as_str(), v["bo"].as_u64()), (Some("fearless"), Some("Mine"), Some(3)));
    }
}
