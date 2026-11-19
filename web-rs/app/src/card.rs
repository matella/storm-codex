//! Shareable match card, browser part (spec `docs/specs/2026-11-19-match-card.md`): loads the card fonts, the
//! minimap and the hero portraits (same origin, so the canvas stays exportable), replays the display list of
//! `storm_web_core::card::layout` on an offscreen 1200 × 675 canvas and saves it as a PNG through
//! `clip::download_blob`. Everything is local: no upload, no share API.

use std::collections::HashMap;

use js_sys::{Array, Function, Promise, Reflect};
use leptos::{
    wasm_bindgen::{closure::Closure, JsCast, JsValue},
    web_sys,
};
use storm_web_core::card::{layout, Align, Card, Op, H, W};
use wasm_bindgen_futures::JsFuture;

use crate::ui::initials;

/// Images of the card: the minimap URL and, per hero, its portrait URL (reference data) and ring colour.
pub struct Art {
    pub minimap: String,
    pub heroes: HashMap<String, (Option<String>, String)>,
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Local calendar day (`YYYY-MM-DD`) of an ISO date, `None` when the browser cannot parse it.
pub fn local_day(iso: &str) -> Option<String> {
    let d = js_sys::Date::new(&JsValue::from_str(iso));
    if d.get_time().is_nan() {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date()
    ))
}

/// Calls `obj[name](...args)` and returns the result as a promise when it is one.
fn call_promise(obj: &JsValue, name: &str, args: &Array) -> Option<Promise> {
    let f: Function = Reflect::get(obj, &JsValue::from_str(name))
        .ok()?
        .dyn_into()
        .ok()?;
    f.apply(obj, args).ok()?.dyn_into().ok()
}

/// Loads the card fonts (`document.fonts.load` per font; fonts the page has not used yet are not loaded by
/// `fonts.ready` alone). Failures fall back to the system fonts of the `font` shorthand.
async fn load_fonts() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(fonts) = Reflect::get(&doc, &JsValue::from_str("fonts")) else {
        return;
    };
    if fonts.is_undefined() {
        return;
    }
    let started: Vec<Promise> = storm_web_core::card::Font::ALL
        .iter()
        .filter_map(|f| call_promise(&fonts, "load", &Array::of1(&JsValue::from_str(f.css()))))
        .collect();
    for p in started {
        // a missing face rejects: the fallback family of the shorthand is used
        let _ = JsFuture::from(p).await;
    }
}

/// Starts loading an image; the promise resolves on `load`, rejects on `error`.
fn start_image(url: &str) -> Option<(web_sys::HtmlImageElement, Promise)> {
    let img = web_sys::HtmlImageElement::new().ok()?;
    let target = img.clone();
    let src = url.to_string();
    let promise = Promise::new(&mut |res, rej| {
        target.set_onload(Some(res.unchecked_ref()));
        target.set_onerror(Some(rej.unchecked_ref()));
        target.set_src(&src);
    });
    Some((img, promise))
}

/// Loads every image in parallel; a failed one is `None` (initials / no backdrop).
async fn load_images(urls: Vec<(String, String)>) -> HashMap<String, web_sys::HtmlImageElement> {
    let started: Vec<_> = urls
        .into_iter()
        .filter_map(|(key, url)| start_image(&url).map(|(img, p)| (key, img, p)))
        .collect();
    let mut out = HashMap::new();
    for (key, img, p) in started {
        if JsFuture::from(p).await.is_ok() && img.natural_width() > 0 {
            out.insert(key, img);
        }
    }
    out
}

fn rounded(ctx: &web_sys::CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    ctx.begin_path();
    ctx.move_to(x + r, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
    let _ = ctx.arc_to(x, y + h, x, y, r);
    let _ = ctx.arc_to(x, y, x + w, y, r);
    ctx.close_path();
}

/// Draws `img` scaled to cover `w × h` (centre crop).
fn cover(
    ctx: &web_sys::CanvasRenderingContext2d,
    img: &web_sys::HtmlImageElement,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    let (iw, ih) = (
        f64::from(img.natural_width()),
        f64::from(img.natural_height()),
    );
    if iw == 0.0 || ih == 0.0 {
        return;
    }
    let s = (w / iw).max(h / ih);
    let (sw, sh) = (w / s, h / s);
    let _ = ctx.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        img,
        (iw - sw) / 2.0,
        (ih - sh) / 2.0,
        sw,
        sh,
        x,
        y,
        w,
        h,
    );
}

