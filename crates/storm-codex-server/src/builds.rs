//! Talent build library. `picks` reuses the parser's shape
//! (`{TierNChoice: talentTreeId}`), which allows importing a build from a played game and
//! broadcasting "planned vs taken" without conversion. The uniqueness of the default build is held by a
//! partial unique index in the database, not by this code.
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value as J};

use crate::AppState;

type Resp = Result<Json<J>, (StatusCode, Json<J>)>;

fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

/// Library mutations follow the same guard as teams/collections: open if
/// no `ADMIN_TOKEN` is configured (default local mode of the follow-up spec), protected otherwise.
/// Reading always stays open, like `list_teams`.
fn refus_admin() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin token requis" })),
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Optional filter on the hero (`dim_heroes.id` key).
    pub hero: Option<String>,
}

/// `GET /api/builds` — the library, optionally filtered by hero.
pub async fn list(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(b ORDER BY b.hero_id, b.is_default DESC, b.name), '[]'::jsonb)
         FROM (
            SELECT id, hero_id, name, picks, notes, is_default, source_match_id, updated_at
            FROM builds
            WHERE $1::text IS NULL OR hero_id = $1
         ) b",
    )
    .bind(q.hero)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct BuildBody {
    pub hero_id: String,
    pub name: String,
    pub picks: J,
    pub notes: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    /// Provenance: `match_id` of the match the build was imported from (`from_match`). `None` for
    /// a build created directly via `POST /api/builds` — absent from the payload, `#[serde(default)]`
    /// leaves it at `null`. `PUT /api/builds/{id}` (`update`, below) does not list this column
    /// in its `UPDATE`: this payload field is therefore ignored on modification, and the
    /// provenance already in the database is kept as is, whatever `source_match_id` carries here.
    #[serde(default)]
    pub source_match_id: Option<i64>,
}

/// `POST /api/builds` — create a build. Marking `is_default` first removes the existing default of the
/// same hero: without that, the partial unique index would reject the insertion.
pub async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<BuildBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let mut tx = s.db.begin().await.map_err(db_err)?;
    if b.is_default {
        sqlx::query("UPDATE builds SET is_default = false WHERE hero_id = $1 AND is_default")
            .bind(&b.hero_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO builds (hero_id, name, picks, notes, is_default, source_match_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(&b.hero_id)
    .bind(&b.name)
    .bind(&b.picks)
    .bind(&b.notes)
    .bind(b.is_default)
    .bind(b.source_match_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(json!({ "id": id })))
}

/// `PUT /api/builds/{id}` — replace a build. Same precaution on the default as `create`.
pub async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<BuildBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let mut tx = s.db.begin().await.map_err(db_err)?;
    if b.is_default {
        sqlx::query(
            "UPDATE builds SET is_default = false WHERE hero_id = $1 AND is_default AND id <> $2",
        )
        .bind(&b.hero_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    let n = sqlx::query(
        "UPDATE builds SET hero_id = $2, name = $3, picks = $4, notes = $5, is_default = $6,
                updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&b.hero_id)
    .bind(&b.name)
    .bind(&b.picks)
    .bind(&b.notes)
    .bind(b.is_default)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        // Missing row: we abandon the transaction (implicit rollback when `tx` drops)
        // rather than committing the default unmarking done above. Without that, a PUT on an
        // unknown id answered 404 while having persisted the loss of the hero's default.
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "build inconnu" })),
        ));
    }
    tx.commit().await.map_err(db_err)?;
    Ok(Json(json!({ "ok": true })))
}

/// `DELETE /api/builds/{id}`.
pub async fn delete(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    let n = sqlx::query("DELETE FROM builds WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "build inconnu" })),
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct FromMatchBody {
    pub match_id: i64,
    pub toon_handle: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
}

/// `POST /api/builds/from-match` — seed a build from a played game. This is what avoids
/// entering 90 heroes by hand: the archive's talents already have the right shape.
pub async fn from_match(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<FromMatchBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    // `hero` is nullable (AI games/left before pick): decoding into `String` would make
    // sqlx fail on those rows and surface a generic 500 on an explicit user action
    // instead of the expected 422.
    let row: Option<(Option<String>, J)> = sqlx::query_as(
        "SELECT hero, COALESCE(data -> 'talents', '{}'::jsonb)
         FROM match_players WHERE match_id = $1 AND toon_handle = $2",
    )
    .bind(b.match_id)
    .bind(&b.toon_handle)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;

    let Some((hero_id, picks)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "joueur absent de ce match" })),
        ));
    };
    let Some(hero_id) = hero_id else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "héros non renseigné pour ce joueur sur ce match" })),
        ));
    };
    if picks.as_object().is_none_or(serde_json::Map::is_empty) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "aucun talent enregistré pour ce joueur" })),
        ));
    }

    create(
        State(s),
        headers,
        Json(BuildBody {
            hero_id,
            name: b.name,
            picks,
            notes: None,
            is_default: b.is_default,
            source_match_id: Some(b.match_id),
        }),
    )
    .await
}
