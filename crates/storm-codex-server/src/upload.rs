//! Upload pipeline (milestone 3's parser.js): token → archive first → parse pool →
//! projection → status. Semantics: waits for the result up to 2 s, otherwise `202 accepted`.

use crate::{config::Config, project, AppState, PARSER_VERSION};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Game fingerprint (overlay compat): SHA-256("date|map|length|sortedToons").
pub(crate) fn game_fingerprint(out: &storm_stats::Output) -> Option<String> {
    let m = out.match_.as_ref()?;
    let players = out.players.as_ref()?;
    let date = m.get("date").and_then(|v| v.as_str())?;
    let map = m.get("map").and_then(|v| v.as_str())?;
    // JS `${length}`: integer without .0, otherwise short decimal (f64 Display == JS String here)
    let length = m.get("length").and_then(|v| v.as_f64())?;
    let mut toons: Vec<&str> = players.keys().map(String::as_str).collect();
    toons.sort_unstable();
    let src = format!("{date}|{map}|{length}|{}", toons.join(","));
    Some(sha256_hex(src.as_bytes()))
}

/// storm-stats status ≠ 1 → typed error class (visible in Admin). Exhaustive over the
/// `storm_stats::status` constants (see constants.rs).
pub(crate) fn reject_class(status: i64) -> &'static str {
    match status {
        0 => "unsupported_mode",  // UNSUPPORTED — brawl / unhandled mode
        -2 => "stats_failure",    // FAILURE — internal failure of the stats engine
        -3 => "unsupported_map",  // map outside the table (and outside EXTRA_MAPS)
        -4 => "computer_player",  // game with AI
        -5 => "incomplete",       // truncated replay / incomplete game
        -6 => "too_old",          // build too old (map missing from the reference data)
        -7 => "unverified_build", // build too recent / unverified
        _ => "parse_failed",
    }
}

#[derive(serde::Serialize)]
pub struct UploadResponse {
    pub status: String, // parsed | duplicate | parse_failed | accepted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_owned())
}

/// True if the header carries a `Bearer` matching a non-revoked upload token.
/// Shared with `lobby::api::ingest`: both endpoints have the same authentication contract.
pub async fn token_valide(db: &sqlx::PgPool, headers: &HeaderMap) -> bool {
    token_id(db, headers).await.is_some()
}

/// The token id, when it is valid (the upload needs it to trace `uploads.token_id`).
pub async fn token_id(db: &sqlx::PgPool, headers: &HeaderMap) -> Option<i64> {
    let tok = bearer(headers)?;
    let h = sha256_hex(tok.as_bytes());
    sqlx::query_scalar("SELECT id FROM upload_tokens WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(&h)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> (StatusCode, Json<UploadResponse>) {
    // 1. authentication by token (token_hash = SHA-256 of the clear token)
    let Some(token_id) = token_id(&state.db, &headers).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(UploadResponse {
                status: "unauthorized".into(),
                match_id: None,
                error_class: None,
            }),
        );
    };

    // 2. file dedup (content hash) — immediate response if already processed
    let content_hash = sha256_hex(&bytes);
    if let Ok(Some(st)) =
        sqlx::query_scalar::<_, String>("SELECT status FROM uploads WHERE fingerprint = $1")
            .bind(&content_hash)
            .fetch_optional(&state.db)
            .await
    {
        if st == "parsed" || st == "duplicate" {
            // 409 Conflict: compat with client-rs's "duplicate" handling
            return (
                StatusCode::CONFLICT,
                Json(UploadResponse {
                    status: "duplicate".into(),
                    match_id: None,
                    error_class: None,
                }),
            );
        }
    }

    // 3. archive first (source of truth), then uploads(pending) row
    let archived = state
        .cfg
        .archive_dir
        .join(format!("{content_hash}.StormReplay"));
    if let Err(e) = write_atomic(&archived, &bytes).await {
        tracing::error!("archiving: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadResponse {
                status: "error".into(),
                match_id: None,
                error_class: Some("io".into()),
            }),
        );
    }
    let upload_id: i64 = match sqlx::query_scalar(
        "INSERT INTO uploads (token_id, filename, fingerprint, archived_path, status)
         VALUES ($1,$2,$3,$4,'pending')
         ON CONFLICT (fingerprint) DO UPDATE SET status = 'pending'
         RETURNING id",
    )
    .bind(token_id)
    .bind(filename(&headers).unwrap_or_else(|| format!("{content_hash}.StormReplay")))
    .bind(&content_hash)
    .bind(Config::archive_entry(&content_hash))
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("insert upload: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(UploadResponse {
                    status: "error".into(),
                    match_id: None,
                    error_class: Some("db".into()),
                }),
            );
        }
    };

    // 4. parse in the pool (never on the HTTP thread); saturated pool → 202 accepted
    let permit = match state.parse_sem.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            // backfill: we accept, the parse will follow as soon as a worker is free
            let st = state.clone();
            tokio::spawn(async move {
                if let Ok(p) = st.parse_sem.clone().acquire_owned().await {
                    run_parse(st, upload_id, archived, p).await;
                }
            });
            return (
                StatusCode::ACCEPTED,
                Json(UploadResponse {
                    status: "accepted".into(),
                    match_id: None,
                    error_class: None,
                }),
            );
        }
    };

    // waits for the result up to 2 s (typically < 0.5 s)
    let st = state.clone();
    let handle = tokio::spawn(async move { run_parse(st, upload_id, archived, permit).await });
    match tokio::time::timeout(Duration::from_secs(2), handle).await {
        Ok(Ok(outcome)) => outcome.into_response(),
        _ => (
            StatusCode::ACCEPTED,
            Json(UploadResponse {
                status: "accepted".into(),
                match_id: None,
                error_class: None,
            }),
        ),
    }
}

