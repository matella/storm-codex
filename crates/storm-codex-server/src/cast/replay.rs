//! POST /api/cast/games/{n}/replay — dépose le replay Custom d'une partie de la série : décodé à la
//! volée (même chemin que `draft::replay_api::load`), résumé (`summary.rs`) conservé dans l'état de
//! la série. **Rien n'est écrit en base** hors de la ligne `cast_live`.
use super::api::{conflict, mutate_ex};
use super::summary::{summary_from_output, SummaryError};
use super::{CastError, GameSummary};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value as J};

type Resp = (StatusCode, Json<J>);

fn unprocessable(code: &str, message: String) -> Resp {
    (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": code, "message": message })))
}

/// Décode les octets d'un `.StormReplay` en résumé (fichier temporaire supprimé dans tous les cas).
pub(super) async fn decode(bytes: &[u8]) -> Result<GameSummary, Resp> {
    let path = std::env::temp_dir().join(format!("cast-replay-{}.StormReplay", uuid::Uuid::new_v4()));
    if std::fs::write(&path, bytes).is_err() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io" }))));
    }
    let p2 = path.clone();
    let parsed = tokio::task::spawn_blocking(move || storm_stats::process_replay(&p2, &p2.to_string_lossy())).await;
    let _ = std::fs::remove_file(&path);
    let Ok(out) = parsed else {
        return Err(unprocessable("parse_failed", "le décodeur a échoué".into()));
    };
    summary_from_output(&out).map_err(|e| {
        let code = match e {
            SummaryError::NotCustom => "not_custom",
            SummaryError::Parse(_) => "parse_failed",
            SummaryError::Missing(_) => "incomplete",
        };
        unprocessable(code, e.to_string())
    })
}

/// Remplit la partie `n` depuis le résumé (vainqueur, takedowns, durée, résumé — le replay fait foi,
/// y compris sur un vainqueur saisi à la main), persiste, synchronise le draft, diffuse `cast.updated`.
pub(super) async fn apply(s: &AppState, n: u8, summary: GameSummary) -> Resp {
    let body = serde_json::to_value(&summary).unwrap_or(J::Null);
    match mutate_ex(s, true, true, |c| c.fill_from_summary(n, summary)).await {
        Ok(_) => (StatusCode::CREATED, Json(body)),
        Err(r) => r,
    }
}

