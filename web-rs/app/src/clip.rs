//! Export of a replay interval as a webm: records the live canvas while the viewer plays the interval. Port of
//! `web/src/clipExport.ts` (the pure `clipFrames` lives in `core/replay2d.rs`).

use std::{cell::RefCell, rc::Rc};

use js_sys::{Array, Promise, Reflect};
use leptos::{
    wasm_bindgen::{closure::Closure, JsCast, JsValue},
    web_sys,
};
use wasm_bindgen_futures::JsFuture;

const CANDIDATES: [&str; 3] = [
    "video/webm;codecs=vp9",
    "video/webm;codecs=vp8",
    "video/webm",
];

/// `MediaRecorder` and `captureStream` are not guaranteed (old browsers, some WebViews).
pub fn supported() -> bool {
    let global = js_sys::global();
    let get = |t: &JsValue, k: &str| Reflect::get(t, &JsValue::from_str(k)).ok();
    let defined = |v: &Option<JsValue>| {
        v.as_ref()
            .is_some_and(|v| !v.is_undefined() && !v.is_null())
    };
    let recorder = get(&global, "MediaRecorder");
    let canvas = get(&global, "HTMLCanvasElement");
    let capture = canvas
        .as_ref()
        .and_then(|c| get(c, "prototype"))
        .and_then(|p| get(&p, "captureStream"));
    defined(&recorder) && defined(&canvas) && capture.is_some_and(|f| f.is_function())
}

/// A running recording of a canvas.
pub struct Recording {
    recorder: web_sys::MediaRecorder,
    chunks: Rc<RefCell<Vec<web_sys::Blob>>>,
    _on_data: Closure<dyn FnMut(web_sys::BlobEvent)>,
}

impl Recording {
    /// Starts recording the canvas video stream at `fps`.
    pub fn start(canvas: &web_sys::HtmlCanvasElement, fps: f64) -> Result<Self, String> {
        let stream = canvas
            .capture_stream_with_frame_request_rate(fps)
            .map_err(|_| "captureStream failed".to_string())?;
        let mime = CANDIDATES
            .iter()
            .find(|m| web_sys::MediaRecorder::is_type_supported(m));
        let recorder = match mime {
            Some(m) => {
                let opts = web_sys::MediaRecorderOptions::new();
                opts.set_mime_type(m);
                web_sys::MediaRecorder::new_with_media_stream_and_media_recorder_options(
                    &stream, &opts,
                )
            }
            None => web_sys::MediaRecorder::new_with_media_stream(&stream),
        }
        .map_err(|_| "MediaRecorder failed".to_string())?;
        let chunks: Rc<RefCell<Vec<web_sys::Blob>>> = Rc::default();
        let sink = chunks.clone();
        let on_data =
            Closure::<dyn FnMut(web_sys::BlobEvent)>::new(move |e: web_sys::BlobEvent| {
                if let Some(b) = e.data() {
                    if b.size() > 0.0 {
                        sink.borrow_mut().push(b);
                    }
                }
            });
        recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
        recorder
            .start()
            .map_err(|_| "could not start".to_string())?;
        Ok(Self {
            recorder,
            chunks,
            _on_data: on_data,
        })
    }

    /// Stops the recorder and resolves with the whole blob once `onstop` has fired (the last chunk is not lost).
    pub async fn stop(self) -> Result<web_sys::Blob, String> {
        let chunks = self.chunks.clone();
        let recorder = self.recorder.clone();
        let promise = Promise::new(&mut move |resolve, reject| {
            let reject2 = reject.clone();
            let on_error = Closure::once_into_js(move |_e: JsValue| {
                let _ = reject2.call1(&JsValue::NULL, &JsValue::from_str("MediaRecorder error"));
            });
            recorder.set_onerror(Some(on_error.unchecked_ref()));
            let (rec2, chunks2) = (recorder.clone(), chunks.clone());
            let on_stop = Closure::once_into_js(move |_e: JsValue| {
                let parts = js_sys_array(&chunks2.borrow());
                let opts = web_sys::BlobPropertyBag::new();
                let mime = rec2.mime_type();
                opts.set_type(if mime.is_empty() { "video/webm" } else { &mime });
                match web_sys::Blob::new_with_blob_sequence_and_options(&parts, &opts) {
                    Ok(blob) => {
                        let _ = resolve.call1(&JsValue::NULL, &blob);
                    }
                    Err(e) => {
                        let _ = reject.call1(&JsValue::NULL, &e);
                    }
                }
            });
            recorder.set_onstop(Some(on_stop.unchecked_ref()));
            let _ = recorder.stop();
        });
        let v = JsFuture::from(promise)
            .await
            .map_err(|_| "recording failed".to_string())?;
        v.dyn_into::<web_sys::Blob>()
            .map_err(|_| "recording failed".to_string())
    }
}

fn js_sys_array(blobs: &[web_sys::Blob]) -> JsValue {
    let arr = Array::new();
    for b in blobs {
        arr.push(b);
    }
    arr.into()
}

/// Starts the download of a blob through a temporary `<a download>`.
pub fn download_blob(blob: &web_sys::Blob, filename: &str) {
    let Ok(url) = web_sys::Url::create_object_url_with_blob(blob) else {
        return;
    };
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Ok(a) = doc.create_element("a") {
            if let Ok(a) = a.dyn_into::<web_sys::HtmlAnchorElement>() {
                a.set_href(&url);
                a.set_download(filename);
                a.click();
            }
        }
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}