struct Outcome {
    status: String,
    match_id: Option<i64>,
    error_class: Option<String>,
}
impl Outcome {
    fn into_response(self) -> (StatusCode, Json<UploadResponse>) {
        (
            StatusCode::OK,
            Json(UploadResponse {
                status: self.status,
                match_id: self.match_id,
                error_class: self.error_class,
            }),
        )
    }
}

/// Parse attempts after which [`resume_pending`] stops re-parsing an upload left `pending`
/// (migration 0023). Count the attempt before starting the decoder so it survives a restart.
pub(crate) const MAX_PARSE_ATTEMPTS: i32 = 3;

/// Retry bookkeeping, not the decode. Keep the original worker and its permit throughout.
async fn record_parse_attempt(db: &sqlx::PgPool, upload_id: i64) -> bool {
    let mut delays_ms = [250, 500].into_iter();
    loop {
        let write =
            sqlx::query("UPDATE uploads SET parse_attempts = parse_attempts + 1 WHERE id = $1")
                .bind(upload_id)
                .execute(db);
        let error = match tokio::time::timeout(Duration::from_secs(1), write).await {
            Ok(Ok(result)) if result.rows_affected() == 1 => return true,
            Ok(Ok(_)) => {
                tracing::error!(upload_id, "cannot record parse attempt: upload row missing");
                return false;
            }
            Ok(Err(error)) => error.to_string(),
            Err(error) => error.to_string(),
        };
        let Some(delay) = delays_ms.next() else {
            tracing::error!(upload_id, %error, "parse attempt not confirmed after retries; leaving upload pending");
            return false;
        };
        tracing::warn!(upload_id, %error, "parse attempt not confirmed; retrying bookkeeping");
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}

/// CPU-bound parse (spawn_blocking) + projection + `uploads` update. Broadcasts the WS event.
async fn run_parse(
    state: AppState,
    upload_id: i64,
    archived: PathBuf,
    _permit: tokio::sync::OwnedSemaphorePermit,
) -> Outcome {
    // An attempt that aborts the process must already be recorded.
    if !record_parse_attempt(&state.db, upload_id).await {
        return Outcome {
            status: "error".into(),
            match_id: None,
            error_class: Some("db".into()),
        };
    }
    let path = archived.clone();
    // catch_unwind via spawn_blocking: a worker panic does not bring the server down
    let parsed = tokio::task::spawn_blocking(move || {
        storm_stats::process_replay(&path, &path.to_string_lossy())
    })
    .await;

    let out = match parsed {
        Ok(out) => out,
        Err(_) => {
            mark_failed(&state.db, upload_id, "panic", "worker panic").await;
            return Outcome {
                status: "parse_failed".into(),
                match_id: None,
                error_class: Some("panic".into()),
            };
        }
    };

    if out.status != 1 {
        let class = reject_class(out.status);
        mark_failed(
            &state.db,
            upload_id,
            class,
            &format!("statut storm-stats {}", out.status),
        )
        .await;
        return Outcome {
            status: "parse_failed".into(),
            match_id: None,
            error_class: Some(class.into()),
        };
    }

    let Some(fp) = game_fingerprint(&out) else {
        mark_failed(
            &state.db,
            upload_id,
            "no_fingerprint",
            "fingerprint indisponible",
        )
        .await;
        return Outcome {
            status: "parse_failed".into(),
            match_id: None,
            error_class: Some("no_fingerprint".into()),
        };
    };
    let build = out
        .match_
        .as_ref()
        .and_then(|m| m.get("version"))
        .and_then(|v| v.get("m_build"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    match project::project_match(&state.db, &fp, PARSER_VERSION, &out).await {
        Ok(match_id) => {
            if let Err(e) = sqlx::query(
                "UPDATE uploads SET status='parsed', parser_version=$2, build=$3, match_id=$4,
                 error_class=NULL, error_msg=NULL, parsed_at=now() WHERE id=$1",
            )
            .bind(upload_id)
            .bind(PARSER_VERSION)
            .bind(build)
            .bind(match_id)
            .execute(&state.db)
            .await
            {
                // the match is projected; the upload stays `pending` and will be resumed at startup
                tracing::error!("upload {upload_id}: parsed status not written: {e}");
            }

            link_lobby(&state, match_id).await;

            // real-time push (milestone 3 T5)
            let _ = state.events.send(serde_json::json!({
                "type": "match.parsed",
                "match_id": match_id,
                "map": out.match_.as_ref().and_then(|m| m.get("map")).cloned(),
            }));
            // push post-game box→Azure (extension Twitch) — optionnel
            if let Some(url) = &state.cfg.azure_push_url {
                let event = crate::azure::match_completed_event(match_id, &out);
                crate::azure::push(url.clone(), state.cfg.azure_push_token.clone(), event);
            }
            Outcome {
                status: "parsed".into(),
                match_id: Some(match_id),
                error_class: None,
            }
        }
        Err(e) => {
            mark_failed(&state.db, upload_id, "projection", &e.to_string()).await;
            Outcome {
                status: "parse_failed".into(),
                match_id: None,
                error_class: Some("projection".into()),
            }
        }
    }
}

/// If `match_id` is the game of the open lobby, the companion switches to debrief. The matching
/// (`lier_match` reads `match_players`) runs on a copy, **outside** the lobby lock, so `GET /api/lobby`,
/// `GET /api/live` and the lobby inputs are never held behind it. The link is then applied under a
/// brief write lock as a compare-and-swap: only if the lobby is still the one that was matched (same
/// `content_hash`) and still unlinked — a lobby replaced meanwhile (next game, same ten players) is
/// never stamped with this match. The persistence stays under that lock (one upsert), like every
/// other `lobby_live` write (`muter` in `lobby/api.rs`): saving outside it could let this snapshot
/// overwrite a concurrent hero/map/team input. Rank readings are attached and `lobby.updated` is
/// broadcast after the lock is released.
async fn link_lobby(state: &AppState, match_id: i64) {
    let Some(mut candidate) = state.lobby.read().await.clone() else {
        return;
    };
    if !crate::lobby::lier_match(&state.db, &mut candidate, match_id).await {
        return;
    }
    let session = {
        let mut guard = state.lobby.write().await;
        let Some(st) = guard
            .as_mut()
            .filter(|st| st.content_hash == candidate.content_hash && st.match_id.is_none())
        else {
            return;
        };
        st.match_id = Some(match_id);
        if let Err(e) = crate::lobby::store::save(&state.db, st).await {
            tracing::error!("lobby save: {e}");
        }
        st.content_hash.clone()
    };
    // pending rank readings of this session (the lobby hash = session id)
    if let Err(e) = crate::rank::attach_to_match(&state.db, &session, match_id).await {
        tracing::error!("rank attach: {e}");
    }
    let _ = state
        .events
        .send(serde_json::json!({ "type": "lobby.updated" }));
}

async fn mark_failed(db: &sqlx::PgPool, upload_id: i64, class: &str, msg: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE uploads SET status='parse_failed', error_class=$2, error_msg=$3, parsed_at=now()
         WHERE id=$1",
    )
    .bind(upload_id)
    .bind(class)
    .bind(msg)
    .execute(db)
    .await
    {
        tracing::error!("upload {upload_id}: parse_failed status ({class}) not written: {e}");
    }
}

/// Writes the archive through a temporary file then a rename: a stop in the middle of writing never
/// leaves a truncated replay in place of the source of truth.
async fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    if let Err(e) = tokio::fs::write(&tmp, bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    tokio::fs::rename(&tmp, path).await
}

/// Resumes at startup the uploads left `pending`: a `202 accepted` queues the parse
/// in memory only, so a restart before the end left them stuck (the
/// client does not resend an accepted file). The archived file is the source of truth;
/// `project_match` is idempotent. An upload already attempted [`MAX_PARSE_ATTEMPTS`] times
/// (each one ended by the process dying) is marked `parse_failed` / `too_many_attempts`
/// instead. Returns the number of uploads queued again.
pub(crate) async fn resume_pending(state: &AppState) -> Result<usize, sqlx::Error> {
    let given_up: Vec<(i64, i32)> = sqlx::query_as(
        "UPDATE uploads SET status = 'parse_failed', error_class = 'too_many_attempts',
                error_msg = 'parse interrupted ' || parse_attempts || ' times (process died)',
                parsed_at = now()
         WHERE status = 'pending' AND archived_path IS NOT NULL AND parse_attempts >= $1
         RETURNING id, parse_attempts",
    )
    .bind(MAX_PARSE_ATTEMPTS)
    .fetch_all(&state.db)
    .await?;
    for (upload_id, attempts) in given_up {
        tracing::warn!(
            "upload {upload_id}: {attempts} interrupted parse attempts, marked parse_failed \
             (too_many_attempts) instead of being resumed"
        );
    }
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, archived_path FROM uploads
         WHERE status = 'pending' AND archived_path IS NOT NULL AND parse_attempts < $1
         ORDER BY id",
    )
    .bind(MAX_PARSE_ATTEMPTS)
    .fetch_all(&state.db)
    .await?;
    let n = rows.len();
    for (upload_id, path) in rows {
        let st = state.clone();
        tokio::spawn(async move {
            if let Ok(p) = st.parse_sem.clone().acquire_owned().await {
                let file = st.cfg.archived_file(&path);
                run_parse(st, upload_id, file, p).await;
            }
        });
    }
    Ok(n)
}

pub(crate) fn filename(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
}

/// Decodes the `%XX` sequences of the X-Filename header. The client (client-rs) percent-encodes the
/// non-ASCII characters and the apostrophe (`'` → `%27`) for a safe HTTP transport; we restore the
/// readable name ("Blackheart's Bay.StormReplay"). Invalid UTF-8 bytes → we keep the raw value.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{game_fingerprint, percent_decode, reject_class};
    use serde_json::json;

    #[test]
    fn decodes_apostrophe_map_name() {
        assert_eq!(
            percent_decode("2024-12-25 21.30.45 Blackheart%27s Bay.StormReplay"),
            "2024-12-25 21.30.45 Blackheart's Bay.StormReplay"
        );
        // no sequence → unchanged
        assert_eq!(
            percent_decode("Cursed Hollow.StormReplay"),
            "Cursed Hollow.StormReplay"
        );
        // % at the end of the string without 2 hex digits → kept as is
        assert_eq!(percent_decode("weird%2"), "weird%2");
    }

    /// Each storm-stats rejection status has its typed class (readable in Admin); an unknown
    /// status falls back to `parse_failed`, never a panic.
    #[test]
    fn reject_class_exhaustif_et_sans_trou() {
        let known = [
            (0, "unsupported_mode"),
            (-2, "stats_failure"),
            (-3, "unsupported_map"),
            (-4, "computer_player"),
            (-5, "incomplete"),
            (-6, "too_old"),
            (-7, "unverified_build"),
        ];
        for (status, class) in known {
            assert_eq!(reject_class(status), class, "statut {status}");
        }
        assert_eq!(reject_class(-99), "parse_failed");
    }

    fn output(date: &str, map: &str, length: f64, toons: &[&str]) -> storm_stats::Output {
        let mut players = serde_json::Map::new();
        for t in toons {
            players.insert((*t).into(), json!({}));
        }
        storm_stats::Output {
            hotkey_profiles: Default::default(),
            actions: Default::default(),
            status: 1,
            match_: Some(
                serde_json::from_value(json!({"date": date, "map": map, "length": length}))
                    .unwrap(),
            ),
            players: Some(players),
        }
    }

    /// The game fingerprint is stable (dedup): independent of the toon order,
    /// sensitive to the content. An integer `length` is shown without `.0` (JS compat of the overlay).
    #[test]
    fn game_fingerprint_stable_et_ordre_independant() {
        let a = output("2026-06-09", "Sky Temple", 900.0, &["b", "a", "c"]);
        let b = output("2026-06-09", "Sky Temple", 900.0, &["c", "a", "b"]);
        assert_eq!(game_fingerprint(&a), game_fingerprint(&b), "toon order");
        let c = output("2026-06-09", "Sky Temple", 901.0, &["a", "b", "c"]);
        assert_ne!(
            game_fingerprint(&a),
            game_fingerprint(&c),
            "different length"
        );
        // without a match (rejection) → no fingerprint
        assert_eq!(
            game_fingerprint(&storm_stats::Output {
                hotkey_profiles: Default::default(),
                actions: Default::default(),
                status: -3,
                match_: None,
                players: None
            }),
            None
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod resume_tests {
    use crate::api_tests::{app, test_state_with_db};
    use crate::refdata::tests::{drop_db, isolated_db};
    use crate::AppState;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::time::Duration;
    use tower::ServiceExt;

    // Startup scans every pending row and parsing writes match/player rows. Neither should
    // run against other integration tests' uploads or fixtures.
    async fn isolated_state() -> Option<(AppState, String)> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let (db, name) = isolated_db().await.unwrap();
        Some((test_state_with_db(url, db, None), name))
    }

    fn committed_replay(name: &str) -> Vec<u8> {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../storm-replay/tests/data");
        std::fs::read(dir.join(name)).expect("committed replay")
    }

    async fn upload_row(db: &sqlx::PgPool, id: i64) -> (String, Option<String>, i32) {
        sqlx::query_as("SELECT status, error_class, parse_attempts FROM uploads WHERE id = $1")
            .bind(id)
            .fetch_one(db)
            .await
            .unwrap()
    }

    async fn wait_for_upload(db: &sqlx::PgPool, id: i64) -> (String, Option<String>, i32) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let row = upload_row(db, id).await;
                if row.0 != "pending" {
                    return row;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("upload remained pending")
    }

    #[tokio::test]
    async fn accepted_upload_recovers_from_transient_attempt_write_error() {
        let Some((state, db_name)) = isolated_state().await else {
            return;
        };
        let token = "upload-retry-test";
        sqlx::query("INSERT INTO upload_tokens (name, token_hash) VALUES ('retry', $1)")
            .bind(super::sha256_hex(token.as_bytes()))
            .execute(&state.db)
            .await
            .unwrap();
        // nextval survives rollback, so only the first bookkeeping UPDATE fails.
        sqlx::raw_sql(
            "CREATE SEQUENCE attempt_writes;
             CREATE FUNCTION fail_first_attempt_write() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF nextval('attempt_writes') = 1 THEN
                     RAISE EXCEPTION 'transient test failure' USING ERRCODE = '40001';
                 END IF;
                 RETURN NEW;
             END; $$;
             CREATE TRIGGER fail_first_attempt_write BEFORE UPDATE OF parse_attempts ON uploads
                 FOR EACH ROW EXECUTE FUNCTION fail_first_attempt_write();",
        )
        .execute(&state.db)
        .await
        .unwrap();
        let permit = state
            .parse_sem
            .clone()
            .acquire_many_owned(state.parse_sem.available_permits().try_into().unwrap())
            .await
            .unwrap();
        let bytes = committed_replay("2026-05-27 22.37.45 Industrial District.StormReplay");
        let response = app(&state)
            .oneshot(
                Request::post("/api/upload-raw")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(bytes))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let id = sqlx::query_scalar("SELECT id FROM uploads")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(upload_row(&state.db, id).await, ("pending".into(), None, 0));
        drop(permit);

        assert_eq!(
            wait_for_upload(&state.db, id).await,
            ("parsed".into(), None, 1)
        );
        let writes: i64 = sqlx::query_scalar("SELECT last_value FROM attempt_writes")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(
            writes, 2,
            "one failed write, then one recorded parse attempt"
        );
        drop_db(state.db, &db_name).await;
    }

    /// An upload left `pending` (202 then restart) is resumed and parsed at startup.
    #[tokio::test]
    async fn pending_upload_is_resumed_and_parsed() {
        let Some((state, db_name)) = isolated_state().await else {
            return;
        };
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("committed replay");
        let hash = super::sha256_hex(&bytes);
        let archived = state.cfg.archive_dir.join(format!("{hash}.StormReplay"));
        super::write_atomic(&archived, &bytes).await.unwrap();
        sqlx::query("DELETE FROM uploads WHERE fingerprint = $1")
            .bind(&hash)
            .execute(&state.db)
            .await
            .unwrap();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO uploads (filename, fingerprint, archived_path, status)
             VALUES ('resume.StormReplay', $1, $2, 'pending') RETURNING id",
        )
        .bind(&hash)
        .bind(archived.to_string_lossy().as_ref())
        .fetch_one(&state.db)
        .await
        .unwrap();

        assert_eq!(super::resume_pending(&state).await.unwrap(), 1);
        assert_eq!(
            wait_for_upload(&state.db, id).await,
            ("parsed".into(), None, 1)
        );
        // no temporary file left next to the archive
        let leftovers = std::fs::read_dir(&state.cfg.archive_dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("tmp-")
            })
            .count();
        assert_eq!(leftovers, 0);
        drop_db(state.db, &db_name).await;
    }

    /// Security review C1 (defence in depth): an upload whose parse kept killing the process is
    /// given up after `MAX_PARSE_ATTEMPTS` instead of being re-parsed at every startup; below the
    /// limit it is still resumed, and every parse attempt is counted.
    #[tokio::test]
    async fn pending_upload_is_given_up_after_max_attempts() {
        let Some((state, db_name)) = isolated_state().await else {
            return;
        };
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay");
        let bytes = std::fs::read(&replay).expect("committed replay");
        let hash = super::sha256_hex(&bytes);
        let archived = state.cfg.archive_dir.join(format!("{hash}.StormReplay"));
        super::write_atomic(&archived, &bytes).await.unwrap();
        // the same content under a second fingerprint: one row at the limit, one just below
        let exhausted_fp = format!("{hash}-exhausted");
        for fp in [&hash, &exhausted_fp] {
            sqlx::query("DELETE FROM uploads WHERE fingerprint = $1")
                .bind(fp)
                .execute(&state.db)
                .await
                .unwrap();
        }
        let insert = |fp: String, attempts: i32| {
            let db = state.db.clone();
            let path = archived.to_string_lossy().into_owned();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO uploads (filename, fingerprint, archived_path, status, parse_attempts)
                     VALUES ('attempts.StormReplay', $1, $2, 'pending', $3) RETURNING id",
                )
                .bind(fp)
                .bind(path)
                .bind(attempts)
                .fetch_one(&db)
                .await
                .unwrap()
            }
        };
        let exhausted = insert(exhausted_fp, super::MAX_PARSE_ATTEMPTS).await;
        let retried = insert(hash.clone(), super::MAX_PARSE_ATTEMPTS - 1).await;

        assert_eq!(super::resume_pending(&state).await.unwrap(), 1);

        let cap = super::MAX_PARSE_ATTEMPTS;
        let given_up = ("parse_failed".into(), Some("too_many_attempts".into()), cap);
        let parsed = ("parsed".into(), None, cap);
        assert_eq!(upload_row(&state.db, exhausted).await, given_up);
        assert_eq!(wait_for_upload(&state.db, retried).await, parsed);

        // A second startup leaves both rows alone.
        assert_eq!(super::resume_pending(&state).await.unwrap(), 0);
        assert_eq!(upload_row(&state.db, exhausted).await, given_up);
        assert_eq!(upload_row(&state.db, retried).await, parsed);
        drop_db(state.db, &db_name).await;
    }
}

