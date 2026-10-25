//! Routes `/api/scouting/*` (spec volets B à E). Écritures protégées comme la gestion existante
//! (`ADMIN_TOKEN`, mode ouvert s'il est absent). Toute mutation d'un rapport (replay ajouté ou
//! retiré, roster, ancres, côté manuel) relance `recompute`, sous verrou de ligne du rapport.

use super::analysis;
use super::facts::{self, slug, FactRef, Facts, GameInput};
use super::pack::{self, PackMeta};
use super::side::{self, Source};
use super::summary::{self, GameSummary};
use crate::{AppState, PARSER_VERSION};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value as J;
use std::collections::{BTreeMap, HashMap};

type Err = (StatusCode, Json<J>);
type Resp = Result<Json<J>, Err>;

fn err(code: StatusCode, msg: impl Into<String>) -> Err {
    (code, Json(serde_json::json!({ "error": msg.into() })))
}
fn db_err(e: sqlx::Error) -> Err {
    tracing::error!("scouting : {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "db")
}
fn guard(h: &HeaderMap, s: &AppState) -> Result<(), Err> {
    if crate::manage::is_admin(h, s) {
        Ok(())
    } else {
        Err(err(StatusCode::UNAUTHORIZED, "admin token requis"))
    }
}
fn not_found() -> Err {
    err(StatusCode::NOT_FOUND, "rapport inconnu")
}

/// Statut dérivé (jamais stocké) : `empty` → `ready` → `analyzed`, ou `stale` si l'analyse a été
/// faite sur un état antérieur des faits.
const STATUS_SQL: &str = "CASE
    WHEN (SELECT count(*) FROM scouting_games g WHERE g.report_id = r.id) = 0 THEN 'empty'
    WHEN r.analysis IS NULL THEN 'ready'
    WHEN COALESCE(r.analysis_facts_version, 0) < r.facts_version THEN 'stale'
    ELSE 'analyzed' END";

// ── recalcul ─────────────────────────────────────────────────────────────────

/// Rôle par héros (slug du nom → rôle moderne `expandedRole`, repli sur le rôle historique).
async fn load_roles(db: &sqlx::PgPool) -> HashMap<String, String> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, COALESCE(data->>'expandedRole', role) FROM dim_heroes",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();
    let mut m = HashMap::new();
    for (id, name, role) in rows {
        if let Some(role) = role {
            m.insert(slug(&id), role.clone());
            m.insert(slug(&name), role);
        }
    }
    m
}

