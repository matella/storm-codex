//! Browser I/O shared by the Rust front-end: fetch the series, follow the `/ws` live channel with
//! reconnection, 10 s safety-net poll. Same behaviour as `useLiveUpdates` + `useCast` in the React app.

use crate::model::CastSeries;
use futures_util::StreamExt;
use gloo_net::{
    http::Request,
    websocket::{futures::WebSocket, Message},
};
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use serde::Deserialize;

pub async fn fetch_cast() -> Result<CastSeries, String> {
    let r = Request::get("/api/cast")
        .send()
        .await
        .map_err(|e| e.to_string())?;
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

/// Write request with a JSON body. A non-OK answer rejects with the server's `{"error": "<code>"}` code, or
/// `http_<status>` (same contract as `send()` in castApi.ts).
pub async fn send_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: &B,
) -> Result<T, String> {
    let req = match method {
        "PUT" => Request::put(path),
        _ => Request::post(path),
    };
    let r = req
        .json(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.ok() {
        let status = r.status();
        #[derive(Deserialize)]
        struct E {
            error: String,
        }
        return Err(r
            .json::<E>()
            .await
            .map(|e| e.error)
            .unwrap_or_else(|_| format!("http_{status}")));
    }
    r.json::<T>().await.map_err(|e| e.to_string())
}

/// Calls `on_width` with the element's width now and every time it changes (`ResizeObserver`), when it is > 0.
pub fn observe_width(id: &str, on_width: impl Fn(f64) + 'static) {
    use wasm_bindgen::{closure::Closure, JsCast};
    let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    else {
        return;
    };
    let measure = {
        let el = el.clone();
        move || {
            let w = el.client_width() as f64;
            if w > 0.0 {
                on_width(w);
            }
        }
    };
    measure();
    let cb = Closure::<dyn FnMut()>::new(measure);
    if let Ok(ro) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        ro.observe(&el);
        std::mem::forget(ro);
    }
    cb.forget();
}

/// `fmtTime`: `dd/mm hh:mm` in the viewer's local time, French format.
pub fn fmt_time(iso: Option<&str>) -> String {
    let Some(iso) = iso else { return "—".into() };
    let opts = js_sys::Object::new();
    for k in ["day", "month", "hour", "minute"] {
        let _ = js_sys::Reflect::set(&opts, &k.into(), &"2-digit".into());
    }
    let d = js_sys::Date::new(&iso.into());
    d.to_locale_string("fr-FR", &opts)
        .as_string()
        .unwrap_or_default()
}

pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

pub fn search() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default()
}

/// `?skin=glass` -> `Some("glass")`.
pub fn skin_from_search(search: &str) -> Option<String> {
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix("skin="))
        .map(str::to_string)
}

pub fn set_body_transparent() {
    if let Some(b) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    {
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
    start_live_events(move |ev| {
        if ev != "draft.updated" {
            refresh()
        }
    });
}

/// Same channel for several consumers: `on` gets `"poll"` every 10 s, `"open"` after each (re)connection and
/// the `type` of every event received (`cast.updated`, `draft.updated`, ...).
pub fn start_live_events(on: impl Fn(&str) + Clone + 'static) {
    let poll = on.clone();
    Interval::new(10_000, move || poll("poll")).forget();
    wasm_bindgen_futures::spawn_local(async move {
        let loc = web_sys::window().map(|w| w.location());
        let host = loc.as_ref().and_then(|l| l.host().ok()).unwrap_or_default();
        let wss = loc
            .and_then(|l| l.protocol().ok())
            .is_some_and(|p| p == "https:");
        let url = format!("{}://{host}/ws", if wss { "wss" } else { "ws" });
        loop {
            if let Ok(mut ws) = WebSocket::open(&url) {
                on("open"); // reconnected: re-read what may have been missed
                while let Some(m) = ws.next().await {
                    match m {
                        Ok(Message::Text(t)) => {
                            if let Ok(e) = serde_json::from_str::<Event>(&t) {
                                on(&e.kind);
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

pub async fn fetch_draft() -> Result<crate::model::DraftState, String> {
    get_json("/api/draft").await
}

/// Recent heroes of the linked players (`None` query = nobody linked, nothing to fetch).
pub async fn fetch_lineup(query: &str) -> Result<crate::model::CastLineup, String> {
    get_json(&format!("/api/cast/lineup?{query}")).await
}
