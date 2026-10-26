//! Routes REST de la régie de cast. Chaque mutation prend le write-lock une seule fois (sur une
//! copie : une erreur ne change jamais l'état), puis persiste et diffuse `cast.updated` sur le WS
//! (les clients refetch `GET /api/cast`). Mode ouvert comme `/api/draft`, pas d'auth.
use super::{store, CastError, LineupMode, MapStatus, Scene, Series, Team};
use crate::draft::{Format, Side};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
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
    mutate_ex(s, gated, false, f).await
}

/// Comme `mutate` ; `sync` : recopie ensuite équipes/pseudos/score/Bo dans le draft (sens unique
/// série → draft) et persiste/diffuse les deux états. Les locks cast et draft ne sont jamais
/// tenus en même temps (le write-lock cast est relâché avant de prendre celui du draft).
async fn mutate_ex<T>(
    s: &AppState,
    gated: bool,
    sync: bool,
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
    if sync {
        {
            let mut d = s.draft.write().await;
            super::sync::sync_into_draft(&out.1, &mut d);
        }
        crate::draft::api::persist(s).await;
    }
    persist(s).await;
    Ok(out)
}

/// Mutation qui répond par la série complète.
async fn mutate_series(s: &AppState, gated: bool, f: impl FnOnce(&mut Series) -> Result<(), CastError>) -> Resp {
    mutate_series_ex(s, gated, false, f).await
}

async fn mutate_series_ex(
    s: &AppState,
    gated: bool,
    sync: bool,
    f: impl FnOnce(&mut Series) -> Result<(), CastError>,
) -> Resp {
    match mutate_ex(s, gated, sync, f).await {
        Ok(((), series)) => series_json(&series),
        Err(r) => r,
    }
}

/// GET /api/cast — état courant.
pub async fn get_cast(State(s): State<AppState>) -> Json<Series> {
    Json(s.cast.read().await.clone())
}

#[derive(Deserialize)]
pub struct SeriesBody {
    /// Équipe complète (nom, bilan, joueurs, capitaine, logo) ; `{name}` seul reste accepté.
    #[serde(default)]
    blue: Team,
    #[serde(default)]
    red: Team,
    bo: u8,
    format: Format,
    #[serde(default)]
    pool: Vec<String>,
}

