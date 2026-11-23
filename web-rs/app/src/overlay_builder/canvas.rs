use super::{Editor, CUSTOM};
use leptos::{ev, html, prelude::*, wasm_bindgen::JsValue, web_sys};
use serde::{Deserialize, Serialize};
use storm_web_app::overlay_builder_model::{snap_delta, Rect};
use storm_web_core::{
    net,
    overlay_layout::{
        WidgetInstance, WidgetSettings, WidgetType, GRID_COLS, GRID_ROWS, MAX_WIDGETS,
    },
    route::app_href,
};

const DRAG_TYPE: &str = "application/x-storm-widget";

pub(super) fn label(kind: WidgetType) -> &'static str {
    match kind {
        WidgetType::Ticker => "Ticker",
        WidgetType::Widget => "Scorecard",
        WidgetType::Queue => "Queue",
        WidgetType::Recap => "Session recap",
        WidgetType::StreamPool => "Stream hero pool",
        WidgetType::PatchBanner => "Patch banner",
        WidgetType::RecordFlash => "Record flash",
        WidgetType::Nemesis => "Nemesis",
        WidgetType::Goal => "Daily goal",
    }
}

pub(super) fn kind(settings: &WidgetSettings) -> WidgetType {
    match settings {
        WidgetSettings::Ticker {} => WidgetType::Ticker,
        WidgetSettings::Widget(_) => WidgetType::Widget,
        WidgetSettings::Queue {} => WidgetType::Queue,
        WidgetSettings::Recap {} => WidgetType::Recap,
        WidgetSettings::StreamPool {} => WidgetType::StreamPool,
        WidgetSettings::PatchBanner {} => WidgetType::PatchBanner,
        WidgetSettings::RecordFlash {} => WidgetType::RecordFlash,
        WidgetSettings::Nemesis {} => WidgetType::Nemesis,
        WidgetSettings::Goal {} => WidgetType::Goal,
    }
}

#[derive(Deserialize)]
struct Ready {
    #[serde(rename = "type")]
    kind: String,
    version: u8,
}

#[derive(Serialize)]
struct Preview<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    widgets: &'a [WidgetInstance],
}

#[component]
fn PreviewFrame(widgets: Memo<Vec<WidgetInstance>>) -> impl IntoView {
    let frame = NodeRef::<html::Iframe>::new();
    let origin = StoredValue::new(net::origin());
    let ready = RwSignal::new(0_u32);
    let error = RwSignal::new(false);
    let listener = window_event_listener(ev::message, move |event: web_sys::MessageEvent| {
        if !origin.with_value(|expected| !expected.is_empty() && event.origin() == *expected) {
            return;
        }
        let Some(window) = frame
            .get_untracked()
            .and_then(|frame| frame.content_window())
        else {
            return;
        };
        if !event
            .source()
            .is_some_and(|source| js_sys::Object::is(source.as_ref(), window.as_ref()))
        {
            return;
        }
        let message = js_sys::JSON::stringify(&event.data())
            .ok()
            .and_then(|s| s.as_string());
        let ready_event = message.and_then(|s| serde_json::from_str::<Ready>(&s).ok());
        if ready_event.is_some_and(|r| r.kind == "storm-overlay-preview-ready" && r.version == 1) {
            ready.update(|n| *n = n.saturating_add(1));
        }
    });
    on_cleanup(move || listener.remove());
    Effect::new(move |_| {
        if ready.get() == 0 {
            return;
        }
        let Some(window) = frame
            .get_untracked()
            .and_then(|frame| frame.content_window())
        else {
            return;
        };
        let sent = widgets.with(|items| {
            let json = serde_json::to_string(&Preview {
                kind: "storm-overlay-preview",
                version: 1,
                widgets: items,
            })
            .ok()?;
            let msg: JsValue = js_sys::JSON::parse(&json).ok()?;
            origin
                .with_value(|target| window.post_message(&msg, target))
                .ok()
        });
        error.set(sent.is_none());
    });
    let settled = move || ready.get() > 0 && !error.get();
    view! {
        <iframe node_ref=frame src=format!("{}?preview=1", app_href(CUSTOM))
            title="Live overlay preview" aria-hidden="true" tabindex="-1" />
        <span class="ob-preview-status" role="status" hidden=settled>
            {move || if error.get() { "Could not update live preview." } else { "Waiting for live preview…" }}
        </span>
    }
}

