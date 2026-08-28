//! Bibliothèque de builds de talents. `picks` reprend la forme du parser
//! (`{TierNChoice: talentTreeId}`), ce qui permet d'importer un build depuis une partie jouée et de
//! diffuser « prévu vs pris » sans conversion. L'unicité du build par défaut est tenue par un index
//! partiel unique en base, pas par ce code.
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

/// Les mutations de la bibliothèque suivent la même garde que teams/collections : ouvertes si
/// aucun `ADMIN_TOKEN` n'est configuré (mode local par défaut de la spec suite), protégées sinon.
/// La lecture reste toujours ouverte, comme `list_teams`.
fn refus_admin() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin token requis" })),
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Filtre optionnel sur le héros (clé `dim_heroes.id`).
    pub hero: Option<String>,
}

/// `GET /api/builds` — la bibliothèque, éventuellement filtrée par héros.
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
    /// Provenance : `match_id` du match d'où le build a été importé (`from_match`). `None` pour
    /// un build créé directement via `POST /api/builds` — absent du payload, `#[serde(default)]`
    /// le laisse à `null`. `PUT /api/builds/{id}` (`update`, plus bas) ne liste pas cette colonne
    /// dans son `UPDATE` : ce champ du payload est donc ignoré à la modification, et la
    /// provenance déjà en base est conservée telle quelle, quoi que porte `source_match_id` ici.
    #[serde(default)]
    pub source_match_id: Option<i64>,
}

/// `POST /api/builds` — créer un build. Marquer `is_default` retire d'abord le défaut existant du
/// même héros : sans ça, l'index partiel unique rejetterait l'insertion.
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

/// `PUT /api/builds/{id}` — remplacer un build. Même précaution sur le défaut que `create`.
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
        sqlx::query("UPDATE builds SET is_default = false WHERE hero_id = $1 AND is_default AND id <> $2")
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
        // Ligne inexistante : on abandonne la transaction (rollback implicite au drop de `tx`)
        // plutôt que de committer le démarquage du défaut fait plus haut. Sans ça, un PUT sur un
        // id inconnu répondait 404 tout en ayant persisté la perte du défaut du héros.
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "build inconnu" }))));
    }
    tx.commit().await.map_err(db_err)?;
    Ok(Json(json!({ "ok": true })))
}

/// `DELETE /api/builds/{id}`.
pub async fn delete(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Resp {
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
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "build inconnu" }))));
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

/// `POST /api/builds/from-match` — amorcer un build depuis une partie jouée. C'est ce qui évite de
/// saisir 90 héros à la main : les talents de l'archive ont déjà la bonne forme.
pub async fn from_match(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<FromMatchBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(refus_admin());
    }
    // `hero` est nullable (parties IA/quittées avant pick) : décoder en `String` ferait échouer
    // sqlx sur ces lignes-là et remonterait un 500 générique sur une action utilisateur explicite
    // au lieu du 422 attendu.
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
