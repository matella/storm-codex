//! The same transparent stage serves saved OBS layouts and the builder's read-only iframe.

use crate::{feeds::Feeds, goal, nemesis, patch_banner, recap, recordflash, sessions, stream_pool};
use leptos::{
    ev,
    prelude::*,
    task::spawn_local,
    wasm_bindgen::{closure::Closure, JsCast, JsValue},
    web_sys::{
        js_sys::{Array, Object, Reflect, JSON},
        ResizeObserver, ResizeObserverEntry,
    },
};
use storm_web_core::{
    aggregates::patch_banner_visible,
    net,
    overlay_layout::{
        valid_text, OverlayLayout, WidgetInstance, WidgetSettings, GRID_CELL_PX, GRID_COLS,
        GRID_ROWS, MAX_WIDGETS,
    },
    route::query_value,
};
use storm_web_sources::{fit::scale_to_fit, options::WidgetOptions};

const WIDTH: i32 = GRID_COLS * GRID_CELL_PX;
const HEIGHT: i32 = GRID_ROWS * GRID_CELL_PX;
const PREVIEW: &str = "storm-overlay-preview";
const READY: &str = "storm-overlay-preview-ready";

pub fn render() -> impl IntoView {
    let search = net::search();
    let widgets = RwSignal::new(Vec::new());
    let message = RwSignal::new(None);
    let label = RwSignal::new(None::<String>);
    let layout_id = if query_value(&search, "preview").as_deref() == Some("1") {
        if !listen_for_preview(widgets) {
            message.set(Some("Preview is only available inside the builder"));
        }
        None
    } else {
        let id = query_value(&search, "id")
            .filter(|id| is_uuid(id))
            .map(|id| id.to_ascii_lowercase());
        if let Some(id) = id.clone() {
            message.set(Some("Loading layout…"));
            spawn_local(async move {
                let result = net::get_json_or_not_found::<OverlayLayout>(&format!(
                    "/api/overlay-layouts/{id}"
                ))
                .await;
                match result {
                    Ok(Some(layout))
                        if layout.summary.id == id
                            && !layout.summary.name.trim().is_empty()
                            && valid_text(&layout.summary.name)
                            && valid_widgets(&layout.widgets)
                            && layout.widgets.iter().all(|w| is_uuid(&w.id)) =>
                    {
                        label.try_set(Some(layout.summary.name));
                        widgets.try_set(layout.widgets);
                        message.try_set(None);
                    }
                    Ok(None) => {
                        message.try_set(Some("Layout not found"));
                    }
                    _ => {
                        message.try_set(Some("Unable to load layout"));
                    }
                }
            });
        } else {
            message.set(Some("Layout not found"));
        }
        id
    };
    let feeds = Feeds::new(widgets.read_only());
    let ordered = Memo::new(move |_| {
        widgets.with(|items| {
            let mut items: Vec<_> = items.iter().collect();
            items.sort_by(|a, b| (a.placement.z_order, &a.id).cmp(&(b.placement.z_order, &b.id)));
            items.into_iter().map(|w| w.id.clone()).collect::<Vec<_>>()
        })
    });
    view! {
        <div class="custom-stage" aria-label=move || label.get()>
            <For each=move || ordered.get() key=|id| id.clone() children=move |id| {
                placement(widgets.read_only(), id, feeds, layout_id.clone())
            } />
            {move || message.get().map(|text| view! { <p role="status" class="custom-status">{text}</p> })}
        </div>
    }
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn valid_widgets(widgets: &[WidgetInstance]) -> bool {
    let mut ids = std::collections::HashSet::new();
    widgets.len() <= MAX_WIDGETS
        && widgets.iter().all(|w| {
            let p = &w.placement;
            !w.id.trim().is_empty()
                && valid_text(&w.id)
                && ids.insert(&w.id)
                && (0..GRID_COLS).contains(&p.grid_x)
                && (0..GRID_ROWS).contains(&p.grid_y)
                && (1..=GRID_COLS - p.grid_x).contains(&p.grid_w)
                && (1..=GRID_ROWS - p.grid_y).contains(&p.grid_h)
        })
}

fn listen_for_preview(widgets: RwSignal<Vec<WidgetInstance>>) -> bool {
    let window = window();
    let Some(parent) = window
        .parent()
        .ok()
        .flatten()
        .filter(|p| !Object::is(p.as_ref(), window.as_ref()))
    else {
        return false;
    };
    let origin = net::origin();
    let peer = parent.clone();
    let expected_origin = origin.clone();
    let listener = window_event_listener(ev::message, move |event| {
        if event.origin() != expected_origin
            || !event
                .source()
                .is_some_and(|s| Object::is(s.as_ref(), peer.as_ref()))
        {
            return;
        }
        if let Some(next) = parse_snapshot(event.data()) {
            widgets.set(next);
        }
    });
    on_cleanup(move || listener.remove());
    let ready = Object::new();
    let _ = Reflect::set(&ready, &"type".into(), &READY.into());
    let _ = Reflect::set(&ready, &"version".into(), &1.into());
    let _ = parent.post_message(&ready, &origin);
    true
}

fn property(value: &JsValue, key: &str) -> Option<JsValue> {
    Reflect::get(value, &key.into()).ok()
}

fn object(value: &JsValue, max_keys: u32) -> Option<&Object> {
    let obj = value.dyn_ref::<Object>()?;
    (!Array::is_array(value) && Object::keys(obj).length() <= max_keys).then_some(obj)
}

fn parse_snapshot(data: JsValue) -> Option<Vec<WidgetInstance>> {
    // Bound the depth and size of the JSON conversion before serializing structured clone data.
    // The wire decoder below is responsible for required field names and their exact types.
    object(&data, 3)?;
    if property(&data, "type")? != PREVIEW || property(&data, "version")?.as_f64()? != 1.0 {
        return None;
    }
    let array = property(&data, "widgets")?.dyn_into::<Array>().ok()?;
    if array.length() as usize > MAX_WIDGETS {
        return None;
    }
    for item in array.iter() {
        let fields = Object::values(object(&item, 8)?);
        for field in fields.iter() {
            if primitive(&field) {
                continue;
            }
            if !Object::values(object(&field, 2)?)
                .iter()
                .all(|v| primitive(&v))
            {
                return None;
            }
        }
    }
    let json = JSON::stringify(array.as_ref()).ok()?.as_string()?;
    let widgets: Vec<WidgetInstance> = serde_json::from_str(&json).ok()?;
    valid_widgets(&widgets).then_some(widgets)
}

fn primitive(value: &JsValue) -> bool {
    value
        .dyn_ref::<leptos::web_sys::js_sys::JsString>()
        .is_some_and(|s| s.length() <= 240)
        || value.as_f64().is_some_and(f64::is_finite)
        || value.as_bool().is_some()
}

fn placement(
    widgets: ReadSignal<Vec<WidgetInstance>>,
    id: String,
    feeds: Feeds,
    layout_id: Option<String>,
) -> impl IntoView {
    let patch_key = layout_id.map(|layout| format!("custom_overlay_patch:{layout}:{id}"));
    let row = Memo::new(move |_| {
        widgets.with(|items| {
            items
                .iter()
                .find(|w| w.id == id)
                .map(|w| w.placement.clone())
        })
    });
    let settings = Memo::new(move |_| row.with(|p| p.as_ref().map(|p| p.settings.clone())));
    let style = move || {
        row.with(|p| {
            p.as_ref().map(|p| {
                format!(
                    "left:{}px;top:{}px;width:{}px;height:{}px;z-index:{}",
                    p.grid_x * GRID_CELL_PX,
                    p.grid_y * GRID_CELL_PX,
                    p.grid_w * GRID_CELL_PX,
                    p.grid_h * GRID_CELL_PX,
                    p.z_order,
                )
            })
        })
    };
    view! {
        <div class="custom-placement" style=style data-widget-type=move || settings.with(|s| s.as_ref().map(kind))>
            {move || settings.get().map(|s| {
                let content = match s {
                    WidgetSettings::Queue {} => {
                        let fit = move || row.with(|p| p.as_ref().map(|p| {
                            let x = f64::from(p.grid_w * GRID_CELL_PX) / f64::from(WIDTH);
                            let y = f64::from(p.grid_h * GRID_CELL_PX) / f64::from(HEIGHT);
                            format!("transform:scale({})", x.min(y))
                        }));
                        return view! { <div class="custom-queue" style=fit>{sessions::queue_content(feeds.names, feeds.recent, feeds.voice)}</div> }.into_any();
                    }
                    WidgetSettings::Ticker {} => sessions::ticker_content(feeds.names, feeds.recent).into_any(),
                    WidgetSettings::Widget(o) => sessions::widget_content(WidgetOptions { me: o.me.map(Into::into) }, feeds.names, feeds.newest).into_any(),
                    WidgetSettings::Recap {} => recap::recap_content(feeds.recap).into_any(),
                    WidgetSettings::StreamPool {} => stream_pool::stream_pool_content(feeds.review).into_any(),
                    WidgetSettings::PatchBanner {} => patch_banner::patch_banner_content(visible_patch(feeds, patch_key.clone())).into_any(),
                    WidgetSettings::RecordFlash {} => recordflash::record_flash_content(feeds.flash).into_any(),
                    WidgetSettings::Nemesis {} => nemesis::nemesis_content(feeds.grudges).into_any(),
                    WidgetSettings::Goal {} => goal::goal_content(feeds.progress).into_any(),
                };
                fitted_content(content, move || row.with(|p| p.as_ref().map(|p| (
                    f64::from(p.grid_w * GRID_CELL_PX), f64::from(p.grid_h * GRID_CELL_PX),
                )))).into_any()
            })}
        </div>
    }
}

fn fitted_content(
    content: impl IntoView,
    bounds: impl Fn() -> Option<(f64, f64)> + Send + 'static,
) -> impl IntoView {
    let node = NodeRef::<leptos::html::Div>::new();
    let measured = RwSignal::new((0.0, 0.0));
    node.on_load(move |element| {
        // The wrapper has no padding or border. Its layout size is independent of both
        // the placement's clip and the transform, so scaling cannot trigger another resize.
        let callback = Closure::<dyn FnMut(Array)>::new(move |entries: Array| {
            if let Some(entry) = entries.get(0).dyn_ref::<ResizeObserverEntry>() {
                let rect = entry.content_rect();
                measured.try_set((rect.width(), rect.height()));
            }
        });
        if let Ok(observer) = ResizeObserver::new(callback.as_ref().unchecked_ref()) {
            observer.observe(&element);
            let observer = StoredValue::new_local((observer, callback));
            on_cleanup(move || {
                observer.with_value(|(observer, _callback)| observer.disconnect());
            });
        }
    });
    let transform = move || {
        let size = measured.get();
        let scale = bounds()
            .and_then(|bounds| scale_to_fit(bounds, size))
            .unwrap_or(0.0);
        format!("scale({scale})")
    };
    view! {
        <div node_ref=node
            style="position:absolute;top:0;left:0;width:max-content;display:flex;flex-direction:column;transform-origin:top left"
            style:transform=transform
        >
            {content}
        </div>
    }
}

fn kind(settings: &WidgetSettings) -> &'static str {
    match settings {
        WidgetSettings::Queue {} => "queue",
        WidgetSettings::Ticker {} => "ticker",
        WidgetSettings::Widget(_) => "widget",
        WidgetSettings::Recap {} => "recap",
        WidgetSettings::StreamPool {} => "stream-pool",
        WidgetSettings::PatchBanner {} => "patch-banner",
        WidgetSettings::RecordFlash {} => "record-flash",
        WidgetSettings::Nemesis {} => "nemesis",
        WidgetSettings::Goal {} => "goal",
    }
}

fn visible_patch(
    feeds: Feeds,
    key: Option<String>,
) -> Memo<Option<storm_web_core::aggregates::PatchItem>> {
    // Freeze the acknowledgement read for this mount, so displaying the patch does not immediately
    // hide it. Preview never touches storage; it has only this mount's reactive visibility state.
    let old = key.as_deref().and_then(net::storage_get);
    let visible = Memo::new(move |_| {
        feeds.patch.with(|p| {
            let p = p.as_ref()?;
            patch_banner_visible(
                &p.internal_id,
                p.live_date.as_deref().map(net::ts),
                net::now_ms() as f64,
                old.as_deref(),
            )
            .then(|| p.clone())
        })
    });
    if let Some(key) = key {
        Effect::new(move |_| {
            if let Some(p) = visible.get() {
                net::storage_set(&key, &p.internal_id)
            }
        });
    }
    visible
}