/// POST /api/cast/games/{n}/replay — corps = octets bruts du replay. 201 `GameSummary` ;
/// 400 `empty_body` ; 409 `SeriesInactive` / `UnknownGame` ; 422 `not_custom` / `parse_failed` / `incomplete`.
pub async fn load_game_replay(State(s): State<AppState>, Path(n): Path<u8>, bytes: Bytes) -> Resp {
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "empty_body" })));
    }
    {
        let c = s.cast.read().await;
        if !c.active {
            return conflict(CastError::SeriesInactive);
        }
        if !c.games.iter().any(|g| g.number == n) {
            return conflict(CastError::UnknownGame);
        }
    }
    match decode(&bytes).await {
        Ok(summary) => apply(&s, n, summary).await,
        Err(r) => r,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, call_raw, test_state};
    use crate::cast::TEST_DB_LOCK as DB_LOCK;
    use crate::draft::Side;

    const MAP: &str = "Tomb of the Spider Queen";

    fn aram() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../storm-stats/tests/data/silver-city-aram.StormReplay")).unwrap()
    }

    /// Replay Custom réel facultatif (non committé : pseudos réels) — `STORM_CAST_CUSTOM_REPLAY`.
    fn real_custom() -> Option<Vec<u8>> {
        let p = std::env::var_os("STORM_CAST_CUSTOM_REPLAY")?;
        std::fs::read(p).ok()
    }

    fn fixture_summary() -> GameSummary {
        let fx: J = serde_json::from_str(include_str!("fixtures/custom_summary.json")).unwrap();
        let out = storm_stats::Output { status: 1, match_: fx["match"].as_object().cloned(), players: fx["players"].as_object().cloned() };
        summary_from_output(&out).unwrap()
    }

    /// Série active + partie 1 lancée sur `MAP`.
    async fn with_game(state: &AppState) {
        let body = json!({"blue":{"name":"Blue Co"},"red":{"name":"Red Co"},"bo":3,"format":"fearless","pool":[MAP, "Sky Temple"]});
        assert_eq!(call(state, "POST", "/api/cast/series", Some(body)).await.0, StatusCode::OK);
        call(state, "POST", "/api/cast/pool/map", Some(json!({"name":MAP,"status":"picked","by":"blue"}))).await;
        let (_, g) = call(state, "POST", "/api/cast/games/start", Some(json!({"map":MAP}))).await;
        assert_eq!(g["number"], 1);
    }

    async fn post(state: &AppState, n: u8, bytes: Vec<u8>) -> (StatusCode, J) {
        let (st, b) = call_raw(state, "POST", &format!("/api/cast/games/{n}/replay"), Some("application/octet-stream"), bytes).await;
        (st, serde_json::from_slice(&b).unwrap_or(J::Null))
    }

    async fn rows(state: &AppState, fp: &str) -> (i64, i64) {
        let m = sqlx::query_scalar("SELECT count(*) FROM matches WHERE fingerprint = $1").bind(fp).fetch_one(&state.db).await.unwrap();
        let u = sqlx::query_scalar("SELECT count(*) FROM uploads WHERE fingerprint = $1").bind(fp).fetch_one(&state.db).await.unwrap();
        (m, u)
    }

    fn fingerprint(bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("cast-fp-{}.StormReplay", uuid::Uuid::new_v4()));
        std::fs::write(&dir, bytes).unwrap();
        let out = storm_stats::process_replay(&dir, "x.StormReplay");
        let _ = std::fs::remove_file(&dir);
        crate::upload::game_fingerprint(&out).unwrap()
    }

    #[tokio::test]
    async fn replay_route_empty_body_is_400() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        let before = state.cast.read().await.clone();
        let (st, v) = post(&state, 1, vec![]).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "empty_body");
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn replay_route_garbage_bytes_is_422_not_500() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        let before = state.cast.read().await.clone();
        let a = aram();
        let half = a[..a.len() / 2].to_vec();
        for (bytes, code) in [(vec![7u8; 64], "parse_failed"), (half, "parse_failed"), (a, "not_custom")] {
            let (st, v) = post(&state, 1, bytes).await;
            assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
            assert_eq!(v["error"], code);
            assert!(v["message"].is_string());
        }
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn replay_route_needs_active_series_and_known_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        *state.cast.write().await = crate::cast::Series::new();
        let (st, v) = post(&state, 1, vec![7u8; 64]).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("SeriesInactive")));
        with_game(&state).await;
        let (st, v) = post(&state, 9, vec![7u8; 64]).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("UnknownGame")));
    }

    #[tokio::test]
    async fn replay_route_writes_nothing_to_matches() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        let mut cases = vec![aram()];
        cases.extend(real_custom());
        for bytes in cases {
            let fp = fingerprint(&bytes);
            let before = rows(&state, &fp).await;
            let (st, _) = post(&state, 1, bytes).await;
            assert!(st == StatusCode::CREATED || st == StatusCode::UNPROCESSABLE_ENTITY, "{st}");
            assert_eq!(rows(&state, &fp).await, before);
        }
    }

    /// Chemin de remplissage (toujours exécuté) : résumé du fixture réel → `GameRecord`, score,
    /// synchro draft, diffusion. Avec `STORM_CAST_CUSTOM_REPLAY`, le même contrôle passe aussi par
    /// la route avec les octets réels.
    #[tokio::test]
    async fn replay_route_fills_game_record() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        with_game(&state).await;
        // un vainqueur saisi à la main est remplacé par celui du replay
        call(&state, "POST", "/api/cast/games/1/winner", Some(json!({"side":"red"}))).await;
        let mut rx = state.events.subscribe();
        let (st, v) = apply(&state, 1, fixture_summary()).await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(v.0["takedowns"], json!([28, 15]));
        let c = state.cast.read().await.clone();
        let g = &c.games[0];
        assert_eq!(g.winner, Some(Side::Blue));
        assert_eq!(g.takedowns, [28, 15]);
        assert_eq!(g.duration_secs, Some(1576));
        assert_eq!(g.summary.as_ref().map(|s| s.players.len()), Some(10));
        assert_eq!(state.draft.read().await.score, [1, 0]);
        assert_eq!(crate::cast::store::load(&state.db).await, Some(c));
        let mut seen = vec![];
        while let Ok(e) = rx.try_recv() {
            seen.push(e["type"].as_str().unwrap_or("").to_string());
        }
        assert!(seen.contains(&"cast.updated".to_string()), "{seen:?}");
        // partie inconnue : 409, rien ne change
        let before = state.cast.read().await.clone();
        assert_eq!(apply(&state, 7, fixture_summary()).await.0, StatusCode::CONFLICT);
        assert_eq!(*state.cast.read().await, before);

        if let Some(bytes) = real_custom() {
            with_game(&state).await;
            let t0 = std::time::Instant::now();
            let (st, v) = post(&state, 1, bytes).await;
            eprintln!("cast replay route (real Custom bytes): {:?}", t0.elapsed());
            assert_eq!(st, StatusCode::CREATED, "{v}");
            let c = state.cast.read().await.clone();
            let g = &c.games[0];
            let s = g.summary.as_ref().unwrap();
            assert_eq!(g.winner, Some(s.winner));
            assert_eq!(g.takedowns, s.takedowns);
            assert_eq!(g.duration_secs, Some(s.duration_secs));
            assert_eq!(s.players.len(), 10);
            assert_eq!(v["players"].as_array().map(Vec::len), Some(10));
        }
    }
}
