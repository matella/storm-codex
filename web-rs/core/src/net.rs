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

/// `GET path` where `204 No Content` means "nothing" (`None`), as `fetchLive` does.
pub async fn get_json_opt<T: serde::de::DeserializeOwned>(path: &str) -> Result<Option<T>, String> {
    let r = Request::get(path).send().await.map_err(|e| e.to_string())?;
    if r.status() == 204 {
        return Ok(None);
    }
    if !r.ok() {
        return Err(format!("{path} → {}", r.status()));
    }
    r.json::<T>().await.map(Some).map_err(|e| e.to_string())
}

/// `n.toLocaleString("fr-FR")`.
pub fn fmt_n(n: f64) -> String {
    js_sys::Number::from(n)
        .to_locale_string("fr-FR")
        .as_string()
        .unwrap_or_default()
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
        let _ = b.style().set_property("margin", "0");
    }
}

/// An event of the `/ws` channel with the few fields the toasts show (`map` of a parsed match, `name` of a patch).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LiveEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
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
    start_live_full(move |e| on(&e.kind));
}

/// Same as `start_live_events`, with the whole event (the synthetic `poll` and `open` events have only a `kind`).
pub fn start_live_full(on: impl Fn(&LiveEvent) + Clone + 'static) {
    let kind = |k: &str| LiveEvent {
        kind: k.into(),
        ..LiveEvent::default()
    };
    let poll = on.clone();
    Interval::new(10_000, move || poll(&kind("poll"))).forget();
    wasm_bindgen_futures::spawn_local(async move {
        let loc = web_sys::window().map(|w| w.location());
        let host = loc.as_ref().and_then(|l| l.host().ok()).unwrap_or_default();
        let wss = loc
            .and_then(|l| l.protocol().ok())
            .is_some_and(|p| p == "https:");
        let url = format!("{}://{host}/ws", if wss { "wss" } else { "ws" });
        loop {
            if let Ok(mut ws) = WebSocket::open(&url) {
                on(&kind("open")); // reconnected: re-read what may have been missed
                while let Some(m) = ws.next().await {
                    match m {
                        Ok(Message::Text(t)) => {
                            if let Ok(e) = serde_json::from_str::<LiveEvent>(&t) {
                                on(&e);
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

pub fn pathname() -> String {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default()
}

/// Local calendar day `YYYY-MM-DD` of an ISO date, in the viewer's time zone (not UTC: a game played in the
/// evening stays on the right local day).
pub fn local_day(iso: &str) -> String {
    day_of(&js_sys::Date::new(&iso.into()))
}

pub fn today_local() -> String {
    day_of(&js_sys::Date::new_0())
}

fn day_of(d: &js_sys::Date) -> String {
    format!(
        "{}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date()
    )
}

/// `GET /api/matches?limit=n`, most recent first.
pub async fn fetch_matches(limit: u32) -> Result<Vec<crate::session::MatchSummary>, String> {
    get_json(&format!("/api/matches?limit={limit}")).await
}

pub async fn fetch_settings() -> Result<crate::session::Settings, String> {
    get_json("/api/settings").await
}

/// `GET /api/now-playing`, as the proxy answers it (parsed by `music::parse_track`). Like the React source,
/// any body that is JSON is read, whatever the status.
pub async fn fetch_now_playing() -> Result<serde_json::Value, String> {
    let r = Request::get("/api/now-playing")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    r.json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())
}

/// `fmtDay` of the queue panel: a `YYYY-MM-DD` key as `Mon 09 Nov` (en-GB).
pub fn fmt_day(key: &str) -> String {
    let mut it = key.split('-').filter_map(|p| p.parse::<f64>().ok());
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
        return key.to_string();
    };
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"weekday".into(), &"short".into());
    let _ = js_sys::Reflect::set(&opts, &"day".into(), &"2-digit".into());
    let _ = js_sys::Reflect::set(&opts, &"month".into(), &"short".into());
    js_sys::Date::new_with_year_month_day(y as u32, m as i32 - 1, d as i32)
        .to_locale_date_string("en-GB", &opts)
        .as_string()
        .unwrap_or_default()
}

/// `localStorage.getItem`; `None` when absent or when storage is blocked.
pub fn storage_get(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}

/// `localStorage.setItem`; a blocked storage is ignored (the page works without it).
pub fn storage_set(key: &str, value: &str) {
    if let Some(Some(s)) = web_sys::window().map(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, value);
    }
}

/// `localStorage.removeItem`.
pub fn storage_remove(key: &str) {
    if let Some(Some(s)) = web_sys::window().map(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item(key);
    }
}

/// `location.origin` (shown in the onboarding as the OBS source URLs).
pub fn origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default()
}

/// Runs `f` once after `ms` milliseconds (`setTimeout`).
pub fn after_ms(ms: u32, f: impl FnOnce() + 'static) {
    gloo_timers::callback::Timeout::new(ms, f).forget();
}

/// Local midnight of a `YYYY-MM-DD` day (plus `plus_days`) as an ISO string; `None` for a date the browser cannot read.
pub fn local_midnight_iso(day: &str, plus_days: u32) -> Option<String> {
    let date = js_sys::Date::new(&format!("{day}T00:00:00").into());
    if date.get_time().is_nan() {
        return None;
    }
    date.set_date(date.get_date() + plus_days);
    Some(date.to_iso_string().into())
}

/// Query string of an aggregate request: the dates are local midnights converted to ISO, `to` made inclusive
/// (`aggParams`).
pub fn agg_params(f: &crate::aggregates::AggFilter) -> String {
    let from = f.from.as_deref().and_then(|d| local_midnight_iso(d, 0));
    let to = f.to.as_deref().and_then(|d| local_midnight_iso(d, 1));
    crate::aggregates::agg_query(f, from.as_deref(), to.as_deref())
}

/// Epoch milliseconds of an ISO date (`new Date(iso).getTime()`).
pub fn ts(iso: &str) -> f64 {
    js_sys::Date::new(&iso.into()).get_time()
}

fn locale(iso: &str, locale: &str, keys: &[(&str, &str)], date_only: bool) -> String {
    let opts = js_sys::Object::new();
    for (k, v) in keys {
        let _ = js_sys::Reflect::set(&opts, &(*k).into(), &(*v).into());
    }
    let d = js_sys::Date::new(&iso.into());
    let s = if date_only {
        d.to_locale_date_string(locale, &opts)
    } else {
        d.to_locale_string(locale, &opts)
    };
    s.as_string().unwrap_or_default()
}

/// `21:05` of an ISO date in local time (fr-FR, two digits).
pub fn hhmm(iso: &str) -> String {
    locale(
        iso,
        "fr-FR",
        &[("hour", "2-digit"), ("minute", "2-digit")],
        false,
    )
}

/// `Mon, 9 November 2026` (en-GB) of an ISO date.
pub fn long_date(iso: &str) -> String {
    locale(
        iso,
        "en-GB",
        &[
            ("weekday", "short"),
            ("day", "numeric"),
            ("month", "long"),
            ("year", "numeric"),
        ],
        true,
    )
}

/// `05 Oct 26` (en-GB, two digits) of an ISO date, a dash without one (`fmtDate` of Trends).
pub fn fmt_date_short(iso: Option<&str>) -> String {
    match iso {
        Some(i) if !i.is_empty() => locale(
            i,
            "en-GB",
            &[("day", "2-digit"), ("month", "short"), ("year", "2-digit")],
            true,
        ),
        _ => "—".into(),
    }
}

/// `DateBlock` of the patch list: the local day of month (two digits) and `Mon YYYY` (en-GB).
pub fn date_block(iso: &str) -> (String, String) {
    let d = js_sys::Date::new(&iso.into());
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"month".into(), &"short".into());
    let month: String = d
        .to_locale_date_string("en-GB", &opts)
        .as_string()
        .unwrap_or_default();
    (
        format!("{:02}", d.get_date()),
        format!("{month} {}", d.get_full_year()),
    )
}
