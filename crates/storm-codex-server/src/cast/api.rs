//! Routes REST de la régie de cast. Chaque mutation prend le write-lock une seule fois (sur une
//! copie : une erreur ne change jamais l'état), puis persiste et diffuse `cast.updated` sur le WS
//! (les clients refetch `GET /api/cast`). Mode ouvert comme `/api/draft`, pas d'auth.
use super::{lineup, store, CastError, LineupMode, MapStatus, Scene, Series, Team};
use crate::draft::{Format, Side};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

type Resp = (StatusCode, Json<J>);

/// Bornes de l'intervalle du diaporama (ms).
const INTERVAL_RANGE: std::ops::RangeInclusive<u32> = 500..=3_600_000;

/// Version suivante de l'état en mémoire — appelée sous le write-lock de la mutation qui le produit.
fn next_version(s: &AppState) -> u64 {
    s.cast_version.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// Écrit `snapshot` (l'état produit par la mutation de version `v`), sauf si une version plus
/// récente est déjà en base : deux persistances concurrentes ne peuvent jamais finir sur l'ancien
/// état (écritures sérialisées par `cast_saved`, qui retient la dernière version écrite).
async fn save_versioned(s: &AppState, snapshot: &Series, v: u64) -> Result<(), sqlx::Error> {
    let mut last = s.cast_saved.lock().await;
    if v <= *last {
        return Ok(());
    }
    store::save(&s.db, snapshot).await?;
    *last = v;
    Ok(())
}

/// Persiste l'instantané de la mutation puis diffuse `cast.updated` (toujours, même si l'écriture
/// échoue : l'état en mémoire a changé). Erreur d'écriture → `Err` (la route répond 500, R24).
pub(super) async fn persist(s: &AppState, snapshot: &Series, v: u64) -> Result<(), sqlx::Error> {
    let res = save_versioned(s, snapshot, v).await;
    if let Err(e) = &res {
        tracing::error!("cast save: {e}");
    }
    let _ = s.events.send(json!({ "type": "cast.updated" }));
    res
}

/// R24 : l'écriture en base a échoué — état appliqué en mémoire et diffusé, mais non persisté.
fn persist_failed() -> Resp {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "persist_failed" })))
}

pub(super) fn conflict(e: CastError) -> Resp {
    (StatusCode::CONFLICT, Json(json!({ "error": e.code() })))
}

/// Synchro série → draft après une mutation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DraftSync {
    No,
    /// Équipes, pseudos, score, Bo.
    Sync,
    /// `Sync` + alignement du moteur sur le format de la série (R20, `POST /api/cast/series`).
    Align,
}

/// Recopie l'état de cast **courant** (relu sous read-lock pendant que le write-lock du draft est
/// tenu) dans le draft, puis persiste le draft : une mutation concurrente plus récente ne peut pas
/// être écrasée par l'instantané d'une plus ancienne. Jamais les deux write-locks ensemble.
pub(super) async fn sync_draft(s: &AppState, align: bool) {
    {
        let mut d = s.draft.write().await;
        let c = s.cast.read().await;
        super::sync::sync_into_draft(&c, &mut d);
        if align {
            d.align_to_series(c.format);
        }
    }
    crate::draft::api::persist(s).await;
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
    mutate_ex(s, gated, DraftSync::No, f).await
}

/// Comme `mutate` ; `sync` : recopie ensuite l'état courant dans le draft (sens unique série →
/// draft, `sync_draft`) et persiste/diffuse les deux états. Les write-locks cast et draft ne sont
/// jamais tenus en même temps. Échec d'écriture de la série → `500 persist_failed` (R24 : l'état
/// reste appliqué en mémoire et `cast.updated` est diffusé).
pub(super) async fn mutate_ex<T>(
    s: &AppState,
    gated: bool,
    sync: DraftSync,
    f: impl FnOnce(&mut Series) -> Result<T, CastError>,
) -> Result<(T, Series), Resp> {
    let (out, version) = {
        let mut g = s.cast.write().await;
        if gated && !g.active {
            return Err(conflict(CastError::SeriesInactive));
        }
        let mut next = g.clone();
        let v = f(&mut next).map_err(conflict)?;
        *g = next.clone();
        ((v, next), next_version(s))
    };
    if sync != DraftSync::No {
        sync_draft(s, sync == DraftSync::Align).await;
    }
    persist(s, &out.1, version).await.map_err(|_| persist_failed())?;
    Ok(out)
}

