//! Two small spike checks, framework-free (plain web-sys):
//! 1. Canvas: draw a map and moving dots like the 2D replay viewer, record 2 s with `MediaRecorder`.
//! 2. Sanitising: sanitise patch-notes-like HTML (with hostile bits) and insert it.
//! Results are written into `#result`.

use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{BlobEvent, CanvasRenderingContext2d, HtmlCanvasElement, MediaRecorder};

const SAMPLE: &str = r##"<h2 id="heroes">Heroes</h2>
<ul><li><strong>Valla</strong>: Hungering Arrow damage increased from 100 to 110</li>
<li>Nerf: <em>Gall</em> Twilight Dream cooldown 60 -> 70</li></ul>
<h3 id="bugfix">Bug fixes</h3>
<p>Fixed an issue where <a href="https://heroespatchnotes.com/x" title="notes">a link</a> failed.</p>
<script>window.__pwned = 1</script>
<img src="x" onerror="window.__pwned = 2">
<a href="javascript:window.__pwned = 3">bad link</a>
<iframe src="https://example.com"></iframe>
<style>body { display: none }</style>
<p onclick="window.__pwned = 4">click</p>"##;

fn doc() -> web_sys::Document {
    web_sys::window().expect("window").document().expect("document")
}

fn report(line: &str) {
    let el = doc().get_element_by_id("result").expect("#result");
    el.set_text_content(Some(&format!("{}{line}\n", el.text_content().unwrap_or_default())));
}

fn sanitise_check() {
    let clean = ammonia::Builder::default().add_generic_attributes(["id"]).clean(SAMPLE).to_string();
    let host = doc().get_element_by_id("patch").expect("#patch");
    host.set_inner_html(&clean);
    let win = web_sys::window().expect("window");
    let pwned = js_sys::Reflect::get(&win, &"__pwned".into()).map(|v| !v.is_undefined()).unwrap_or(false);
    let q = |s: &str| host.query_selector(s).ok().flatten().is_some();
    report(&format!(
        "sanitise: {} -> {} bytes; script={} img-onerror={} js-href={} iframe={} style={} onclick={} keeps h2#heroes={} ul={} a[href]={}; script ran={pwned}",
        SAMPLE.len(),
        clean.len(),
        q("script"),
        host.inner_html().contains("onerror"),
        host.inner_html().contains("javascript:"),
        q("iframe"),
        q("style"),
        host.inner_html().contains("onclick"),
        q("h2#heroes"),
        q("ul"),
        q("a[href^=https]"),
    ));
}

fn draw(ctx: &CanvasRenderingContext2d, t: f64) {
    let (w, h) = (640.0, 640.0);
    ctx.set_fill_style_str("#1b2a1f");
    ctx.fill_rect(0.0, 0.0, w, h);
    ctx.set_stroke_style_str("#3d6b4a");
    ctx.set_line_width(6.0);
    ctx.stroke_rect(40.0, 40.0, w - 80.0, h - 80.0);
    for i in 0..10 {
        let a = t / 700.0 + i as f64;
        let (x, y) = (w / 2.0 + (w / 3.0) * a.cos() * (0.4 + 0.06 * i as f64), h / 2.0 + (h / 3.0) * a.sin() * (0.4 + 0.06 * i as f64));
        ctx.set_fill_style_str(if i < 5 { "#2f6df6" } else { "#e0414b" });
        ctx.begin_path();
        let _ = ctx.arc(x, y, 9.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
}

fn canvas_check() {
    let canvas: HtmlCanvasElement = doc().get_element_by_id("map").expect("#map").dyn_into().expect("canvas");
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d").ok().flatten().expect("2d").dyn_into().expect("2d ctx");
    // draw loop, like the viewer's requestAnimationFrame loop
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let c2 = ctx.clone();
    *g.borrow_mut() = Some(Closure::new(move |t: f64| {
        draw(&c2, t);
        let _ = web_sys::window().expect("window").request_animation_frame(f.borrow().as_ref().expect("cb").as_ref().unchecked_ref());
    }));
    let _ = web_sys::window().expect("window").request_animation_frame(g.borrow().as_ref().expect("cb").as_ref().unchecked_ref());

    let supported = js_sys::Reflect::has(&js_sys::global(), &"MediaRecorder".into()).unwrap_or(false);
    if !supported {
        report("canvas: MediaRecorder missing");
        return;
    }
    let stream = match canvas.capture_stream_with_frame_request_rate(30.0) {
        Ok(s) => s,
        Err(e) => return report(&format!("canvas: captureStream failed: {e:?}")),
    };
    let rec = MediaRecorder::new_with_media_stream(&stream).expect("recorder");
    let chunks: Rc<RefCell<Vec<web_sys::Blob>>> = Rc::default();
    let c = chunks.clone();
    let on_data = Closure::<dyn FnMut(BlobEvent)>::new(move |e: BlobEvent| {
        if let Some(b) = e.data() {
            if b.size() > 0.0 {
                c.borrow_mut().push(b);
            }
        }
    });
    rec.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
    on_data.forget();
    let r2 = rec.clone();
    let on_stop = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        let total: f64 = chunks.borrow().iter().map(|b| b.size()).sum();
        report(&format!("canvas: recorded {} chunk(s), {total} bytes, type {}", chunks.borrow().len(), r2.mime_type()));
    });
    rec.set_onstop(Some(on_stop.as_ref().unchecked_ref()));
    on_stop.forget();
    rec.start().expect("start");
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(2_000).await;
        let _ = rec.stop();
    });
}

fn main() {
    sanitise_check();
    canvas_check();
}
