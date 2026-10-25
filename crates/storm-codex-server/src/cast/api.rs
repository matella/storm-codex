//! Routes REST de la régie de cast. Chaque mutation prend le write-lock une seule fois (sur une
//! copie : une erreur ne change jamais l'état), puis persiste et diffuse `cast.updated` sur le WS
//! (les clients refetch `GET /api/cast`). Mode ouvert comme `/api/draft`, pas d'auth.
use super::{store, CastError, LineupMode, MapStatus, Scene, Series, Team};
use crate::draft::{Format, Side};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

type Resp = (StatusCode, Json<J>);

/// Bornes de l'intervalle du diaporama (ms).
const INTERVAL_RANGE: std::ops::RangeInclusive<u32> = 500..=3_600_000;

/// Persiste la série courante + diffuse l'event temps réel (sans état : les clients refetch).
pub(super) async fn persist(s: &AppState) {
    let snapshot = { s.cast.read().await.clone() };
    if let Err(e) = store::save(&s.db, &snapshot).await {
        tracing::error!("cast save: {e}");
    }
    let _ = s.events.send(json!({ "type": "cast.updated" }));
}

fn conflict(e: CastError) -> Resp {
    (StatusCode::CONFLICT, Json(json!({ "error": format!("{e:?}") })))
}

fn series_json(s: &Series) -> Resp {
    (StatusCode::OK, Json(serde_json::to_value(s).unwrap_or(J::Null)))
}

/// Applique `f` sous un seul write-lock, sur une copie (échec → état inchangé, pas de persist).
/// `gated` : refuse (409 `SeriesInactive`) tant que la série n'est pas active.
async fn mutate<T>(
    s: &AppState,
    gated: bool,
    f: impl FnOnce(&mut Series) -> Result<T, CastError>,
) -> Result<(T, Series), Resp> {
    let out = {
        let mut g = s.cast.write().await;
        if gated && !g.active {
            return Err(conflict(CastError::SeriesInactive));
        }
        let mut next = g.clone();
        let v = f(&mut next).map_err(conflict)?;
        *g = next.clone();
        (v, next)
    };
    persist(s).await;
    Ok(out)
}

/// Mutation qui répond par la série complète.
async fn mutate_series(s: &AppState, gated: bool, f: impl FnOnce(&mut Series) -> Result<(), CastError>) -> Resp {
    match mutate(s, gated, f).await {
        Ok(((), series)) => series_json(&series),
        Err(r) => r,
    }
}

/// GET /api/cast — état courant.
pub async fn get_cast(State(s): State<AppState>) -> Json<Series> {
    Json(s.cast.read().await.clone())
}