/// POST /api/cast/series — crée/redémarre : parties, score et diaporama remis à zéro (la scène
/// courante est conservée), `active = true`.
pub async fn start_series(State(s): State<AppState>, Json(b): Json<SeriesBody>) -> Resp {
    if !matches!(b.bo, 1 | 3 | 5 | 7) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "bad_bo" })));
    }
    mutate_series_ex(&s, false, true, |c| {
        let mut n = Series::new();
        n.active = true;
        // les logos (stockés en base) survivent au redémarrage de série
        // logo absent du corps → on garde celui en place (stocké en base)
        let (old_blue, old_red) = (c.blue.logo.take(), c.red.logo.take());
        n.blue = Team { logo: b.blue.logo.or(old_blue), ..b.blue };
        n.red = Team { logo: b.red.logo.or(old_red), ..b.red };
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
    mutate_series_ex(&s, true, true, |c| {
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
    mutate_series_ex(&s, true, true, |c| {
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
    mutate_series_ex(&s, true, true, |c| {
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
        // game: 0 = désépingler (« dernière partie ») ; les numéros de partie commencent à 1.
        // Autre valeur : posée telle quelle (pas de validation d'existence, comme avant).
        if let Some(g) = b.game {
            c.slide.game = if g == 0 { None } else { Some(g) };
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

/// Taille maximale d'un logo (octets).
const LOGO_MAX: usize = 512 * 1024;
/// Limite de corps de la route d'upload : un peu au-dessus de `LOGO_MAX` pour que le handler
/// réponde lui-même 413 proprement (au-delà, l'extracteur répond 413).
pub const LOGO_BODY_LIMIT: usize = LOGO_MAX + 1024;
const LOGO_MIMES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

fn side_str(side: &str) -> Option<&'static str> {
    match side {
        "blue" => Some("blue"),
        "red" => Some("red"),
        _ => None,
    }
}

fn err(code: StatusCode, e: &str) -> Resp {
    (code, Json(json!({ "error": e })))
}

/// POST /api/cast/logo/{side} — corps = octets de l'image, `Content-Type` png/jpeg/webp, 1..=512 Ko.
/// Stocke en base et pose `Team.logo = /api/cast/logo/{side}?v={updated_at_ms}`. Autorisé série inactive.
pub async fn upload_logo(State(s): State<AppState>, Path(side): Path<String>, headers: HeaderMap, body: Bytes) -> Resp {
    let Some(side) = side_str(&side) else { return err(StatusCode::NOT_FOUND, "bad_side") };
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !LOGO_MIMES.contains(&mime.as_str()) {
        return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_mime");
    }
    if body.is_empty() {
        return err(StatusCode::BAD_REQUEST, "empty_body");
    }
    if body.len() > LOGO_MAX {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "logo_too_large");
    }
    let version: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO cast_logo (side, mime, bytes, updated_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (side) DO UPDATE SET mime = EXCLUDED.mime, bytes = EXCLUDED.bytes, updated_at = now()
         RETURNING (floor(extract(epoch from updated_at) * 1000))::bigint",
    )
    .bind(side)
    .bind(&mime)
    .bind(body.as_ref())
    .fetch_one(&s.db)
    .await;
    let version = match version {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("cast logo save: {e}");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    let url = format!("/api/cast/logo/{side}?v={version}");
    mutate_series(&s, false, |c| {
        let team = if side == "blue" { &mut c.blue } else { &mut c.red };
        team.logo = Some(url);
        Ok(())
    })
    .await
}

/// GET /api/cast/logo/{side} — octets du logo (404 si aucun).
pub async fn get_logo(State(s): State<AppState>, Path(side): Path<String>) -> Response {
    let Some(side) = side_str(&side) else { return err(StatusCode::NOT_FOUND, "bad_side").into_response() };
    let row: Result<Option<(String, Vec<u8>)>, sqlx::Error> =
        sqlx::query_as("SELECT mime, bytes FROM cast_logo WHERE side = $1").bind(side).fetch_optional(&s.db).await;
    match row {
        Ok(Some((mime, bytes))) => {
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=86400".to_string())], bytes).into_response()
        }
        Ok(None) => err(StatusCode::NOT_FOUND, "no_logo").into_response(),
        Err(e) => {
            tracing::error!("cast logo load: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "db").into_response()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, call_raw, test_state};
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
    async fn post_series_accepts_full_teams_and_players() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let body = json!({
            "blue":{"name":"Blue Co","record":"4-1","captain":2,
                    "players":[{"name":"P1","tagline":"t1"},{"name":"P2"},{"name":"P3"},{"name":"P4"},{"name":"P5"}]},
            "red":{"name":"Red Co"},"bo":3,"format":"standard","pool":["A","B"]});
        let (st, v) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["blue"]["record"], "4-1");
        assert_eq!(v["blue"]["captain"], 2);
        assert_eq!(v["blue"]["players"][0]["name"], "P1");
        assert_eq!(v["blue"]["players"][0]["tagline"], "t1");
        assert_eq!(v["blue"]["players"][4]["name"], "P5");
        assert_eq!(v["red"]["name"], "Red Co");
        assert_eq!(v["red"]["players"].as_array().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn post_series_with_name_only_still_works_and_keeps_logos() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        {
            let mut c = state.cast.write().await;
            c.blue.logo = Some("/api/cast/logo/blue?v=1".into());
        }
        let (st, v) = call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["blue"]["name"], "Blue Co");
        assert_eq!(v["blue"]["logo"], "/api/cast/logo/blue?v=1");
        assert_eq!(v["red"]["logo"], J::Null);
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
    async fn slide_game_zero_clears_pinned_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let (_, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"game":2}))).await;
        assert_eq!(v["slide"]["game"], 2);
        let (st, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"game":0,"auto":true}))).await;
        assert_eq!(st, StatusCode::OK);
        assert!(v["slide"]["game"].is_null());
        assert_eq!(v["slide"]["auto"], true);
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

    const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 1];

    async fn logo_rows(state: &AppState) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM cast_logo").fetch_one(&state.db).await.unwrap()
    }

    async fn clear_logos(state: &AppState) {
        sqlx::query("DELETE FROM cast_logo").execute(&state.db).await.unwrap();
    }

    #[tokio::test]
    async fn logo_upload_stores_and_serves_same_bytes() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        clear_logos(&state).await;
        let (st, bytes) = call_raw(&state, "POST", "/api/cast/logo/blue", Some("image/png"), PNG.to_vec()).await;
        assert_eq!(st, StatusCode::OK);
        let v: J = serde_json::from_slice(&bytes).unwrap();
        let url = v["blue"]["logo"].as_str().unwrap().to_string();
        assert!(url.starts_with("/api/cast/logo/blue?v="), "{url}");
        assert_eq!(state.cast.read().await.blue.logo.as_deref(), Some(url.as_str()));
        let (st, got) = call_raw(&state, "GET", &url, None, vec![]).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(got, PNG.to_vec());
        let (st, _) = call_raw(&state, "GET", "/api/cast/logo/red", None, vec![]).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        // le logo survit au redémarrage de série
        let (_, v) = call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        assert_eq!(v["blue"]["logo"].as_str(), Some(url.as_str()));
        clear_logos(&state).await;
    }

    #[tokio::test]
    async fn logo_over_512kb_is_413() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        clear_logos(&state).await;
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/red", Some("image/png"), vec![1; 512 * 1024 + 1]).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/red", Some("image/png"), vec![1; 700 * 1024]).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(logo_rows(&state).await, 0);
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/red", Some("image/png"), vec![1; 512 * 1024]).await;
        assert_eq!(st, StatusCode::OK);
        clear_logos(&state).await;
    }

    #[tokio::test]
    async fn logo_wrong_mime_is_415() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        clear_logos(&state).await;
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/blue", Some("image/gif"), PNG.to_vec()).await;
        assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/blue", None, PNG.to_vec()).await;
        assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(logo_rows(&state).await, 0);
    }

    #[tokio::test]
    async fn logo_empty_body_is_400() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        clear_logos(&state).await;
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/blue", Some("image/png"), vec![]).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/green", Some("image/png"), PNG.to_vec()).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        assert_eq!(logo_rows(&state).await, 0);
    }

    #[tokio::test]
    async fn post_series_with_bad_bo_is_400_and_leaves_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let before = state.cast.read().await.clone();
        for bo in [0, 2, 4, 9] {
            let mut b = start_body();
            b["bo"] = json!(bo);
            let (st, v) = call(&state, "POST", "/api/cast/series", Some(b)).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "bo={bo}");
            assert_eq!(v["error"], "bad_bo");
        }
        assert_eq!(*state.cast.read().await, before);
        let mut b = start_body();
        b["bo"] = json!(7);
        assert_eq!(call(&state, "POST", "/api/cast/series", Some(b)).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn mutations_sync_into_draft_and_broadcast() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        {
            let d = state.draft.read().await;
            assert_eq!((d.blue.name.as_str(), d.red.name.as_str(), d.bo, d.score), ("Blue Co", "Red Co", 5, [0, 0]));
        }
        let team = json!({"name":"T","players":[{"name":"p1"},{"name":"p2"},{},{},{}]});
        call(&state, "PUT", "/api/cast/teams", Some(json!({"blue":team,"red":{"name":"R"}}))).await;
        call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"A","status":"picked","by":"blue"}))).await;
        call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"A"}))).await;
        let mut rx = state.events.subscribe();
        call(&state, "POST", "/api/cast/games/1/winner", Some(json!({"side":"red"}))).await;
        {
            let d = state.draft.read().await;
            assert_eq!(d.blue.name, "T");
            assert_eq!(d.blue.players[1], "p2");
            assert_eq!(d.score, [0, 1]);
        }
        let mut seen = vec![];
        while let Ok(e) = rx.try_recv() {
            seen.push(e["type"].as_str().unwrap_or("").to_string());
        }
        assert!(seen.contains(&"draft.updated".to_string()) && seen.contains(&"cast.updated".to_string()), "{seen:?}");
        call(&state, "POST", "/api/cast/score", Some(json!({"blue":3,"red":2}))).await;
        assert_eq!(state.draft.read().await.score, [3, 2]);
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