fn strings(v: &J) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Recalcule côtés cibles et faits d'un rapport ; incrémente `facts_version`. Idempotent quant au
/// contenu. Le verrou `FOR UPDATE` sérialise les recalculs concurrents d'un même rapport.
pub async fn recompute(db: &sqlx::PgPool, report_id: i64) -> Result<(), sqlx::Error> {
    let roles = load_roles(db).await;
    let mut tx = db.begin().await?;
    let Some((roster, anchors)): Option<(J, J)> =
        sqlx::query_as("SELECT roster, anchors FROM scouting_reports WHERE id = $1 FOR UPDATE")
            .bind(report_id)
            .fetch_optional(&mut *tx)
            .await?
    else {
        return Ok(());
    };
    let (roster, anchors) = (strings(&roster), strings(&anchors));
    let rows: Vec<(i64, J, Option<i32>, Option<String>)> = sqlx::query_as(
        "SELECT id, summary, target_team, target_source FROM scouting_games
         WHERE report_id = $1 ORDER BY played_at, id",
    )
    .bind(report_id)
    .fetch_all(&mut *tx)
    .await?;
    let games: Vec<(i64, GameSummary, Option<i32>, Option<String>)> = rows
        .into_iter()
        .filter_map(|(id, s, t, src)| serde_json::from_value(s).ok().map(|s| (id, s, t, src)))
        .collect();

    let summaries: Vec<GameSummary> = games.iter().map(|g| g.1.clone()).collect();
    let detection = side::candidates(&summaries, &anchors);
    let effective = side::effective_roster(&roster, &detection);

    let mut sides: Vec<Option<u8>> = Vec::with_capacity(games.len());
    for (gid, g, team, src) in &games {
        let (new_team, new_src) = if src.as_deref() == Some(Source::Manual.as_str()) {
            (team.and_then(|t| u8::try_from(t).ok()), Some(Source::Manual))
        } else {
            match side::detect(g, &effective, &anchors) {
                Some((t, s)) => (Some(t), Some(s)),
                None => (None, None),
            }
        };
        let new_team_i = new_team.map(i32::from);
        let new_src_s = new_src.map(|s| s.as_str().to_string());
        if new_team_i != *team || new_src_s != *src {
            sqlx::query("UPDATE scouting_games SET target_team = $2, target_source = $3 WHERE id = $1")
                .bind(gid)
                .bind(new_team_i)
                .bind(new_src_s)
                .execute(&mut *tx)
                .await?;
        }
        sides.push(new_team);
    }

    let inputs: Vec<GameInput> = games
        .iter()
        .zip(&sides)
        .filter_map(|((gid, g, _, _), s)| s.map(|side| GameInput { gid: *gid, summary: g, side }))
        .collect();
    let excluded = (games.len() - inputs.len()) as u32;
    let computed = facts::compute(&inputs, &effective, excluded, &roles);
    let snapshot = serde_json::json!({
        "facts": computed,
        "detection": detection,
        "roster": effective,
        "roster_auto": roster.is_empty(),
    });
    sqlx::query(
        "UPDATE scouting_reports SET facts = $2, facts_version = facts_version + 1, updated_at = now()
         WHERE id = $1",
    )
    .bind(report_id)
    .bind(snapshot)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

// ── rapports ─────────────────────────────────────────────────────────────────

/// GET /api/scouting — liste, du plus récent au plus ancien (sans faits ni analyse).
pub async fn list(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(&format!(
        "SELECT COALESCE(jsonb_agg(x ORDER BY x->>'created_at' DESC), '[]'::jsonb) FROM (
           SELECT jsonb_build_object(
             'id', r.id, 'title', r.title, 'target_name', r.target_name,
             'created_at', r.created_at, 'updated_at', r.updated_at,
             'games', (SELECT count(*) FROM scouting_games g WHERE g.report_id = r.id),
             'record', r.facts->'facts'->'overview'->'record',
             'first_date', r.facts->'facts'->'overview'->'first_date',
             'last_date', r.facts->'facts'->'overview'->'last_date',
             'builds', r.facts->'facts'->'overview'->'builds',
             'analysis_imported_at', r.analysis_imported_at,
             'status', {STATUS_SQL}) x
           FROM scouting_reports r) t"
    ))
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct CreateBody {
    title: String,
    #[serde(default)]
    target_name: Option<String>,
}

/// POST /api/scouting — crée un rapport vide.
pub async fn create(State(s): State<AppState>, headers: HeaderMap, Json(b): Json<CreateBody>) -> Result<(StatusCode, Json<J>), Err> {
    guard(&headers, &s)?;
    let title = b.title.trim();
    if title.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "title requis"));
    }
    let target = b.target_name.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO scouting_reports (title, target_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(title)
    .bind(target)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    recompute(&s.db, id).await.map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

fn index_of(snapshot: &J) -> BTreeMap<String, FactRef> {
    serde_json::from_value(snapshot["facts"]["index"].clone()).unwrap_or_default()
}

/// GET /api/scouting/{id} — rapport complet : faits (instantané), parties, détection du roster,
/// analyse avec preuves résolues contre les faits COURANTS, statut dérivé.
pub async fn get(State(s): State<AppState>, Path(id): Path<i64>) -> Resp {
    let v: Option<J> = sqlx::query_scalar(&format!(
        "SELECT jsonb_build_object(
           'id', r.id, 'title', r.title, 'target_name', r.target_name,
           'roster', r.roster, 'anchors', r.anchors,
           'facts_version', r.facts_version, 'snapshot', r.facts,
           'analysis', r.analysis, 'analysis_facts_version', r.analysis_facts_version,
           'analysis_model', r.analysis_model, 'analysis_imported_at', r.analysis_imported_at,
           'created_at', r.created_at, 'updated_at', r.updated_at,
           'status', {STATUS_SQL},
           'games', COALESCE((SELECT jsonb_agg(jsonb_build_object(
               'gid', g.id, 'filename', g.filename, 'played_at', g.played_at, 'map', g.map,
               'length_s', g.summary->'length_s', 'winner', g.summary->'winner',
               'target_team', g.target_team, 'target_source', g.target_source,
               'teams', jsonb_build_array(
                   (SELECT COALESCE(jsonb_agg(jsonb_build_object('toon', p->'toon', 'name', p->'name', 'hero', p->'hero')), '[]'::jsonb)
                      FROM jsonb_array_elements(g.summary->'teams'->0) p),
                   (SELECT COALESCE(jsonb_agg(jsonb_build_object('toon', p->'toon', 'name', p->'name', 'hero', p->'hero')), '[]'::jsonb)
                      FROM jsonb_array_elements(g.summary->'teams'->1) p)))
             ORDER BY g.played_at, g.id) FROM scouting_games g WHERE g.report_id = r.id), '[]'::jsonb))
         FROM scouting_reports r WHERE r.id = $1"
    ))
    .bind(id)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;
    let Some(mut v) = v else { return Err(not_found()) };
    if let Ok(a) = serde_json::from_value::<analysis::Analysis>(v["analysis"].clone()) {
        if !v["analysis"].is_null() {
            let (resolved, tally) = analysis::resolve(&a, &index_of(&v["snapshot"]));
            v["analysis"] = resolved;
            v["analysis_tally"] = serde_json::to_value(tally).unwrap_or(J::Null);
        }
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct PatchBody {
    title: Option<String>,
    target_name: Option<String>,
    roster: Option<Vec<String>>,
    anchors: Option<Vec<String>>,
}

/// PATCH /api/scouting/{id} — titre, équipe, roster, ancres (les deux derniers → recalcul).
pub async fn patch(State(s): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(b): Json<PatchBody>) -> Resp {
    guard(&headers, &s)?;
    let clean = |v: Vec<String>| -> J {
        let mut out: Vec<String> = Vec::new();
        for t in v {
            let t = t.trim().to_string();
            if !t.is_empty() && !out.contains(&t) {
                out.push(t);
            }
        }
        serde_json::to_value(out).unwrap_or_else(|_| serde_json::json!([]))
    };
    let title = b.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let recalc = b.roster.is_some() || b.anchors.is_some();
    let done = sqlx::query(
        "UPDATE scouting_reports SET
           title = COALESCE($2, title),
           target_name = CASE WHEN $3::text IS NULL THEN target_name ELSE NULLIF(trim($3), '') END,
           roster = COALESCE($4, roster),
           anchors = COALESCE($5, anchors),
           updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(title)
    .bind(b.target_name)
    .bind(b.roster.map(clean))
    .bind(b.anchors.map(clean))
    .execute(&s.db)
    .await
    .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(not_found());
    }
    if recalc {
        recompute(&s.db, id).await.map_err(db_err)?;
    }
    get(State(s), Path(id)).await
}

/// DELETE /api/scouting/{id} — les parties partent en cascade ; les bruts archivés restent.
pub async fn delete(State(s): State<AppState>, Path(id): Path<i64>, headers: HeaderMap) -> Result<StatusCode, Err> {
    guard(&headers, &s)?;
    let done = sqlx::query("DELETE FROM scouting_reports WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await
        .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── replays ──────────────────────────────────────────────────────────────────

/// Parse + résumé d'un replay archivé (CPU, hors thread HTTP).
async fn parse(s: &AppState, path: std::path::PathBuf, name: String) -> Result<(storm_stats::Output, GameSummary, String), String> {
    let _permit = s.parse_sem.acquire().await.map_err(|_| "pool".to_string())?;
    let out = tokio::task::spawn_blocking(move || storm_stats::process_replay(&path, &name))
        .await
        .map_err(|_| "panic".to_string())?;
    if out.status != 1 {
        return Err(crate::upload::reject_class(out.status).to_string());
    }
    let fp = crate::upload::game_fingerprint(&out).ok_or("no_fingerprint")?;
    let sum = summary::summarize(&out).map_err(|_| "summary".to_string())?;
    Ok((out, sum, fp))
}

/// POST /api/scouting/{id}/replays — ajoute un replay (octets bruts, header `X-Filename`), même
/// contrat que `/api/upload`. Réponse : `added` / `duplicate` / `parse_failed`.
pub async fn add_replay(State(s): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, bytes: Bytes) -> Resp {
    guard(&headers, &s)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM scouting_reports WHERE id = $1")
        .bind(id)
        .fetch_optional(&s.db)
        .await
        .map_err(db_err)?;
    if exists.is_none() {
        return Err(not_found());
    }
    let sha = crate::upload::sha256_hex(&bytes);
    let dup: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM scouting_games WHERE report_id = $1 AND file_sha256 = $2",
    )
    .bind(id)
    .bind(&sha)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;
    if let Some(gid) = dup {
        return Ok(Json(serde_json::json!({ "status": "duplicate", "gid": gid })));
    }

    // archive d'abord (source de vérité), comme l'upload principal
    let dir = s.cfg.archive_dir.join("scouting");
    let path = dir.join(format!("{sha}.StormReplay"));
    let written = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&path, &bytes).await
    };
    if let Err(e) = written.await {
        tracing::error!("archivage scouting : {e}");
        return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "io"));
    }
    let filename = crate::upload::filename(&headers);
    let (out, sum, fp) = match parse(&s, path.clone(), filename.clone().unwrap_or_else(|| path.to_string_lossy().into())).await {
        Ok(x) => x,
        Err(class) => {
            return Ok(Json(serde_json::json!({ "status": "parse_failed", "error_class": class })))
        }
    };
    let gid: Option<i64> = sqlx::query_scalar(
        "INSERT INTO scouting_games (report_id, fingerprint, file_sha256, archived_path, filename,
            played_at, map, build, summary, data, parser_version)
         VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7, $8, $9, $10, $11)
         ON CONFLICT (report_id, fingerprint) DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(&fp)
    .bind(&sha)
    .bind(path.to_string_lossy().as_ref())
    .bind(filename)
    .bind(sum.date.as_deref())
    .bind(&sum.map)
    .bind(sum.build.and_then(|b| i32::try_from(b).ok()))
    .bind(serde_json::to_value(&sum).unwrap_or(J::Null))
    .bind(out.to_json())
    .bind(PARSER_VERSION)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;
    let Some(gid) = gid else {
        // même partie, fichier différent (ré-export) : doublon au sens de la partie
        return Ok(Json(serde_json::json!({ "status": "duplicate" })));
    };
    recompute(&s.db, id).await.map_err(db_err)?;
    let team: Option<(Option<i32>, Option<String>)> =
        sqlx::query_as("SELECT target_team, target_source FROM scouting_games WHERE id = $1")
            .bind(gid)
            .fetch_optional(&s.db)
            .await
            .map_err(db_err)?;
    let (target_team, target_source) = team.unwrap_or((None, None));
    Ok(Json(serde_json::json!({
        "status": "added", "gid": gid, "map": sum.map,
        "target_team": target_team, "target_source": target_source,
    })))
}