#[derive(Deserialize)]
pub struct TeamName {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
pub struct SeriesBody {
    blue: TeamName,
    red: TeamName,
    bo: u8,
    format: Format,
    #[serde(default)]
    pool: Vec<String>,
}

/// POST /api/cast/series — crée/redémarre : parties, score et diaporama remis à zéro (la scène
/// courante est conservée), `active = true`.
pub async fn start_series(State(s): State<AppState>, Json(b): Json<SeriesBody>) -> Resp {
    mutate_series(&s, false, |c| {
        let mut n = Series::new();
        n.active = true;
        n.blue = Team { name: b.blue.name, ..Team::default() };
        n.red = Team { name: b.red.name, ..Team::default() };
        n.bo = b.bo;
        n.format = b.format;
        n.scene = c.scene;
        n.set_pool(&b.pool);
        *c = n;
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct TeamsBody {
    blue: Team,
    red: Team,
}

/// PUT /api/cast/teams — remplace les deux équipes (nom, logo, bilan, joueurs, capitaine).
pub async fn put_teams(State(s): State<AppState>, Json(b): Json<TeamsBody>) -> Resp {
    mutate_series(&s, true, |c| {
        c.blue = b.blue;
        c.red = b.red;
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct MapBody {
    name: String,
    #[serde(flatten)]
    status: MapStatus,
}

/// POST /api/cast/pool/map `{name, status, by?}` — ban / pick / decider / available.
pub async fn mark_map(State(s): State<AppState>, Json(b): Json<MapBody>) -> Resp {
    mutate_series(&s, true, |c| c.mark_map(&b.name, b.status)).await
}

#[derive(Deserialize)]
pub struct StartGameBody {
    map: String,
}

/// POST /api/cast/games/start `{map}` → `{number}`.
pub async fn start_game(State(s): State<AppState>, Json(b): Json<StartGameBody>) -> Resp {
    match mutate(&s, true, |c| c.start_game(&b.map)).await {
        Ok((number, _)) => (StatusCode::OK, Json(json!({ "number": number }))),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
pub struct WinnerBody {
    side: Side,
    #[serde(default)]
    takedowns: Option<[u32; 2]>,
    #[serde(default)]
    duration_secs: Option<u32>,
}

/// POST /api/cast/games/{n}/winner `{side, takedowns?, duration_secs?}`.
pub async fn set_winner(State(s): State<AppState>, Path(n): Path<u8>, Json(b): Json<WinnerBody>) -> Resp {
    mutate_series(&s, true, |c| {
        c.set_winner(n, b.side)?;
        if let Some(g) = c.games.iter_mut().find(|g| g.number == n) {
            if let Some(t) = b.takedowns {
                g.takedowns = t;
            }
            if b.duration_secs.is_some() {
                g.duration_secs = b.duration_secs;
            }
        }
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct ScoreBody {
    blue: u8,
    red: u8,
}

/// POST /api/cast/score `{blue, red}` (override) ou `null` (retour au score calculé).
pub async fn set_score(State(s): State<AppState>, Json(b): Json<Option<ScoreBody>>) -> Resp {
    mutate_series(&s, true, |c| {
        c.score_override = b.map(|b| [b.blue, b.red]);
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct SceneBody {
    scene: Scene,
}

/// POST /api/cast/scene `{scene}` — autorisé même série inactive.
pub async fn set_scene(State(s): State<AppState>, Json(b): Json<SceneBody>) -> Resp {
    mutate_series(&s, false, |c| {
        c.scene = b.scene;
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct LineupModeBody {
    mode: LineupMode,
}

/// POST /api/cast/lineup-mode `{mode}` — `auto` | `a` | `b`.
pub async fn set_lineup_mode(State(s): State<AppState>, Json(b): Json<LineupModeBody>) -> Resp {
    mutate_series(&s, true, |c| {
        c.lineup_mode = b.mode;
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct SlideBody {
    #[serde(default)]
    game: Option<u8>,
    #[serde(default)]
    index: Option<u8>,
    #[serde(default)]
    auto: Option<bool>,
    #[serde(default)]
    interval_ms: Option<u32>,
}

/// POST /api/cast/slide — mise à jour partielle du diaporama (champs absents inchangés).
pub async fn set_slide(State(s): State<AppState>, Json(b): Json<SlideBody>) -> Resp {
    mutate_series(&s, true, |c| {
        if b.game.is_some() {
            c.slide.game = b.game;
        }
        if let Some(i) = b.index {
            c.slide.index = i;
        }
        if let Some(a) = b.auto {
            c.slide.auto = a;
        }
        if let Some(ms) = b.interval_ms {
            c.slide.interval_ms = ms.clamp(*INTERVAL_RANGE.start(), *INTERVAL_RANGE.end());
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, test_state};
    use serde_json::json;
    use std::time::{Duration, Instant};

    /// Les tests d'intégration partagent la ligne `cast_live` : on les sérialise.
    static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn start_body() -> J {
        json!({"blue":{"name":"Blue Co"},"red":{"name":"Red Co"},"bo":5,"format":"fearless","pool":["A","B","C"]})
    }

    #[tokio::test]
    async fn get_cast_returns_default_inactive_series() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let (st, v) = call(&state, "GET", "/api/cast", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["active"], false);
        assert_eq!(v["scene"], "hide");
        assert_eq!(v["pool"], json!([]));
    }

    #[tokio::test]
    async fn post_series_activates_and_resets_games() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"A","status":"picked","by":"blue"}))).await;
        let (_, g) = call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"A"}))).await;
        assert_eq!(g["number"], 1);
        call(&state, "POST", "/api/cast/games/1/winner", Some(json!({"side":"red","takedowns":[3,7],"duration_secs":600}))).await;
        let (_, v) = call(&state, "GET", "/api/cast", None).await;
        assert_eq!(v["games"][0]["winner"], "red");
        assert_eq!(v["games"][0]["takedowns"], json!([3, 7]));
        assert_eq!(v["games"][0]["duration_secs"], 600);

        let (st, v) = call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["active"], true);
        assert_eq!(v["games"], json!([]));
        assert_eq!(v["score_override"], J::Null);
        assert_eq!(v["blue"]["name"], "Blue Co");
        assert_eq!(v["pool"].as_array().unwrap().len(), 3);
        assert_eq!(v["pool"][0]["status"], "available");
    }

    #[tokio::test]
    async fn invalid_map_marking_returns_409_and_leaves_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let before = state.cast.read().await.clone();
        let (st, v) = call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"Z","status":"decider"}))).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(v["error"], "NotInPool");
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn marking_a_played_map_returns_409() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"A","status":"picked","by":"blue"}))).await;
        call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"A"}))).await;
        let (st, v) = call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"A","status":"banned","by":"red"}))).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(v["error"], "MapAlreadyPlayed");
    }

    #[tokio::test]
    async fn mutations_on_inactive_series_return_409_except_series_and_scene() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        *state.cast.write().await = Series::new();
        let team = json!({"name":"x","players":[{},{},{},{},{}]});
        let cases = [
            ("PUT", "/api/cast/teams", json!({"blue":team,"red":team})),
            ("POST", "/api/cast/pool/map", json!({"name":"A","status":"banned","by":"red"})),
            ("POST", "/api/cast/games/start", json!({"map":"A"})),
            ("POST", "/api/cast/games/1/winner", json!({"side":"blue"})),
            ("POST", "/api/cast/score", json!({"blue":1,"red":0})),
            ("POST", "/api/cast/lineup-mode", json!({"mode":"a"})),
            ("POST", "/api/cast/slide", json!({"index":2})),
        ];
        for (m, p, b) in cases {
            let (st, v) = call(&state, m, p, Some(b)).await;
            assert_eq!(st, StatusCode::CONFLICT, "{p}");
            assert_eq!(v["error"], "SeriesInactive", "{p}");
        }
        assert!(!state.cast.read().await.active);
        let (st, _) = call(&state, "POST", "/api/cast/scene", Some(json!({"scene":"lineups"}))).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        assert_eq!(st, StatusCode::OK);
        let (st, v) = call(&state, "POST", "/api/cast/score", Some(json!({"blue":1,"red":0}))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["score_override"], json!([1, 0]));
        let (_, v) = call(&state, "POST", "/api/cast/score", Some(J::Null)).await;
        assert_eq!(v["score_override"], J::Null);
        // la scène survit au redémarrage de série
        assert_eq!(v["scene"], "lineups");
    }

    #[tokio::test]
    async fn teams_lineup_mode_and_slide_update_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let team = json!({"name":"T","record":"3-1","captain":2,"players":[{"name":"p1"},{},{},{},{}]});
        let (st, v) = call(&state, "PUT", "/api/cast/teams", Some(json!({"blue":team,"red":{"name":"R"}}))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["blue"]["players"][0]["name"], "p1");
        assert_eq!(v["blue"]["captain"], 2);
        let (_, v) = call(&state, "POST", "/api/cast/lineup-mode", Some(json!({"mode":"b"}))).await;
        assert_eq!(v["lineup_mode"], "b");
        let (_, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"game":1,"auto":true,"interval_ms":1}))).await;
        assert_eq!(v["slide"]["game"], 1);
        assert_eq!(v["slide"]["auto"], true);
        assert_eq!(v["slide"]["interval_ms"], 500);
        assert_eq!(v["slide"]["index"], 0);
    }

    #[tokio::test]
    async fn scene_change_broadcasts_cast_updated() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let mut rx = state.events.subscribe();
        let (st, v) = call(&state, "POST", "/api/cast/scene", Some(json!({"scene":"draft"}))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["scene"], "draft");
        assert_eq!(rx.recv().await.unwrap(), json!({"type":"cast.updated"}));
    }

    #[tokio::test]
    async fn state_survives_restart() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        call(&state, "POST", "/api/cast/scene", Some(json!({"scene":"ingame"}))).await;
        let saved = state.cast.read().await.clone();
        assert_eq!(store::load(&state.db).await, Some(saved));
    }

    #[test]
    fn load_of_garbage_json_returns_none() {
        assert!(store::decode(json!([1, 2])).is_none());
        assert!(store::decode(json!("x")).is_none());
        assert!(store::decode(json!(null)).is_none());
        // objet partiel : défauts
        assert_eq!(store::decode(json!({})), Some(Series::new()));
        assert_eq!(store::decode(json!({"x": 1})), Some(Series::new()));
        // tableau `players` de mauvaise longueur
        assert!(store::decode(json!({"blue": {"players": [{}, {}, {}]}})).is_none());
        // valeurs d'enum inconnues
        assert!(store::decode(json!({"scene": "nope"})).is_none());
        assert!(store::decode(json!({"format": "nope"})).is_none());
        assert!(store::decode(json!({"pool": [{"name":"A","status":"banned","by":"green"}]})).is_none());
    }

    #[tokio::test]
    async fn load_of_unreadable_row_returns_none() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        sqlx::query("INSERT INTO cast_live (id, state) VALUES (1, $1) ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state")
            .bind(json!({"scene": "nope", "blue": {"players": [{}, {}, {}]}}))
            .execute(&state.db)
            .await
            .unwrap();
        assert!(store::load(&state.db).await.is_none());
        sqlx::query("UPDATE cast_live SET state = '[1]'::jsonb WHERE id = 1").execute(&state.db).await.unwrap();
        assert!(store::load(&state.db).await.is_none());
    }

    #[tokio::test]
    async fn scene_switch_to_overlay_poll_under_500ms() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let t0 = Instant::now();
        call(&state, "POST", "/api/cast/scene", Some(json!({"scene":"series"}))).await;
        let (_, v) = call(&state, "GET", "/api/cast", None).await;
        assert_eq!(v["scene"], "series");
        assert!(t0.elapsed() < Duration::from_millis(500), "{:?}", t0.elapsed());
    }
}
