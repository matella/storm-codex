//! Local OBS layout storage. A save replaces the entire layout in one transaction.

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Request, State},
    http::StatusCode,
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{auth, AppState};

const BODY_LIMIT: usize = 64 * 1024;
const MAX_WIDGETS: usize = 100;
const MAX_TEXT_CHARS: usize = 120;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/admin/overlay-layouts", get(list).post(create))
        .route(
            "/api/admin/overlay-layouts/{id}",
            get(read).put(update).delete(delete),
        )
        .route_layer(DefaultBodyLimit::max(BODY_LIMIT))
        .route_layer(from_fn_with_state(state.clone(), require_admin))
        .route("/api/overlay-layouts/{id}", get(read))
}

async fn require_admin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !auth::is_admin(req.headers(), state.cfg.admin_token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "admin token requis"})),
        )
            .into_response();
    }
    next.run(req).await
}

#[derive(Debug, thiserror::Error)]
enum LayoutError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("invalid layout JSON")]
    Json(#[from] JsonRejection),
    #[error("layout not found")]
    Missing,
    #[error("db")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for LayoutError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Json(e) => match e.status() {
                StatusCode::UNPROCESSABLE_ENTITY => StatusCode::BAD_REQUEST,
                status => status,
            },
            Self::Missing => StatusCode::NOT_FOUND,
            Self::Database(e) => {
                tracing::error!(%e, "overlay layouts database error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(json!({"error": self.to_string()}))).into_response()
    }
}

#[derive(Serialize, sqlx::FromRow)]
struct Summary {
    id: String,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct Layout {
    #[serde(flatten)]
    #[sqlx(flatten)]
    summary: Summary,
    widgets: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveLayout {
    name: String,
    widgets: Vec<Placement>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Placement {
    widget_type: WidgetType,
    grid_x: i32,
    grid_y: i32,
    grid_w: i32,
    grid_h: i32,
    z_order: i32,
    settings: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WidgetType {
    Ticker,
    Widget,
    Queue,
    NowPlaying,
    Recap,
    StreamPool,
    PatchBanner,
    RecordFlash,
    Nemesis,
    Goal,
}

impl SaveLayout {
    fn validate(mut self) -> Result<Self, LayoutError> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || !valid_text(&self.name) {
            return Err(LayoutError::Invalid(
                "layout name must contain 1 to 120 characters without control characters",
            ));
        }
        if self.widgets.len() > MAX_WIDGETS {
            return Err(LayoutError::Invalid(
                "a layout can hold at most 100 widgets",
            ));
        }
        for widget in &self.widgets {
            let Placement {
                grid_x: x,
                grid_y: y,
                grid_w: w,
                grid_h: h,
                ..
            } = *widget;
            if !(0..80).contains(&x)
                || !(0..45).contains(&y)
                || !(1..=80 - x).contains(&w)
                || !(1..=45 - y).contains(&h)
            {
                return Err(LayoutError::Invalid(
                    "widget rectangle must fit the 80×45 grid",
                ));
            }
            let settings = widget
                .settings
                .as_object()
                .ok_or(LayoutError::Invalid("widget settings must be an object"))?;
            for (key, value) in settings {
                let valid = match (&widget.widget_type, key.as_str()) {
                    (WidgetType::Widget, "me") => value.as_str().is_some_and(valid_text),
                    (WidgetType::NowPlaying, "mini" | "reveal") => value.is_boolean(),
                    _ => false,
                };
                if !valid {
                    return Err(LayoutError::Invalid(
                        "unsupported widget setting or invalid value",
                    ));
                }
            }
        }
        Ok(self)
    }
}

fn valid_text(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

fn parse_id(id: &str) -> Result<String, LayoutError> {
    Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| LayoutError::Invalid("invalid layout id"))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Summary>>, LayoutError> {
    Ok(Json(sqlx::query_as(
        "SELECT id::text, name, created_at, updated_at FROM overlay_layout ORDER BY updated_at DESC, id",
    ).fetch_all(&s.db).await?))
}

async fn read(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Layout>, LayoutError> {
    let id = parse_id(&id)?;
    Ok(Json(load(&mut *s.db.acquire().await?, &id).await?))
}

// Read parent and children from one statement snapshot, including during concurrent saves.
async fn load(db: &mut PgConnection, id: &str) -> Result<Layout, LayoutError> {
    sqlx::query_as(
        "SELECT l.id::text, l.name, l.created_at, l.updated_at,
                COALESCE((SELECT jsonb_agg(to_jsonb(w) - 'layout_id' ORDER BY w.z_order, w.id)
                          FROM overlay_widget_instance w WHERE w.layout_id = l.id), '[]'::jsonb) AS widgets
         FROM overlay_layout l WHERE l.id = $1::text::uuid",
    ).bind(id).fetch_optional(db).await?.ok_or(LayoutError::Missing)
}

async fn create(
    State(s): State<AppState>,
    body: Result<Json<SaveLayout>, JsonRejection>,
) -> Result<(StatusCode, Json<Layout>), LayoutError> {
    Ok((
        StatusCode::CREATED,
        Json(save(&s.db, None, body?.0.validate()?).await?),
    ))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<SaveLayout>, JsonRejection>,
) -> Result<Json<Layout>, LayoutError> {
    let id = parse_id(&id)?;
    Ok(Json(save(&s.db, Some(&id), body?.0.validate()?).await?))
}

async fn save(db: &PgPool, id: Option<&str>, body: SaveLayout) -> Result<Layout, LayoutError> {
    let mut tx = db.begin().await?;
    let id: String = if let Some(id) = id {
        // Updating the parent first locks it and serializes replacement with other PUTs/deletes.
        let id = sqlx::query_scalar(
            "UPDATE overlay_layout SET name = $2, updated_at = clock_timestamp()
             WHERE id = $1::text::uuid RETURNING id::text",
        )
        .bind(id)
        .bind(body.name)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LayoutError::Missing)?;
        sqlx::query("DELETE FROM overlay_widget_instance WHERE layout_id = $1::text::uuid")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        id
    } else {
        sqlx::query_scalar("INSERT INTO overlay_layout (name) VALUES ($1) RETURNING id::text")
            .bind(body.name)
            .fetch_one(&mut *tx)
            .await?
    };
    sqlx::query(
        "INSERT INTO overlay_widget_instance (layout_id, widget_type, grid_x, grid_y, grid_w, grid_h, z_order, settings)
         SELECT $1::text::uuid, w.* FROM jsonb_to_recordset($2::jsonb) AS w(
             widget_type text, grid_x int, grid_y int, grid_w int, grid_h int, z_order int, settings jsonb)",
    ).bind(&id).bind(sqlx::types::Json(body.widgets)).execute(&mut *tx).await?;
    let layout = load(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(layout)
}

async fn delete(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, LayoutError> {
    let result = sqlx::query("DELETE FROM overlay_layout WHERE id = $1::text::uuid")
        .bind(parse_id(&id)?)
        .execute(&s.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(LayoutError::Missing);
    }
    Ok(StatusCode::NO_CONTENT)
}