#[derive(Deserialize)]
pub struct SideBody {
    /// 0/1 = côté fixé à la main ; `null` = revenir à la détection automatique.
    target_team: Option<u8>,
}

/// PATCH /api/scouting/{id}/replays/{gid} — côté cible manuel (ou retour à l'automatique).
pub async fn set_side(State(s): State<AppState>, Path((id, gid)): Path<(i64, i64)>, headers: HeaderMap, Json(b): Json<SideBody>) -> Resp {
    guard(&headers, &s)?;
    if b.target_team.is_some_and(|t| t > 1) {
        return Err(err(StatusCode::BAD_REQUEST, "target_team ∈ {0, 1, null}"));
    }
    let (team, src) = match b.target_team {
        Some(t) => (Some(i32::from(t)), Some(Source::Manual.as_str())),
        None => (None, None),
    };
    let done = sqlx::query(
        "UPDATE scouting_games SET target_team = $3, target_source = $4 WHERE id = $2 AND report_id = $1",
    )
    .bind(id)
    .bind(gid)
    .bind(team)
    .bind(src)
    .execute(&s.db)
    .await
    .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "partie inconnue"));
    }
    recompute(&s.db, id).await.map_err(db_err)?;
    get(State(s), Path(id)).await
}

/// DELETE /api/scouting/{id}/replays/{gid}
pub async fn delete_replay(State(s): State<AppState>, Path((id, gid)): Path<(i64, i64)>, headers: HeaderMap) -> Resp {
    guard(&headers, &s)?;
    let done = sqlx::query("DELETE FROM scouting_games WHERE id = $2 AND report_id = $1")
        .bind(id)
        .bind(gid)
        .execute(&s.db)
        .await
        .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "partie inconnue"));
    }
    recompute(&s.db, id).await.map_err(db_err)?;
    get(State(s), Path(id)).await
}