#[component]
pub(super) fn Canvas(editor: Editor) -> impl IntoView {
    let canvas = NodeRef::<html::Div>::new();
    let widgets = Memo::new(move |_| editor.draft.with(|draft| draft.widgets.clone()));
    let full = move || widgets.with(|ws| ws.len() >= MAX_WIDGETS);
    let add = move |kind, x, y| {
        if editor.busy.get_untracked() {
            return;
        }
        let id = editor.fresh_key();
        let added = editor
            .draft
            .try_update(|draft| draft.add(id.clone(), kind, x, y))
            .unwrap_or_default();
        if added {
            editor.selected.set(Some(id));
            editor.notice.set("");
        } else {
            editor.notice.set(if full() {
                "This layout already has 100 widgets."
            } else {
                "No free room for that widget. Move, resize, or remove a widget first."
            });
        }
    };
    view! {
        <section aria-label="Widget editor">
            <div class="ob-palette" role="group" aria-label="Widget palette">
                {WidgetType::ALL.into_iter().enumerate().map(|(index, kind)| view! {
                    <button class="btn" type="button" aria-label=format!("Add {}", label(kind))
                        draggable=move || !editor.busy.get() && !full() disabled=full
                        on:click=move |_| add(kind, 0, 0)
                        on:dragstart=move |event: web_sys::DragEvent| {
                            if let Some(data) = event.data_transfer() {
                                data.set_effect_allowed("copy");
                                let _ = data.set_data(DRAG_TYPE, &index.to_string());
                            }
                        }>{label(kind)}</button>
                }).collect_view()}
            </div>
            <p class="ob-hint">"Drag a widget onto the grid, or click to add it. Arrow keys move or resize a focused handle by one cell."</p>
            <div class="ob-canvas" node_ref=canvas
                on:click=move |_| editor.selected.set(None)
                on:dragover=move |event: web_sys::DragEvent| {
                    if let Some(data) = event.data_transfer() {
                        if !editor.busy.get_untracked() && data.types().includes(&DRAG_TYPE.into(), 0) {
                            event.prevent_default();
                            data.set_drop_effect("copy");
                        }
                    }
                }
                on:drop=move |event: web_sys::DragEvent| {
                    let Some(index) = event.data_transfer().and_then(|data| data.get_data(DRAG_TYPE).ok()).and_then(|s| s.parse::<usize>().ok()) else { return; };
                    let (Some(kind), Some(canvas)) = (WidgetType::ALL.get(index), canvas.get_untracked()) else { return; };
                    event.prevent_default();
                    let bounds = canvas.get_bounding_client_rect();
                    let x = snap_delta(f64::from(event.client_x()) - bounds.x(), bounds.width(), GRID_COLS);
                    let y = snap_delta(f64::from(event.client_y()) - bounds.y(), bounds.height(), GRID_ROWS);
                    add(*kind, x, y);
                }>
                <PreviewFrame widgets=widgets />
                <For each=move || widgets.with(|items| items.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
                    key=String::clone children=move |id| view! { <Handles editor=editor canvas=canvas id=id /> } />
            </div>
        </section>
    }
}

#[component]
fn Handles(editor: Editor, canvas: NodeRef<html::Div>, id: String) -> impl IntoView {
    let id = StoredValue::new(id);
    let placement = Memo::new(move |_| {
        editor.draft.with(|draft| {
            id.with_value(|id| {
                draft
                    .widgets
                    .iter()
                    .find(|w| &w.id == id)
                    .map(|w| w.placement.clone())
            })
        })
    });
    let selected =
        Memo::new(move |_| id.with_value(|id| editor.selected.with(|s| s.as_ref() == Some(id))));
    let name = placement.with_untracked(|p| {
        p.as_ref()
            .map(|p| label(kind(&p.settings)))
            .unwrap_or("Widget")
    });
    view! {
        <div class="ob-handles" class:ob-selected=selected style=move || placement.with(|p| p.as_ref().map(|p| {
            let pct = |n, extent| f64::from(n) * 100.0 / f64::from(extent);
            format!("left:{}%;top:{}%;width:{}%;height:{}%;z-index:{}", pct(p.grid_x, GRID_COLS), pct(p.grid_y, GRID_ROWS),
                pct(p.grid_w, GRID_COLS), pct(p.grid_h, GRID_ROWS), if selected.get() { 101 } else { p.z_order + 1 })
        }).unwrap_or_default())>
            <Handle editor=editor id=id canvas=canvas name=name resize=false />
            <Handle editor=editor id=id canvas=canvas name=name resize=true />
        </div>
    }
}

#[derive(Clone, Copy)]
struct Gesture {
    pointer: i32,
    start_x: i32,
    start_y: i32,
    rect: Rect,
}

#[component]
fn Handle(
    editor: Editor,
    id: StoredValue<String>,
    canvas: NodeRef<html::Div>,
    name: &'static str,
    resize: bool,
) -> impl IntoView {
    let button = NodeRef::<html::Button>::new();
    let gesture = RwSignal::new(None::<Gesture>);
    let select = move || editor.selected.set(Some(id.get_value()));
    let rect = move || {
        editor.draft.with_untracked(|draft| {
            id.with_value(|id| {
                draft
                    .widgets
                    .iter()
                    .find(|w| &w.id == id)
                    .map(|w| Rect::from(&w.placement))
            })
        })
    };
    let transform = move |start: Rect, dx, dy| {
        if editor.busy.get_untracked() {
            return;
        }
        let new = start.drag(dx, dy, resize);
        if rect() != Some(new) {
            editor.draft.update(|draft| {
                id.with_value(|id| {
                    if let Some(instance) = draft.widgets.iter_mut().find(|w| &w.id == id) {
                        new.apply(&mut instance.placement);
                    }
                })
            });
        }
    };
    view! {
        <button node_ref=button type="button" class=if resize { "ob-resize" } else { "ob-move" }
            aria-label=format!("{} {name}", if resize { "Resize" } else { "Move" })
            aria-pressed=move || (!resize).then(|| id.with_value(|id| editor.selected.with(|s| s.as_ref() == Some(id))).to_string())
            on:click=move |event| { event.stop_propagation(); select(); }
            on:pointerdown=move |event: web_sys::PointerEvent| {
                if event.button() != 0 || !event.is_primary() || editor.busy.get_untracked() { return; }
                let (Some(button), Some(rect)) = (button.get_untracked(), rect()) else { return; };
                select();
                let pointer = event.pointer_id();
                if button.set_pointer_capture(pointer).is_ok() {
                    gesture.set(Some(Gesture { pointer, start_x: event.client_x(), start_y: event.client_y(), rect }));
                }
            }
            on:pointermove=move |event: web_sys::PointerEvent| {
                let (Some(g), Some(canvas)) = (gesture.get_untracked(), canvas.get_untracked()) else { return; };
                if event.pointer_id() != g.pointer { return; }
                let bounds = canvas.get_bounding_client_rect();
                transform(g.rect, snap_delta(f64::from(event.client_x() - g.start_x), bounds.width(), GRID_COLS),
                    snap_delta(f64::from(event.client_y() - g.start_y), bounds.height(), GRID_ROWS));
            }
            on:lostpointercapture=move |_| gesture.set(None)
            on:keydown=move |event: web_sys::KeyboardEvent| {
                if matches!(event.key().as_str(), "Backspace" | "Delete") {
                    event.prevent_default();
                    // Acts on the current selection only (click or the inspector's "Selected
                    // widget" dropdown), never on whichever handle merely has keyboard focus —
                    // Tab-ing through handles must stay non-destructive.
                    editor.remove_selected();
                    return;
                }
                let (dx, dy) = match event.key().as_str() {
                    "ArrowLeft" => (-1, 0), "ArrowRight" => (1, 0), "ArrowUp" => (0, -1), "ArrowDown" => (0, 1), _ => return,
                };
                event.prevent_default();
                select();
                if let Some(rect) = rect() { transform(rect, dx, dy); }
            }>
            {if resize { "" } else { name }}
        </button>
    }
}
