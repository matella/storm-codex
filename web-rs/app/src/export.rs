//! Export HTML of a scouting report, browser part: loads the portraits and minimaps, shrinks them to WebP data URIs
//! (the file stays readable offline), renders through the pure `storm_web_core::reportexport` and starts the download.
//! Port of `web/src/reportExport.ts`.

use std::collections::BTreeMap;

use js_sys::{Array, Object, Promise, Reflect};
use leptos::{
    wasm_bindgen::{JsCast, JsValue},
    web_sys,
};
use storm_web_core::{
    aggregates::minimap_image,
    logic::map_image,
    net,
    reportexport::{export_file_name, heroes_of, maps_of, render_report_html, ExportAssets},
    scouting::ScoutingReport,
};
use wasm_bindgen_futures::{future_to_promise, JsFuture};

/// What the operator is told once the file is saved.
pub struct Saved {
    pub file_name: String,
    pub bytes: usize,
    pub portraits: usize,
    pub heroes: usize,
    pub maps: usize,
}

/// A hero of the report with what the export needs from the reference data.
pub struct HeroInfo {
    pub name: String,
    pub icon: Option<String>,
    pub ring: &'static str,
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Loads an image element from an object URL (resolves on `load`, rejects on `error`).
async fn load_image(url: &str) -> Result<web_sys::HtmlImageElement, String> {
    let img = web_sys::HtmlImageElement::new().map_err(js_err)?;
    let target = img.clone();
    let src = url.to_string();
    let promise = Promise::new(&mut |res, rej| {
        target.set_onload(Some(res.unchecked_ref()));
        target.set_onerror(Some(rej.unchecked_ref()));
        target.set_src(&src);
    });
    JsFuture::from(promise)
        .await
        .map_err(|_| "image failed to load".to_string())?;
    Ok(img)
}

/// Remote image → WebP data URI shrunk to `max` px on its longest side (portraits are cropped square), `None` when
/// the image cannot be had.
async fn to_data_uri(url: &str, max: f64, square: bool) -> Option<String> {
    let win = web_sys::window()?;
    let res: web_sys::Response = JsFuture::from(win.fetch_with_str(url))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    let is_image = res
        .headers()
        .get("content-type")
        .ok()
        .flatten()
        .is_some_and(|t| t.starts_with("image/"));
    if !res.ok() || !is_image {
        return None;
    }
    let blob: web_sys::Blob = JsFuture::from(res.blob().ok()?)
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    let object_url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    let img = load_image(&object_url).await;
    let _ = web_sys::Url::revoke_object_url(&object_url);
    let img = img.ok()?;
    let (w, h) = (
        f64::from(img.natural_width()),
        f64::from(img.natural_height()),
    );
    if w == 0.0 || h == 0.0 {
        return None;
    }
    let side = if square { w.min(h) } else { 0.0 };
    let (sw, sh) = if square { (side, side) } else { (w, h) };
    let scale = (max / sw.max(sh)).min(1.0);
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()?
        .document()?
        .create_element("canvas")
        .ok()?
        .dyn_into()
        .ok()?;
    let (cw, ch) = ((sw * scale).round(), (sh * scale).round());
    canvas.set_width(cw as u32);
    canvas.set_height(ch as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    ctx.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        &img,
        (w - sw) / 2.0,
        (h - sh) / 2.0,
        sw,
        sh,
        0.0,
        0.0,
        cw,
        ch,
    )
    .ok()?;
    canvas
        .to_data_url_with_type_and_encoder_options("image/webp", &JsValue::from_f64(0.82))
        .ok()
}

/// Runs the downloads in parallel; one slot per URL, in order.
async fn load_all(jobs: Vec<(String, f64, bool)>) -> Vec<Option<String>> {
    let promises = Array::new();
    for (url, max, square) in jobs {
        promises.push(&future_to_promise(async move {
            Ok(to_data_uri(&url, max, square)
                .await
                .map_or(JsValue::NULL, |s| JsValue::from_str(&s)))
        }));
    }
    match JsFuture::from(Promise::all(&promises)).await {
        Ok(v) => Array::from(&v).iter().map(|x| x.as_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// `11 Nov 2026, 10:42` — `toLocaleString("en-GB", {dateStyle: "medium", timeStyle: "short"})`.
fn generated_at() -> String {
    let opts = Object::new();
    let _ = Reflect::set(&opts, &"dateStyle".into(), &"medium".into());
    let _ = Reflect::set(&opts, &"timeStyle".into(), &"short".into());
    js_sys::Date::new_0()
        .to_locale_string("en-GB", &opts)
        .as_string()
        .unwrap_or_default()
}

/// Starts the download of the file through a temporary `<a download>`; the object URL is released 10 s later.
fn save(html: &str, file_name: &str) -> Result<(), String> {
    let parts = Array::of1(&JsValue::from_str(html));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/html;charset=utf-8");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).map_err(js_err)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_err)?;
    let a: web_sys::HtmlAnchorElement = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?
        .create_element("a")
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "no anchor".to_string())?;
    a.set_href(&url);
    a.set_download(file_name);
    a.click();
    net::after_ms(10_000, move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    Ok(())
}

/// Builds and saves the report. `info` gives, for each hero, its portrait URL and ring colour (reference data).
pub async fn export_report(r: &ScoutingReport, info: &[HeroInfo]) -> Result<Saved, String> {
    let heroes = heroes_of(r);
    let maps = maps_of(r);
    let mut assets = ExportAssets::default();
    let mut hero_jobs = Vec::new();
    let mut hero_names = Vec::new();
    for h in &heroes {
        let known = info.iter().find(|i| &i.name == h);
        assets
            .ring
            .insert(h.clone(), known.map_or("#afa9ec", |i| i.ring).to_string());
        if let Some(icon) = known.and_then(|i| i.icon.clone()) {
            hero_jobs.push((icon, 128.0, true));
            hero_names.push(h.clone());
        }
    }
    let portraits = load_all(hero_jobs).await;
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for (h, uri) in hero_names.into_iter().zip(portraits) {
        if let Some(u) = uri {
            got.insert(h, u);
        }
    }
    assets.hero = got;
    // minimap first, painted battleground art as a fallback
    let minimaps = load_all(
        maps.iter()
            .map(|m| (minimap_image(m), 900.0, false))
            .collect(),
    )
    .await;
    let missing: Vec<usize> = (0..maps.len())
        .filter(|i| minimaps.get(*i).cloned().flatten().is_none())
        .collect();
    let fallback = load_all(
        missing
            .iter()
            .map(|i| (map_image(&maps[*i]), 900.0, false))
            .collect(),
    )
    .await;
    for (i, m) in maps.iter().enumerate() {
        let uri = minimaps.get(i).cloned().flatten().or_else(|| {
            missing
                .iter()
                .position(|x| *x == i)
                .and_then(|p| fallback.get(p).cloned().flatten())
        });
        if let Some(u) = uri {
            assets.map.insert(m.clone(), u);
        }
    }
    let html = render_report_html(r, &assets, &generated_at());
    let file_name = export_file_name(&r.title);
    save(&html, &file_name)?;
    Ok(Saved {
        file_name,
        bytes: html.len(),
        portraits: assets.hero.len(),
        heroes: heroes.len(),
        maps: assets.map.len(),
    })
}