// ── pack ─────────────────────────────────────────────────────────────────────

async fn load_pack(s: &AppState, id: i64) -> Result<(String, Option<String>, i32, Facts), Err> {
    let row: Option<(String, Option<String>, i32, Option<J>)> = sqlx::query_as(
        "SELECT title, target_name, facts_version, facts FROM scouting_reports WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;
    let Some((title, target, fv, snap)) = row else { return Err(not_found()) };
    let facts: Facts = snap
        .and_then(|v| serde_json::from_value(v["facts"].clone()).ok())
        .ok_or_else(|| err(StatusCode::CONFLICT, "faits pas encore calculés"))?;
    Ok((title, target, fv, facts))
}

fn file_name(title: &str, ext: &str) -> String {
    let base = slug(title);
    format!("scouting-{}.{ext}", if base.is_empty() { "report".into() } else { base })
}

/// GET /api/scouting/{id}/pack.md
pub async fn pack_md(State(s): State<AppState>, Path(id): Path<i64>) -> Result<Response, Err> {
    let (title, target, fv, facts) = load_pack(&s, id).await?;
    let meta = PackMeta { report_id: id, facts_version: fv, title: &title, target_name: target.as_deref() };
    let body = pack::markdown(&meta, &facts);
    Ok((
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("inline; filename=\"{}\"", file_name(&title, "md"))),
        ],
        body,
    )
        .into_response())
}

