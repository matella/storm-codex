//! Browser I/O shared by both spike apps: fetch the series, follow the `/ws` live channel with
//! reconnection, 10 s safety-net poll. Same behaviour as `useLiveUpdates` + `useCast` in the React app.

use crate::model::CastSeries;
use futures_util::StreamExt;
use gloo_net::{http::Request, websocket::{futures::WebSocket, Message}};
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use serde::Deserialize;

pub async fn fetch_cast() -> Result<CastSeries, String> {
    let r = Request::get("/api/cast").send().await.map_err(|e| e.to_string())?;
    if !r.ok() {
        return Err(format!("HTTP {}", r.status()));
    }
    r.json::<CastSeries>().await.map_err(|e| e.to_string())
}

/// `GET path` decoded as JSON; the error text is shown to the user (`path -> status`, like `get()` in api.ts).
pub async fn get_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let r = Request::get(path).send().await.map_err(|e| e.to_string())?;
    if !r.ok() {
        return Err(format!("{path} → {}", r.status()));
    }
    r.json::<T>().await.map_err(|e| e.to_string())
}

/// `fmtTime`: `dd/mm hh:mm` in the viewer's local time, French format.
pub fn fmt_time(iso: Option<&str>) -> String {
    let Some(iso) = iso else { return "—".into() };
    let opts = js_sys::Object::new();
    for k in ["day", "month", "hour", "minute"] {
        let _ = js_sys::Reflect::set(&opts, &k.into(), &"2-digit".into());
    }
    let d = js_sys::Date::new(&iso.into());
    d.to_locale_string("fr-FR", &opts).as_string().unwrap_or_default()
}

pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

pub fn search() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// `?skin=glass` -> `Some("glass")`.
pub fn skin_from_search(search: &str) -> Option<String> {
    search.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("skin=")).map(str::to_string)
}

pub fn set_body_transparent() {
    if let Some(b) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
        let _ = b.style().set_property("background", "transparent");
    }
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
}

/// Calls `refresh` on every `cast.updated`, after each (re)connection, and every 10 s.
pub fn start_live(refresh: impl Fn() + Clone + 'static) {
    let poll = refresh.clone();
    Interval::new(10_000, move || poll()).forget();
    wasm_bindgen_futures::spawn_local(async move {
        let loc = web_sys::window().map(|w| w.location());
        let host = loc.as_ref().and_then(|l| l.host().ok()).unwrap_or_default();
        let wss = loc.and_then(|l| l.protocol().ok()).is_some_and(|p| p == "https:");
        let url = format!("{}://{host}/ws", if wss { "wss" } else { "ws" });
        loop {
            if let Ok(mut ws) = WebSocket::open(&url) {
                refresh(); // reconnected: re-read what may have been missed
                while let Some(m) = ws.next().await {
                    match m {
                        Ok(Message::Text(t)) => {
                            if serde_json::from_str::<Event>(&t).is_ok_and(|e| e.kind == "cast.updated") {
                                refresh();
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            TimeoutFuture::new(2_000).await;
        }
    });
}
