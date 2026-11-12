//! HTTP calls of the cast control room (`castApi.ts` + the draft calls of `api.ts`). Server errors are
//! `{"error":"<code>"}` (409: a `CastError` variant such as `NotInPool`): they are returned as `Err(code)`.

use std::future::Future;

use leptos::{prelude::*, task::spawn_local, web_sys};
use serde_json::{json, Value};
use storm_web_core::{
    castctl::{scene_id, side_id},
    model::{Scene, Side, Team},
    net,
};

fn error_code(status: u16, body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("http_{status}"))
}

fn parse(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or(Value::Null)
}

async fn send(method: &str, url: &str, body: Option<String>) -> Result<Value, String> {
    let (s, t) = net::write(method, url, body, None).await?;
    if (200..300).contains(&s) {
        Ok(parse(&t))
    } else {
        Err(error_code(s, &t))
    }
}

async fn send_raw(
    url: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    file: &web_sys::File,
) -> Result<Value, String> {
    let (s, t) = net::write_raw(
        "POST",
        url,
        Some(content_type),
        headers,
        Some(file.clone().into()),
        None,
    )
    .await?;
    if (200..300).contains(&s) {
        Ok(parse(&t))
    } else {
        Err(error_code(s, &t))
    }
}

fn post(url: &str, body: Value) -> impl Future<Output = Result<Value, String>> {
    let (url, body) = (url.to_string(), body.to_string());
    async move { send("POST", &url, Some(body)).await }
}

// ── series ─────────────────────────────────────────────────────────────────────

pub async fn series(body: Value) -> Result<Value, String> {
    post("/api/cast/series", body).await
}

pub async fn teams(blue: &Team, red: &Team) -> Result<Value, String> {
    send(
        "PUT",
        "/api/cast/teams",
        Some(json!({ "blue": blue, "red": red }).to_string()),
    )
    .await
}

/// `status`: available | banned | picked | decider; `by` only for banned / picked.
pub async fn mark_map(name: &str, status: &str, by: Option<Side>) -> Result<Value, String> {
    let mut b = json!({ "name": name, "status": status });
    if let Some(s) = by {
        b["by"] = json!(side_id(s));
    }
    post("/api/cast/pool/map", b).await
}

pub async fn start_game(map: &str) -> Result<Value, String> {
    post("/api/cast/games/start", json!({ "map": map })).await
}

pub async fn winner(n: u32, side: Side) -> Result<Value, String> {
    post(
        &format!("/api/cast/games/{n}/winner"),
        json!({ "side": side_id(side) }),
    )
    .await
}

pub async fn remove_game(n: u32) -> Result<Value, String> {
    send("DELETE", &format!("/api/cast/games/{n}"), None).await
}

/// `None` = back to the computed score.
pub async fn score(s: Option<(u32, u32)>) -> Result<Value, String> {
    let body = match s {
        Some((b, r)) => json!({ "blue": b, "red": r }),
        None => Value::Null,
    };
    post("/api/cast/score", body).await
}

pub async fn scene(s: Scene) -> Result<Value, String> {
    post("/api/cast/scene", json!({ "scene": scene_id(s) })).await
}

pub async fn lineup_mode(mode: &str) -> Result<Value, String> {
    post("/api/cast/lineup-mode", json!({ "mode": mode })).await
}

pub async fn slide(patch: Value) -> Result<Value, String> {
    post("/api/cast/slide", patch).await
}

/// Body = the image bytes (png / jpeg / webp, up to 512 kB).
pub async fn logo_upload(side: Side, file: &web_sys::File) -> Result<Value, String> {
    send_raw(
        &format!("/api/cast/logo/{}", side_id(side)),
        &file.type_(),
        &[],
        file,
    )
    .await
}

/// Custom replay of game `n` → summary (nothing is written to the database).
pub async fn game_replay(n: u32, file: &web_sys::File, swap: bool) -> Result<Value, String> {
    let q = if swap { "?swap=1" } else { "" };
    send_raw(
        &format!("/api/cast/games/{n}/replay{q}"),
        "application/octet-stream",
        &[],
        file,
    )
    .await
}

/// Batch import, step 1: decodes a replay without writing anything.
pub async fn replay_inspect(file: &web_sys::File) -> Result<Value, String> {
    send_raw(
        "/api/cast/replays/inspect",
        "application/octet-stream",
        &[],
        file,
    )
    .await
}

/// Batch import, step 2: creates game `map` and fills it from the replay.
pub async fn replay_import(
    map: &str,
    file: &web_sys::File,
    swap: bool,
    series_id: &str,
) -> Result<Value, String> {
    let enc = storm_web_core::route::uri_component;
    let q = format!(
        "map={}&series_id={}{}",
        enc(map),
        enc(series_id),
        if swap { "&swap=1" } else { "" }
    );
    send_raw(
        &format!("/api/cast/replays/import?{q}"),
        "application/octet-stream",
        &[],
        file,
    )
    .await
}

// ── draft ──────────────────────────────────────────────────────────────────────

pub async fn draft_post(path: &str, body: Option<Value>) -> Result<Value, String> {
    send(
        "POST",
        &format!("/api/draft{path}"),
        body.map(|b| b.to_string()),
    )
    .await
}

pub async fn draft_replay_remove(i: usize) -> Result<Value, String> {
    send("DELETE", &format!("/api/draft/replay/queue/{i}"), None).await
}

pub async fn draft_replay_load(file: &web_sys::File) -> Result<Value, String> {
    let name = storm_web_core::route::uri_component(&file.name());
    send_raw(
        "/api/draft/replay/load",
        "application/octet-stream",
        &[("X-Filename", &name)],
        file,
    )
    .await
}

// ── runner ─────────────────────────────────────────────────────────────────────

/// Runs a server action of the console: the error (if any) goes to the banner, and the state is read again
/// (`run` in Cast.tsx; the response is not put in a cache, the whole series is fetched again instead).
#[derive(Clone, Copy)]
pub struct Runner {
    pub err: RwSignal<Option<String>>,
    pub refresh: RwSignal<u32>,
}

impl Runner {
    pub async fn exec<T>(self, f: impl Future<Output = Result<T, String>>) -> Option<T> {
        self.err.set(None);
        let out = match f.await {
            Ok(v) => Some(v),
            Err(e) => {
                self.err.set(Some(e));
                None
            }
        };
        self.refresh.update(|n| *n += 1);
        out
    }

    pub fn run<T: 'static>(self, f: impl Future<Output = Result<T, String>> + 'static) {
        spawn_local(async move {
            self.exec(f).await;
        });
    }
}
