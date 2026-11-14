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

/// CPU-bound parse (spawn_blocking) + projection + `uploads` update. Broadcasts the WS event.
async fn run_parse(
    state: AppState,
    upload_id: i64,
    archived: PathBuf,
    _permit: tokio::sync::OwnedSemaphorePermit,
) -> Outcome {
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

            // If this match is the game of the open lobby, the companion switches to debrief. Lock
            // held for the read (`lier_match` queries `match_players` under this lock), the
            // mutation and the persistence (same pattern as `muter` in `lobby/api.rs`): the
            // WS broadcast happens after the explicit `drop`.
            let mut lobby_lie = false;
            let mut guard = state.lobby.write().await;
            if let Some(st) = guard.as_mut() {
                if crate::lobby::lier_match(&state.db, st, match_id).await {
                    if let Err(e) = crate::lobby::store::save(&state.db, st).await {
                        tracing::error!("lobby save: {e}");
                    }
                    // pending rank readings of this session (the lobby hash = session id)
                    if let Err(e) =
                        crate::rank::attach_to_match(&state.db, &st.content_hash, match_id).await
                    {
                        tracing::error!("rank attach: {e}");
                    }
                    lobby_lie = true;
                }
            }
            drop(guard);
            if lobby_lie {
                let _ = state
                    .events
                    .send(serde_json::json!({ "type": "lobby.updated" }));
            }

            // real-time push (milestone 3 T5)
            let _ = state.events.send(serde_json::json!({
                "type": "match.parsed",
                "match_id": match_id,
                "map": out.match_.as_ref().and_then(|m| m.get("map")).cloned(),
            }));
            // Jarvis event (milestone 5) — best-effort, does not affect the response
            let event = crate::jarvis::match_completed_event(match_id, &out);
            if let Some(url) = &state.cfg.redis_url {
                let (url, chan) = (url.clone(), state.cfg.jarvis_channel.clone());
                let ev = event.clone();
                tokio::spawn(async move { crate::jarvis::publish(&url, &chan, &ev).await });
            }
            // push post-game box→Azure (extension Twitch) — optionnel
            if let Some(url) = &state.cfg.azure_push_url {
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
/// `project_match` is idempotent. Returns the number of uploads queued again.
pub(crate) async fn resume_pending(state: &AppState) -> Result<usize, sqlx::Error> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, archived_path FROM uploads
         WHERE status = 'pending' AND archived_path IS NOT NULL ORDER BY id",
    )
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
    use crate::api_tests::test_state;

    /// An upload left `pending` (202 then restart) is resumed and parsed at startup.
    #[tokio::test]
    async fn pending_upload_is_resumed_and_parsed() {
        let Some(state) = test_state(None).await else {
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

        let n = super::resume_pending(&state).await.unwrap();
        assert!(n >= 1, "the pending upload must be queued again");
        let mut status = String::new();
        for _ in 0..100 {
            status = sqlx::query_scalar("SELECT status FROM uploads WHERE id = $1")
                .bind(id)
                .fetch_one(&state.db)
                .await
                .unwrap();
            if status != "pending" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(status, "parsed");
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
    }
}
