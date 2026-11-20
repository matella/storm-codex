//! Local OBS layout storage. A save replaces the entire layout in one transaction.

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Request, State},
    http::StatusCode,
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{auth, AppState};

const BODY_LIMIT: usize = 64 * 1024;
// Compile the same Serde-only contract as storm-web-core, without its browser dependencies.
#[path = "../../../web-rs/core/src/overlay_layout/wire.rs"]
mod wire;
use wire::{
    valid_text, OverlayLayout, OverlayLayoutSummary, SaveOverlayLayout, WidgetPlacement, GRID_COLS,
    GRID_ROWS, MAX_WIDGETS,
};

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
    #[error("invalid stored overlay layout")]
    InvalidStoredLayout,
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
            Self::InvalidStoredLayout => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Database(e) => {
                tracing::error!(%e, "overlay layouts database error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(json!({"error": self.to_string()}))).into_response()
    }
}

#[derive(sqlx::FromRow)]
struct Summary {
    id: String,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Summary> for OverlayLayoutSummary {
    fn from(row: Summary) -> Self {
        Self {
            id: row.id,
            name: row.name,
            created_at: row.created_at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            updated_at: row.updated_at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        }
    }
}

#[derive(sqlx::FromRow)]
struct LayoutRow {
    #[sqlx(flatten)]
    summary: Summary,
    widgets: Value,
}

impl SaveOverlayLayout {
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
            let WidgetPlacement {
                grid_x: x,
                grid_y: y,
                grid_w: w,
                grid_h: h,
                ..
            } = *widget;
            if !(0..GRID_COLS).contains(&x)
                || !(0..GRID_ROWS).contains(&y)
                || !(1..=GRID_COLS - x).contains(&w)
                || !(1..=GRID_ROWS - y).contains(&h)
            {
                return Err(LayoutError::Invalid(
                    "widget rectangle must fit the 80×45 grid",
                ));
            }
        }
        Ok(self)
    }
}

fn parse_id(id: &str) -> Result<String, LayoutError> {
    Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| LayoutError::Invalid("invalid layout id"))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<OverlayLayoutSummary>>, LayoutError> {
    let rows: Vec<Summary> = sqlx::query_as(
        "SELECT id::text, name, created_at, updated_at FROM overlay_layout ORDER BY updated_at DESC, id",
    ).fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn read(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<OverlayLayout>, LayoutError> {
    let id = parse_id(&id)?;
    Ok(Json(load(&mut *s.db.acquire().await?, &id).await?))
}

// Read parent and children from one statement snapshot, including during concurrent saves.
async fn load(db: &mut PgConnection, id: &str) -> Result<OverlayLayout, LayoutError> {
    let row: LayoutRow = sqlx::query_as(
        "SELECT l.id::text, l.name, l.created_at, l.updated_at,
                COALESCE((SELECT jsonb_agg(jsonb_build_object(
                              'id', w.id, 'widget_type', w.widget_type, 'grid_x', w.grid_x,
                              'grid_y', w.grid_y, 'grid_w', w.grid_w, 'grid_h', w.grid_h,
                              'z_order', w.z_order, 'settings', w.settings) ORDER BY w.z_order, w.id)
                          FROM overlay_widget_instance w WHERE w.layout_id = l.id), '[]'::jsonb) AS widgets
         FROM overlay_layout l WHERE l.id = $1::text::uuid",
    ).bind(id).fetch_optional(db).await?.ok_or(LayoutError::Missing)?;
    // Fail closed if out-of-contract stored data ever bypasses the database constraint.
    let widgets = serde_json::from_value(row.widgets).map_err(|_| {
        tracing::error!(layout_id = %id, "overlay layout contains invalid stored widget data");
        LayoutError::InvalidStoredLayout
    })?;
    Ok(OverlayLayout {
        summary: row.summary.into(),
        widgets,
    })
}

async fn create(
    State(s): State<AppState>,
    body: Result<Json<SaveOverlayLayout>, JsonRejection>,
) -> Result<(StatusCode, Json<OverlayLayout>), LayoutError> {
    Ok((
        StatusCode::CREATED,
        Json(save(&s.db, None, body?.0.validate()?).await?),
    ))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<SaveOverlayLayout>, JsonRejection>,
) -> Result<Json<OverlayLayout>, LayoutError> {
    let id = parse_id(&id)?;
    Ok(Json(save(&s.db, Some(&id), body?.0.validate()?).await?))
}

async fn save(
    db: &PgPool,
    id: Option<&str>,
    body: SaveOverlayLayout,
) -> Result<OverlayLayout, LayoutError> {
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

#[cfg(test)]
#[path = "overlay_layout_tests.rs"]
mod overlay_layout_tests;