/// Mutation qui répond par la série complète.
async fn mutate_series(s: &AppState, gated: bool, f: impl FnOnce(&mut Series) -> Result<(), CastError>) -> Resp {
    mutate_series_ex(s, gated, DraftSync::No, f).await
}

async fn mutate_series_ex(
    s: &AppState,
    gated: bool,
    sync: DraftSync,
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

/// Corps JSON des routes de la régie : 64 Kio suffisent (série complète ≈ quelques Ko) ; l'API est
/// en mode ouvert, donc on borne ce qu'un client peut faire avaler au serveur.
pub const JSON_BODY_LIMIT: usize = 64 * 1024;
/// Bornes de saisie (caractères) : pool de cartes, noms/bilans/pseudos, ligne libre, URL de logo.
const MAX_POOL: usize = 32;
const MAX_TEXT: usize = 64;
const MAX_TAGLINE: usize = 120;
const MAX_URL: usize = 512;

fn bad_input(code: &'static str) -> Resp {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": code })))
}

/// Une équipe respecte les bornes de saisie (nom, bilan, logo, pseudos, lignes libres, toons).
fn team_within_limits(t: &Team) -> bool {
    let len = |x: &str| x.chars().count();
    len(&t.name) <= MAX_TEXT
        && len(&t.record) <= MAX_TEXT
        && t.logo.as_deref().is_none_or(|l| len(l) <= MAX_URL)
        && t.players.iter().all(|p| {
            len(&p.name) <= MAX_TEXT
                && len(&p.tagline) <= MAX_TAGLINE
                && p.toon.as_deref().is_none_or(|x| len(x) <= MAX_TEXT)
        })
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
    if b.pool.len() > MAX_POOL || b.pool.iter().any(|n| n.chars().count() > MAX_TEXT) {
        return bad_input("bad_pool");
    }
    if !team_within_limits(&b.blue) || !team_within_limits(&b.red) {
        return bad_input("bad_team");
    }
    // R20 : démarrer une série remet aussi le moteur de draft sur son format/steps (partie + série
    // neuves), dans le même passage que la synchro (`DraftSync::Align`).
    mutate_series_ex(&s, false, DraftSync::Align, |c| {
        let mut n = Series::new();
        n.series_id = uuid::Uuid::new_v4().to_string();
        n.active = true;
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
    if !team_within_limits(&b.blue) || !team_within_limits(&b.red) {
        return bad_input("bad_team");
    }
    mutate_series_ex(&s, true, DraftSync::Sync, |c| {
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
    mutate_series_ex(&s, true, DraftSync::Sync, |c| {
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

/// DELETE /api/cast/games/{n} — retire la dernière partie lancée par erreur (R21) : sans vainqueur
/// ni résumé (sinon 409 `GameHasResult`), la dernière seulement (409 `NotLastGame`). La carte garde
/// son statut et redevient lançable ; synchro draft comme `/winner`.
pub async fn remove_game(State(s): State<AppState>, Path(n): Path<u8>) -> Resp {
    mutate_series_ex(&s, true, DraftSync::Sync, |c| c.remove_game(n)).await
}

#[derive(Deserialize)]
pub struct ScoreBody {
    blue: u8,
    red: u8,
}

/// POST /api/cast/score `{blue, red}` (override) ou `null` (retour au score calculé).
pub async fn set_score(State(s): State<AppState>, Json(b): Json<Option<ScoreBody>>) -> Resp {
    mutate_series_ex(&s, true, DraftSync::Sync, |c| {
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

/// Index de slide maximal accepté (R25) ; l'overlay le prend modulo le nombre de slides.
const SLIDE_INDEX_MAX: u8 = 99;

/// Horloge murale du serveur (ms depuis l'epoch) : ancre de la rotation auto du diaporama.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// POST /api/cast/slide — mise à jour partielle du diaporama (champs absents inchangés).
/// R25 : `index` ≤ 99 sinon `400 bad_index` ; `game` = 0 (désépingler) ou un numéro de partie
/// existant, sinon `409 UnknownGame` (état inchangé). La garde `SeriesInactive` (409) passe avant la
/// validation de `index` (400).
pub async fn set_slide(State(s): State<AppState>, Json(b): Json<SlideBody>) -> Resp {
    if !s.cast.read().await.active {
        return conflict(CastError::SeriesInactive);
    }
    if b.index.is_some_and(|i| i > SLIDE_INDEX_MAX) {
        return err(StatusCode::BAD_REQUEST, "bad_index");
    }
    mutate_series(&s, true, |c| {
        // game: 0 = désépingler (« dernière partie ») ; les numéros de partie commencent à 1.
        if let Some(g) = b.game {
            if g != 0 && !c.games.iter().any(|x| x.number == g) {
                return Err(CastError::UnknownGame);
            }
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
        // toute modification repart de la slide montrée à cet instant (rotation auto relancée)
        c.slide.since_ms = now_ms();
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

/// Les octets commencent bien par la signature du type déclaré (PNG `89 50 4E 47`, JPEG `FF D8 FF`,
/// WebP `RIFF....WEBP`) : un `Content-Type` menteur est refusé.
fn magic_matches(mime: &str, b: &[u8]) -> bool {
    match mime {
        "image/png" => b.starts_with(&[0x89, b'P', b'N', b'G']),
        "image/jpeg" => b.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/webp" => b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP",
        _ => false,
    }
}

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
    if !magic_matches(&mime, &body) {
        return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "mime_mismatch");
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

/// GET /api/cast/logo/{side} — octets du logo (404 si aucun), `X-Content-Type-Options: nosniff`.
pub async fn get_logo(State(s): State<AppState>, Path(side): Path<String>) -> Response {
    let Some(side) = side_str(&side) else { return err(StatusCode::NOT_FOUND, "bad_side").into_response() };
    let row: Result<Option<(String, Vec<u8>)>, sqlx::Error> =
        sqlx::query_as("SELECT mime, bytes FROM cast_logo WHERE side = $1").bind(side).fetch_optional(&s.db).await;
    match row {
        Ok(Some((mime, bytes))) => {
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(None) => err(StatusCode::NOT_FOUND, "no_logo").into_response(),
        Err(e) => {
            tracing::error!("cast logo load: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "db").into_response()
        }
    }
}

#[derive(Deserialize, Default)]
pub struct LineupQuery {
    #[serde(default)]
    blue: String,
    #[serde(default)]
    red: String,
}

/// Longueur maximale d'un toon reçu par la route lineup (octets).
const TOON_MAX: usize = 64;

/// Une équipe : toons séparés par des virgules, position conservée (champ vide = joueur non lié).
/// `None` si une entrée dépasse `TOON_MAX`.
fn split_toons(s: &str) -> Option<Vec<String>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    s.split(',').map(str::trim).map(|t| (t.len() <= TOON_MAX).then(|| t.to_owned())).collect()
}

/// GET /api/cast/lineup?blue=toon,...&red=toon,... : `{ blue: [[HeroCount]], red: [[HeroCount]] }`,
/// une liste par joueur dans l'ordre reçu (vide si non lié ou < 5 parties). Une seule requête SQL.
pub async fn get_lineup(State(s): State<AppState>, Query(q): Query<LineupQuery>) -> Resp {
    let (Some(blue), Some(red)) = (split_toons(&q.blue), split_toons(&q.red)) else {
        return err(StatusCode::BAD_REQUEST, "bad_toon");
    };
    if blue.len() > lineup::MAX_PER_SIDE || red.len() > lineup::MAX_PER_SIDE {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "too_many_players" })));
    }
    let all: Vec<String> = blue.iter().chain(red.iter()).cloned().collect();
    match lineup::recent_heroes_many(&s.db, &all).await {
        Ok(mut v) => {
            let red_part = v.split_off(blue.len());
            (StatusCode::OK, Json(json!({ "blue": v, "red": red_part })))
        }
        Err(e) => {
            tracing::error!("cast lineup: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" })))
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

    use crate::cast::TEST_DB_LOCK as DB_LOCK;

    fn start_body() -> J {
        json!({"blue":{"name":"Blue Co"},"red":{"name":"Red Co"},"bo":5,"format":"fearless","pool":["A","B","C"]})
    }

    #[tokio::test]
    async fn post_series_rejects_oversized_pool_and_long_names_without_changing_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let before = call(&state, "GET", "/api/cast", None).await.1;
        let too_many: Vec<String> = (0..33).map(|i| format!("Map{i}")).collect();
        let mut body = start_body();
        body["pool"] = json!(too_many);
        let (st, v) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_pool")));
        let mut body = start_body();
        body["pool"] = json!(["x".repeat(65)]);
        let (st, v) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_pool")));
        let after = call(&state, "GET", "/api/cast", None).await.1;
        assert_eq!(before, after, "un refus ne change rien");
        // 32 cartes de 64 caractères : à la borne, accepté
        let mut body = start_body();
        body["pool"] = json!((0..32).map(|i| format!("{i:0>64}")).collect::<Vec<_>>());
        let (st, _) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
    }

    #[tokio::test]
    async fn teams_with_overlong_text_are_400_bad_team() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let mut body = start_body();
        body["blue"] = json!({"name": "n".repeat(65)});
        let (st, v) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_team")));
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let long_tag = json!({"name":"B","players":[{"name":"p","tagline":"t".repeat(121)},{},{},{},{}]});
        let ok_team = json!({"name":"R"});
        let (st, v) = call(&state, "PUT", "/api/cast/teams", Some(json!({"blue": long_tag, "red": ok_team}))).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_team")));
    }

    #[tokio::test]
    async fn json_body_over_64_kib_is_413() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let mut body = start_body();
        body["pool"] = json!(["z".repeat(70 * 1024)]);
        let (st, _) = call(&state, "POST", "/api/cast/series", Some(body)).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
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
    async fn start_series_aligns_draft_format_and_steps() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        {
            let mut d = state.draft.write().await;
            *d = crate::draft::DraftState::new(Format::Standard, crate::draft::Side::Red, "Sky Temple".into());
            d.apply("Muradin").unwrap();
            d.series_bans.insert("Valla".into());
            d.set_unavailable("Jaina", true);
        }
        let (st, _) = call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        assert_eq!(st, StatusCode::OK);
        let d = state.draft.read().await;
        assert_eq!(d.format, Format::Fearless);
        assert_eq!(d.steps, crate::draft::steps_for(Format::Fearless));
        assert_eq!(d.cursor, 0);
        assert!(d.assignments.iter().all(|a| a.is_none()) && d.assignments.len() == d.steps.len());
        assert!(d.series_bans.is_empty());
        assert_eq!(d.map, "Sky Temple");
        assert_eq!(d.first_pick, crate::draft::Side::Red);
        assert!(d.manual_unavailable.contains("Jaina"));
        assert_eq!(d.blue.name, "Blue Co");
    }

    #[tokio::test]
    async fn start_series_deselects_replay_game_and_stops_autoplay() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        {
            let mut d = state.draft.write().await;
            d.replay.current = Some(0);
            d.replay.playing = true;
            d.replay.next_at_ms = Some(1);
            d.replay.interval_ms = 9_000;
        }
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let d = state.draft.read().await;
        assert_eq!(d.replay.current, None);
        assert!(!d.replay.playing);
        assert_eq!(d.replay.next_at_ms, None);
        assert_eq!(d.replay.interval_ms, 9_000);
    }

    #[tokio::test]
    async fn start_series_with_same_format_still_resets_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        {
            let mut d = state.draft.write().await;
            *d = crate::draft::DraftState::new(Format::Fearless, crate::draft::Side::Blue, "Sky Temple".into());
            d.apply("Muradin").unwrap();
            d.start_next_game();
            d.apply("Jaina").unwrap();
        }
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let d = state.draft.read().await;
        assert_eq!(d.format, Format::Fearless);
        assert_eq!(d.cursor, 0);
        assert!(d.series_bans.is_empty() && d.series_games.is_empty());
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
            ("DELETE", "/api/cast/games/1", J::Null),
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
        call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":"A","status":"picked","by":"blue"}))).await;
        call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"A"}))).await;
        let (_, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"game":1,"auto":true,"interval_ms":1}))).await;
        assert_eq!(v["slide"]["game"], 1);
        assert_eq!(v["slide"]["auto"], true);
        assert_eq!(v["slide"]["interval_ms"], 500);
        assert_eq!(v["slide"]["index"], 0);
        assert!(v["slide"]["since_ms"].as_i64().is_some_and(|t| t > 0), "ancre de rotation posée");
    }

    #[tokio::test]
    async fn slide_game_zero_clears_pinned_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        for m in ["A", "B"] {
            call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":m,"status":"picked","by":"blue"}))).await;
            call(&state, "POST", "/api/cast/games/start", Some(json!({"map":m}))).await;
        }
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
        // résumé de partie illisible (ancien `{}`) : ignoré, la série reste chargeable
        let s = store::decode(json!({"games": [{"number": 1, "map": "A", "summary": {}}]})).unwrap();
        assert_eq!((s.games.len(), s.games[0].summary.is_none()), (1, true));
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
        {
            use tower::util::ServiceExt;
            let req = axum::http::Request::get(&url).body(axum::body::Body::empty()).unwrap();
            let resp = crate::api_tests::app(&state).oneshot(req).await.unwrap();
            assert_eq!(resp.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(resp.headers()[header::CONTENT_TYPE], "image/png");
        }
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
        let mut max = PNG.to_vec();
        max.resize(512 * 1024, 1);
        let (st, _) = call_raw(&state, "POST", "/api/cast/logo/red", Some("image/png"), max).await;
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

    /// Item 5 : signature des octets ≠ type déclaré → 415 `mime_mismatch`, rien n'est stocké.
    #[tokio::test]
    async fn logo_magic_bytes_must_match_declared_mime() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        clear_logos(&state).await;
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10];
        let mut webp = b"RIFF".to_vec();
        webp.extend([0x10, 0, 0, 0]);
        webp.extend(b"WEBPVP8 ");
        let svg = b"<svg onload=x>".to_vec();
        for (mime, bytes) in [("image/png", jpeg.clone()), ("image/jpeg", PNG.to_vec()), ("image/webp", PNG.to_vec()), ("image/png", svg)] {
            let (st, b) = call_raw(&state, "POST", "/api/cast/logo/blue", Some(mime), bytes).await;
            assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{mime}");
            assert_eq!(serde_json::from_slice::<J>(&b).unwrap()["error"], "mime_mismatch");
        }
        assert_eq!(logo_rows(&state).await, 0);
        for (mime, bytes) in [("image/jpeg", jpeg), ("image/webp", webp), ("image/png", PNG.to_vec())] {
            let (st, _) = call_raw(&state, "POST", "/api/cast/logo/blue", Some(mime), bytes).await;
            assert_eq!(st, StatusCode::OK, "{mime}");
        }
        clear_logos(&state).await;
    }

    /// Item 6 : une entrée de toon > 64 octets → 400 `bad_toon`.
    #[tokio::test]
    async fn lineup_rejects_overlong_toon() {
        let Some(state) = test_state(None).await else { return };
        let long = "x".repeat(65);
        let (st, v) = call(&state, "GET", &format!("/api/cast/lineup?blue=a,{long}&red="), None).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_toon")));
        let ok = "x".repeat(64);
        let (st, _) = call(&state, "GET", &format!("/api/cast/lineup?blue={ok}&red="), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(split_toons(""), Some(vec![]));
        assert_eq!(split_toons(" a , ,b"), Some(vec!["a".to_owned(), String::new(), "b".to_owned()]));
    }

    /// R25 : `game` doit exister (0 = désépingler), `index` ≤ 99 ; refus → état inchangé.
    #[tokio::test]
    async fn slide_on_inactive_series_is_409_even_with_bad_index() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        *state.cast.write().await = crate::cast::Series::new();
        let (st, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"index":100}))).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("SeriesInactive")));
    }

    #[tokio::test]
    async fn slide_rejects_unknown_game_and_index_over_99() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        let before = state.cast.read().await.clone();
        let (st, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"game":1}))).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("UnknownGame")));
        let (st, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"index":100}))).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_index")));
        assert_eq!(*state.cast.read().await, before);
        let (st, v) = call(&state, "POST", "/api/cast/slide", Some(json!({"index":99,"game":0}))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["slide"]["index"], 99);
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

    /// R21 : corriger un vainqueur (score, lineup B, synchro draft suivent) et retirer la dernière
    /// partie lancée par erreur (la carte redevient lançable) ; refus typés, état inchangé.
    #[tokio::test]
    async fn change_winner_and_remove_last_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        for (m, by) in [("A", "blue"), ("B", "red")] {
            call(&state, "POST", "/api/cast/pool/map", Some(json!({"name":m,"status":"picked","by":by}))).await;
        }
        call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"A"}))).await;
        call(&state, "POST", "/api/cast/games/1/winner", Some(json!({"side":"blue"}))).await;
        assert_eq!(state.draft.read().await.score, [1, 0]);
        let (st, v) = call(&state, "POST", "/api/cast/games/1/winner", Some(json!({"side":"red"}))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["games"][0]["winner"], "red");
        assert_eq!(state.draft.read().await.score, [0, 1]);
        assert_eq!(state.cast.read().await.lineup_mode_effective(), LineupMode::B);

        call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"B"}))).await;
        // partie 1 : pas la dernière ; partie 2 inconnue après retrait
        let (st, v) = call(&state, "DELETE", "/api/cast/games/1", None).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("NotLastGame")));
        let mut rx = state.events.subscribe();
        let (st, v) = call(&state, "DELETE", "/api/cast/games/2", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["games"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["pool"][1]["status"], "picked");
        assert_eq!(state.cast.read().await.next_map(), Some("B"));
        assert_eq!(state.draft.read().await.score, [0, 1]);
        let mut seen = vec![];
        while let Ok(e) = rx.try_recv() {
            seen.push(e["type"].as_str().unwrap_or("").to_string());
        }
        assert!(seen.contains(&"cast.updated".to_string()), "{seen:?}");
        let (st, v) = call(&state, "DELETE", "/api/cast/games/2", None).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("UnknownGame")));
        // partie 1 a un vainqueur : non retirable
        let before = state.cast.read().await.clone();
        let (st, v) = call(&state, "DELETE", "/api/cast/games/1", None).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::CONFLICT, Some("GameHasResult")));
        assert_eq!(*state.cast.read().await, before);
        // la carte B se relance avec le même numéro
        let (_, g) = call(&state, "POST", "/api/cast/games/start", Some(json!({"map":"B"}))).await;
        assert_eq!(g["number"], 2);
    }

    /// Item 1 : un instantané plus ancien (version inférieure) écrit après un plus récent est ignoré.
    #[tokio::test]
    async fn older_snapshot_never_overwrites_newer_one() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        let (mut old, mut new) = (Series::new(), Series::new());
        old.scene = Scene::Series;
        new.scene = Scene::Draft;
        persist(&state, &new, 2).await.unwrap();
        persist(&state, &old, 1).await.unwrap();
        assert_eq!(store::load(&state.db).await, Some(new.clone()));
        // même version rejouée : ignorée aussi
        persist(&state, &old, 2).await.unwrap();
        assert_eq!(store::load(&state.db).await, Some(new));
    }

    /// Items 1 + 4 : deux mutations concurrentes (avec synchro draft) — la base et le draft finissent
    /// sur l'état en mémoire le plus récent, quel que soit l'ordre des persistances.
    #[tokio::test]
    async fn interleaved_mutations_persist_and_sync_latest_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        for round in 0..20u8 {
            let (a, b) = (state.clone(), state.clone());
            let ta = tokio::spawn(async move { call(&a, "POST", "/api/cast/score", Some(json!({"blue": round, "red": 0}))).await });
            let tb = tokio::spawn(async move { call(&b, "POST", "/api/cast/score", Some(json!({"blue": 0, "red": round}))).await });
            assert_eq!(ta.await.unwrap().0, StatusCode::OK);
            assert_eq!(tb.await.unwrap().0, StatusCode::OK);
            let mem = state.cast.read().await.clone();
            assert_eq!(store::load(&state.db).await.as_ref(), Some(&mem), "round {round}");
            assert_eq!(state.draft.read().await.score, mem.score(), "round {round}");
        }
    }

    /// Item 4 : la synchro relit l'état de cast COURANT (pas l'instantané d'une mutation antérieure).
    #[tokio::test]
    async fn sync_draft_copies_current_cast_state() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        call(&state, "POST", "/api/cast/series", Some(start_body())).await;
        {
            let mut c = state.cast.write().await;
            c.score_override = Some([3, 1]);
            c.blue.name = "Newer".into();
        }
        sync_draft(&state, false).await;
        let d = state.draft.read().await;
        assert_eq!((d.score, d.blue.name.as_str()), ([3, 1], "Newer"));
    }

    /// R24 : écriture en base impossible → 500 `persist_failed`, état appliqué en mémoire, diffusé.
    #[tokio::test]
    async fn failed_save_answers_500_persist_failed_but_applies_and_broadcasts() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else { return };
        state.db.close().await;
        let mut rx = state.events.subscribe();
        let (st, v) = call(&state, "POST", "/api/cast/scene", Some(json!({"scene":"lineups"}))).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::INTERNAL_SERVER_ERROR, Some("persist_failed")));
        assert_eq!(state.cast.read().await.scene, Scene::Lineups);
        assert_eq!(rx.recv().await.unwrap(), json!({"type":"cast.updated"}));
    }

    /// Item 2 : codes d'erreur stables (contrat 05-api), indépendants du nom Rust.
    #[test]
    fn cast_error_codes_are_stable() {
        let all = [
            (CastError::UnknownMap, "UnknownMap"),
            (CastError::NotInPool, "NotInPool"),
            (CastError::DuplicateDecider, "DuplicateDecider"),
            (CastError::MapNotChosen, "MapNotChosen"),
            (CastError::UnknownGame, "UnknownGame"),
            (CastError::WinnerAlreadySet, "WinnerAlreadySet"),
            (CastError::NotLastGame, "NotLastGame"),
            (CastError::GameHasResult, "GameHasResult"),
            (CastError::MapAlreadyPlayed, "MapAlreadyPlayed"),
            (CastError::SeriesInactive, "SeriesInactive"),
            (CastError::SideMismatch, "side_mismatch"),
            (CastError::SeriesChanged, "series_changed"),
        ];
        for (e, code) in all {
            assert_eq!(e.code(), code);
            assert_eq!(conflict(e).1 .0, json!({"error": code}));
        }
    }

    /// Item 3 : un `schema_version` plus récent est signalé mais l'état reste chargeable.
    #[test]
    fn newer_schema_is_detected_and_still_loads() {
        assert_eq!(store::newer_schema(&json!({"schema_version": 99})), Some(99));
        assert_eq!(store::newer_schema(&json!({"schema_version": crate::cast::SCHEMA_VERSION})), None);
        assert_eq!(store::newer_schema(&json!({})), None);
        let s = store::decode(json!({"schema_version": 99, "bo": 3})).unwrap();
        assert_eq!((s.schema_version, s.bo), (99, 3));
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