/// M5 (lock across I/O, round 2): linking a parsed match to the open lobby must not hold the lobby
/// write lock across its database round trips, and must not stamp a lobby that was replaced while
/// the link was being computed. The linking query is held back deterministically by an
/// `ACCESS EXCLUSIVE` lock on `match_players` taken from another connection.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod lobby_link_tests {
    use super::link_lobby;
    use crate::api_tests::{call, test_state};
    use crate::lobby::{LobbyPlayerState, LobbyState};
    use crate::players::testutil::{insert_match, row, uid};
    use axum::http::StatusCode;
    use std::time::Duration;

    fn lobby(tags: &[String], hash: &str) -> LobbyState {
        let mut st = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        st.status = None;
        st.content_hash = hash.into();
        st.players = tags
            .iter()
            .map(|t| {
                let (name, disc) = t.split_once('#').unwrap();
                LobbyPlayerState {
                    name: name.into(),
                    discriminant: disc.into(),
                    battletag: t.clone(),
                    team: None,
                    team_manual: false,
                    toon_handle: None,
                    history: None,
                }
            })
            .collect();
        st
    }

    /// A match whose two players carry the BattleTags returned (fresh names: shared database).
    async fn tagged_match(db: &sqlx::PgPool) -> (i64, Vec<String>) {
        let (a, b) = (format!("a{}", &uid()[..10]), format!("b{}", &uid()[..10]));
        let (ta, tb) = (format!("toon-{}", uid()), format!("toon-{}", uid()));
        let id = insert_match(
            db,
            None,
            &[row(&ta, &a, 0, "Valla"), row(&tb, &b, 1, "Uther")],
        )
        .await;
        sqlx::query(
            "UPDATE match_players SET data = data || '{\"tag\":\"1234\"}' WHERE match_id = $1",
        )
        .bind(id)
        .execute(db)
        .await
        .unwrap();
        (id, vec![format!("{a}#1234"), format!("{b}#1234")])
    }

    /// Blocks `lier_match`'s read of `match_players` until the returned transaction ends, then
    /// waits until a backend is actually parked on that lock.
    async fn hold_match_players(db: &sqlx::PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
        let mut tx = db.begin().await.unwrap();
        sqlx::query("LOCK TABLE match_players IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx
    }

    async fn wait_link_query_blocked(db: &sqlx::PgPool) {
        for _ in 0..500 {
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE wait_event_type = 'Lock' AND query LIKE '%name || ''#'' || (data ->> ''tag'')%'",
            )
            .fetch_one(db)
            .await
            .unwrap();
            if n > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the lobby link query never reached the match_players lock");
    }

    #[tokio::test]
    async fn lobby_reads_are_not_blocked_while_a_match_is_being_linked() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let (match_id, tags) = tagged_match(&state.db).await;
        *state.lobby.write().await = Some(lobby(&tags, &format!("h-{}", uid())));
        let tx = hold_match_players(&state.db).await;
        let link = {
            let st = state.clone();
            tokio::spawn(async move { link_lobby(&st, match_id).await })
        };
        wait_link_query_blocked(&state.db).await;
        let read = tokio::time::timeout(
            Duration::from_secs(2),
            call(&state, "GET", "/api/lobby", None),
        )
        .await;
        tx.rollback().await.unwrap();
        link.await.unwrap();
        let (st, _) = read.expect("GET /api/lobby blocked behind the lobby link's DB round trips");
        assert_eq!(st, StatusCode::OK);
        // the link still lands once the database answers
        assert_eq!(
            state.lobby.read().await.as_ref().unwrap().match_id,
            Some(match_id)
        );
    }

    #[tokio::test]
    async fn a_lobby_replaced_during_the_link_is_not_stamped() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let (match_id, tags) = tagged_match(&state.db).await;
        *state.lobby.write().await = Some(lobby(&tags, &format!("old-{}", uid())));
        let tx = hold_match_players(&state.db).await;
        let link = {
            let st = state.clone();
            tokio::spawn(async move { link_lobby(&st, match_id).await })
        };
        wait_link_query_blocked(&state.db).await;
        // a new lobby blob (same ten players, next game) arrives while the link is in flight
        let new_hash = format!("new-{}", uid());
        let replaced = tokio::time::timeout(Duration::from_secs(2), async {
            *state.lobby.write().await = Some(lobby(&tags, &new_hash));
        })
        .await;
        tx.rollback().await.unwrap();
        link.await.unwrap();
        replaced.expect("lobby replacement blocked behind the lobby link's DB round trips");
        let g = state.lobby.read().await;
        let cur = g.as_ref().unwrap();
        assert_eq!(cur.content_hash, new_hash);
        assert_eq!(
            cur.match_id, None,
            "the replaced lobby must not inherit the old link"
        );
    }
}