/// GET /api/scouting/{id}/pack.xlsx
pub async fn pack_xlsx(State(s): State<AppState>, Path(id): Path<i64>) -> Result<Response, Err> {
    let (title, target, fv, facts) = load_pack(&s, id).await?;
    let meta = PackMeta { report_id: id, facts_version: fv, title: &title, target_name: target.as_deref() };
    let bytes = super::xlsx::workbook(&meta, &facts).map_err(|e| {
        tracing::error!("xlsx : {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "xlsx")
    })?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            ),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", file_name(&title, "xlsx"))),
        ],
        bytes,
    )
        .into_response())
}

// ── analyse ──────────────────────────────────────────────────────────────────

/// PUT /api/scouting/{id}/analysis — corps = texte brut de la réponse du LLM. Écrase l'analyse
/// existante (décision opérateur) ; refus sans rien écraser si le JSON est invalide.
pub async fn put_analysis(State(s): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, body: String) -> Resp {
    guard(&headers, &s)?;
    let row: Option<(i32, Option<J>)> =
        sqlx::query_as("SELECT facts_version, facts FROM scouting_reports WHERE id = $1")
            .bind(id)
            .fetch_optional(&s.db)
            .await
            .map_err(db_err)?;
    let Some((fv, snap)) = row else { return Err(not_found()) };
    let imported = analysis::import(&body, id, i64::from(fv))
        .map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    let stored_fv = imported
        .facts_version
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(fv);
    let a = &imported.analysis;
    sqlx::query(
        "UPDATE scouting_reports SET analysis = $2, analysis_facts_version = $3, analysis_model = $4,
            analysis_imported_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(serde_json::to_value(a).unwrap_or(J::Null))
    .bind(stored_fv)
    .bind(a.model.as_deref())
    .execute(&s.db)
    .await
    .map_err(db_err)?;
    let (_, tally) = analysis::resolve(a, &snap.as_ref().map(index_of).unwrap_or_default());
    Ok(Json(serde_json::json!({ "imported": true, "warnings": imported.warnings, "tally": tally })))
}

/// DELETE /api/scouting/{id}/analysis
pub async fn delete_analysis(State(s): State<AppState>, Path(id): Path<i64>, headers: HeaderMap) -> Result<StatusCode, Err> {
    guard(&headers, &s)?;
    let done = sqlx::query(
        "UPDATE scouting_reports SET analysis = NULL, analysis_facts_version = NULL, analysis_model = NULL,
            analysis_imported_at = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .execute(&s.db)
    .await
    .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── re-process ───────────────────────────────────────────────────────────────

/// Re-parse les replays de scouting dont `parser_version` est périmé, puis recalcule les rapports
/// touchés. Appelé par `POST /api/admin/reprocess` (en arrière-plan). Renvoie le nombre de replays.
pub async fn reprocess_stale(s: &AppState) -> usize {
    let targets: Vec<(i64, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, report_id, archived_path, filename FROM scouting_games WHERE parser_version < $1",
    )
    .bind(PARSER_VERSION)
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    let n = targets.len();
    let mut reports = std::collections::BTreeSet::new();
    for (gid, rid, path, name) in targets {
        let p = std::path::PathBuf::from(&path);
        match parse(s, p, name.unwrap_or(path)).await {
            Ok((out, sum, _)) => {
                let _ = sqlx::query(
                    "UPDATE scouting_games SET summary = $2, data = $3, parser_version = $4 WHERE id = $1",
                )
                .bind(gid)
                .bind(serde_json::to_value(&sum).unwrap_or(J::Null))
                .bind(out.to_json())
                .bind(PARSER_VERSION)
                .execute(&s.db)
                .await;
                reports.insert(rid);
            }
            Err(class) => tracing::warn!("reprocess scouting {gid} : {class}"),
        }
    }
    for rid in reports {
        if let Err(e) = recompute(&s.db, rid).await {
            tracing::error!("recalcul scouting {rid} : {e}");
        }
    }
    n
}