fn draw(
    ctx: &web_sys::CanvasRenderingContext2d,
    ops: &[Op],
    images: &HashMap<String, web_sys::HtmlImageElement>,
) {
    ctx.set_text_baseline("alphabetic");
    for op in ops {
        match op {
            Op::Backdrop { alpha } => {
                if let Some(img) = images.get("") {
                    ctx.save();
                    ctx.set_global_alpha(*alpha);
                    cover(ctx, img, 0.0, 0.0, W, H);
                    ctx.restore();
                }
            }
            Op::Rect {
                x,
                y,
                w,
                h,
                r,
                fill,
            } => {
                ctx.set_fill_style_str(fill);
                if *r > 0.0 {
                    rounded(ctx, *x, *y, *w, *h, *r);
                    ctx.fill();
                } else {
                    ctx.fill_rect(*x, *y, *w, *h);
                }
            }
            Op::Text {
                x,
                y,
                text,
                font,
                color,
                align,
            } => {
                ctx.set_font(font.css());
                ctx.set_fill_style_str(color);
                ctx.set_text_align(match align {
                    Align::Left => "left",
                    Align::Right => "right",
                    Align::Center => "center",
                });
                let _ = ctx.fill_text(text, *x, *y);
            }
            Op::Crown { x, y, w, color } => {
                // three-point crown on a band
                let h = w * 0.75;
                ctx.begin_path();
                ctx.move_to(*x, *y);
                ctx.line_to(*x, y - h);
                ctx.line_to(x + w * 0.27, y - h * 0.45);
                ctx.line_to(x + w * 0.5, y - h);
                ctx.line_to(x + w * 0.73, y - h * 0.45);
                ctx.line_to(x + w, y - h);
                ctx.line_to(x + w, *y);
                ctx.close_path();
                ctx.set_fill_style_str(color);
                ctx.fill();
            }
            Op::Portrait {
                hero,
                cx,
                cy,
                r,
                ring,
            } => {
                ctx.begin_path();
                let _ = ctx.arc(*cx, *cy, *r, 0.0, std::f64::consts::TAU);
                ctx.set_fill_style_str("#181a22");
                ctx.fill();
                match images.get(hero) {
                    Some(img) => {
                        ctx.save();
                        ctx.clip();
                        cover(ctx, img, cx - r, cy - r, 2.0 * r, 2.0 * r);
                        ctx.restore();
                    }
                    None => {
                        ctx.set_font(&format!(
                            "700 {}px Figtree, 'Segoe UI', sans-serif",
                            (r * 0.8).round()
                        ));
                        ctx.set_fill_style_str(ring);
                        ctx.set_text_align("center");
                        ctx.set_text_baseline("middle");
                        let _ = ctx.fill_text(&initials(hero), *cx, *cy + 1.0);
                        ctx.set_text_baseline("alphabetic");
                    }
                }
                ctx.begin_path();
                let _ = ctx.arc(*cx, *cy, *r, 0.0, std::f64::consts::TAU);
                ctx.set_line_width(3.0);
                ctx.set_stroke_style_str(ring);
                ctx.stroke();
            }
        }
    }
}

/// `canvas.toBlob("image/png")` as a future.
async fn to_png(canvas: &web_sys::HtmlCanvasElement) -> Result<web_sys::Blob, String> {
    let target = canvas.clone();
    let mut failed = None;
    let promise = Promise::new(&mut |res, rej| {
        let cb = Closure::once_into_js(move |b: JsValue| {
            let _ = if b.is_null() {
                rej.call1(&JsValue::NULL, &JsValue::from_str("empty canvas"))
            } else {
                res.call1(&JsValue::NULL, &b)
            };
        });
        if let Err(e) = target.to_blob_with_type(cb.unchecked_ref(), "image/png") {
            failed = Some(js_err(e));
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    JsFuture::from(promise)
        .await
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "not a blob".to_string())
}

/// Draws the card on a fresh canvas (returned so a caller can inspect it).
pub async fn render(card: &Card, art: &Art) -> Result<web_sys::HtmlCanvasElement, String> {
    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let canvas: web_sys::HtmlCanvasElement = doc
        .create_element("canvas")
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "no canvas".to_string())?;
    canvas.set_width(W as u32);
    canvas.set_height(H as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(js_err)?
        .ok_or("no 2d context")?
        .dyn_into()
        .map_err(|_| "no 2d context".to_string())?;
    let mut urls = vec![(String::new(), art.minimap.clone())];
    for (hero, (icon, _)) in &art.heroes {
        if let Some(icon) = icon {
            urls.push((hero.clone(), icon.clone()));
        }
    }
    load_fonts().await;
    let images = load_images(urls).await;
    let measure = |s: &str, f: storm_web_core::card::Font| {
        ctx.set_font(f.css());
        ctx.measure_text(s).map(|m| m.width()).unwrap_or(0.0)
    };
    let ring = |h: &str| {
        art.heroes
            .get(h)
            .map_or_else(|| "#afa9ec".to_string(), |(_, r)| r.clone())
    };
    let ops = layout(card, &measure, &ring);
    draw(&ctx, &ops, &images);
    Ok(canvas)
}

/// Renders the card and starts the PNG download; returns the file name.
pub async fn save(card: &Card, art: &Art) -> Result<String, String> {
    let canvas = render(card, art).await?;
    let blob = to_png(&canvas).await?;
    crate::clip::download_blob(&blob, &card.file_name);
    Ok(card.file_name.clone())
}
