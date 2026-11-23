mod canvas;
mod inspector;

use leptos::{ev, prelude::*, web_sys};
use serde::de::DeserializeOwned;
use storm_web_app::overlay_builder_model::Draft;
use storm_web_core::{
    net,
    overlay_layout::{
        simulation::Simulations, valid_text, OverlayLayout, OverlayLayoutSummary, SaveOverlayLayout,
    },
    route::app_href,
};
use wasm_bindgen_futures::spawn_local;

const API: &str = "/api/admin/overlay-layouts";
const CUSTOM: &str = "/overlay/custom";

async fn request(
    method: &str,
    path: &str,
    body: Option<SaveOverlayLayout>,
) -> Result<String, String> {
    let body = body
        .map(|data| serde_json::to_string(&data))
        .transpose()
        .map_err(|_| "Could not encode the layout.")?;
    let (status, text) = net::write(method, path, body, None)
        .await
        .map_err(|_| "Could not reach the local server.")?;
    match status {
        200..=299 => Ok(text),
        401 => Err("Admin credentials required. Enter the local admin token in Admin, then reload layouts.".into()),
        404 => Err("That saved layout no longer exists.".into()),
        _ => Err(format!("The server could not complete the request (HTTP {status}).")),
    }
}

async fn read<T: DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<SaveOverlayLayout>,
) -> Result<T, String> {
    serde_json::from_str(&request(method, path, body).await?)
        .map_err(|_| "The server returned an unreadable layout response.".into())
}

#[derive(Clone, Copy)]
struct Editor {
    draft: RwSignal<Draft>,
    selected: RwSignal<Option<String>>,
    simulate: RwSignal<Simulations>,
    serial: RwSignal<u64>,
    saved: RwSignal<SaveOverlayLayout>,
    layouts: RwSignal<Vec<OverlayLayoutSummary>>,
    available: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    notice: RwSignal<&'static str>,
}

impl Editor {
    fn new() -> Self {
        let draft = Draft::default();
        Self {
            saved: RwSignal::new(draft.payload()),
            draft: RwSignal::new(draft),
            selected: RwSignal::new(None),
            simulate: RwSignal::new(Simulations::default()),
            serial: RwSignal::new(0),
            layouts: RwSignal::new(Vec::new()),
            available: RwSignal::new(false),
            busy: RwSignal::new(false),
            error: RwSignal::new(None),
            notice: RwSignal::new(""),
        }
    }

    fn fresh_key(self) -> String {
        self.serial.update(|n| *n += 1);
        format!("draft-{}", self.serial.get_untracked())
    }

    fn dirty(self) -> bool {
        self.draft
            .with(|draft| self.saved.with(|saved| draft.payload() != *saved))
    }

    fn can_discard(self) -> bool {
        !untrack(|| self.dirty()) || net::confirm("Discard unsaved layout changes?")
    }

    fn set_draft(self, draft: Draft) {
        self.selected.set(None);
        self.simulate.set(Simulations::default());
        self.saved.set(draft.payload());
        self.draft.set(draft);
    }

    /// Removes the selected widget, if any (used by both the inspector's "Remove widget" button
    /// and the canvas handle's Backspace/Delete key). Clears that widget kind's simulate flag so
    /// a later re-added widget of the same kind starts with Simulate off, regardless of which
    /// removal path was used.
    fn remove_selected(self) {
        let Some(id) = self.selected.get_untracked() else {
            return;
        };
        self.selected.set(None);
        self.draft.update(|draft| {
            if let Some(kind) = draft
                .widgets
                .iter()
                .find(|w| w.id == id)
                .map(|w| canvas::kind(&w.placement.settings))
                .and_then(storm_web_core::overlay_layout::simulation::Kind::from_widget)
            {
                self.simulate.update(|state| state.set(kind, false));
            }
            draft.widgets.retain(|widget| widget.id != id);
            draft.normalize_layers();
        });
    }

    fn begin(self) -> bool {
        if self.busy.get_untracked() {
            return false;
        }
        self.busy.set(true);
        self.error.set(None);
        self.notice.set("");
        true
    }

    fn finish<T>(self, result: Result<T, String>, accept: impl FnOnce(T)) {
        // Don't apply a response once the route (and its signals) has been disposed.
        if self.busy.try_set(false).is_some() {
            return;
        }
        match result {
            Ok(value) => accept(value),
            Err(error) => self.error.set(Some(error)),
        }
    }

    fn reload(self) {
        if !self.begin() {
            return;
        }
        spawn_local(async move {
            let result = read("GET", API, None).await;
            self.finish(result, |layouts| {
                self.layouts.set(layouts);
                self.available.set(true);
            });
        });
    }

    fn open(self, id: String) {
        if !self.can_discard() || !self.begin() {
            return;
        }
        spawn_local(async move {
            let result: Result<OverlayLayout, _> = read("GET", &format!("{API}/{id}"), None).await;
            self.finish(result, |layout| {
                let mut draft = Draft {
                    id: Some(layout.summary.id),
                    name: layout.summary.name,
                    widgets: layout.widgets,
                };
                for instance in &mut draft.widgets {
                    instance.id = self.fresh_key();
                }
                draft.normalize_layers();
                self.set_draft(draft);
            });
        });
    }

    fn save(self) {
        let (id, payload) = self
            .draft
            .with_untracked(|draft| (draft.id.clone(), draft.payload()));
        if payload.name.is_empty() || !valid_text(&payload.name) || !self.begin() {
            return;
        }
        let (method, url) = match id {
            Some(id) => ("PUT", format!("{API}/{id}")),
            None => ("POST", API.into()),
        };
        spawn_local(async move {
            let result: Result<OverlayLayout, _> = read(method, &url, Some(payload)).await;
            self.finish(result, |layout| {
                self.draft.update(|draft| draft.apply_save(&layout));
                self.saved.set(self.draft.with_untracked(Draft::payload));
                self.layouts.update(|layouts| {
                    layouts.retain(|old| old.id != layout.summary.id);
                    layouts.insert(0, layout.summary);
                });
            });
        });
    }

    fn delete(self) {
        let Some(id) = self.draft.with_untracked(|draft| draft.id.clone()) else {
            return;
        };
        if !net::confirm("Delete this saved layout and discard any unsaved changes? Its OBS URL will stop working.") || !self.begin() { return; }
        spawn_local(async move {
            let result = request("DELETE", &format!("{API}/{id}"), None).await;
            self.finish(result, |_| {
                self.layouts
                    .update(|layouts| layouts.retain(|old| old.id != id));
                self.set_draft(Draft::default());
                self.notice.set("Layout deleted.");
            });
        });
    }

    fn obs_url(self) -> Option<String> {
        self.draft.with(|draft| {
            draft
                .id
                .as_ref()
                .map(|id| format!("{}{}?id={id}", net::origin(), app_href(CUSTOM)))
        })
    }
}

#[component]
pub fn OverlayBuilder() -> impl IntoView {
    let editor = Editor::new();
    editor.reload();
    let unload = window_event_listener(ev::beforeunload, move |event| {
        if untrack(|| editor.dirty()) {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let current_id = Memo::new(move |_| editor.draft.with(|draft| draft.id.clone()));
    let name_valid = move || {
        editor.draft.with(|draft| {
            let name = draft.name.trim();
            !name.is_empty() && valid_text(name)
        })
    };
    view! {
        <section class="ob-page">
            <header>
                <div class="ds-kicker">"OBS Browser Source"</div>
                <h1 class="ds-title" tabindex="-1">"Overlay Builder"</h1>
                <p>"Arrange widgets on a 1920 × 1080 canvas. OBS uses the last saved layout."</p>
            </header>
            <div class="ob-toolbar">
                <label for="ob-field-saved-layouts">"Saved layouts"</label>
                <select id="ob-field-saved-layouts" disabled=move || editor.busy.get() || !editor.available.get() on:change=move |ev| {
                    let target = event_target::<web_sys::HtmlSelectElement>(&ev);
                    let id = target.value();
                    target.set_value(current_id.get_untracked().as_deref().unwrap_or_default());
                    if !id.is_empty() { editor.open(id); }
                }>
                    <option value="" selected=move || current_id.get().is_none()>"Select a saved layout"</option>
                    {move || editor.layouts.get().into_iter().map(|summary| {
                        let id = summary.id;
                        view! { <option value=id.clone() selected=move || current_id.with(|current| current.as_ref() == Some(&id))>{summary.name}</option> }
                    }).collect_view()}
                </select>
                <button type="button" class="btn" disabled=move || editor.busy.get() on:click=move |_| editor.reload()>"Reload layouts"</button>
                <button type="button" class="btn" disabled=move || editor.busy.get() || !editor.available.get()
                    on:click=move |_| if editor.can_discard() {
                        editor.error.set(None);
                        editor.notice.set("");
                        editor.set_draft(Draft::default());
                    }>"New layout"</button>
            </div>
            <Show when=move || editor.error.with(Option::is_some)>
                <div class="ob-error" role="alert">{move || editor.error.get()} " " <a href=app_href("/admin")>"Open Admin"</a></div>
            </Show>
            <div role="status">{move || editor.notice.get()}</div>
            <Show when=move || editor.busy.get() && !editor.available.get()><p role="status">"Loading layouts…"</p></Show>
            <Show when=move || editor.available.get()>
                <fieldset class="ob-editor" disabled=move || editor.busy.get()>
                    <legend class="ob-sr">"Layout editor"</legend>
                    <div class="ob-toolbar">
                        <label for="ob-field-layout-name">"Layout name"</label>
                        <input id="ob-field-layout-name" type="text" prop:value=move || editor.draft.with(|draft| draft.name.clone())
                            on:input=move |ev| editor.draft.update(|draft| draft.name = event_target_value(&ev)) />
                        <button type="button" class="btn primary" disabled=move || !name_valid() || (current_id.get().is_some() && !editor.dirty())
                            on:click=move |_| editor.save()>"Save layout"</button>
                        <button type="button" class="btn" disabled=move || current_id.get().is_none()
                            on:click=move |_| editor.delete()>"Delete layout"</button>
                        <span role="status">{move || if editor.busy.get() { "Working…" } else if editor.dirty() { "Unsaved changes" } else if current_id.get().is_some() { "Saved" } else { "New draft" }}</span>
                    </div>
                    <Show when=move || !name_valid()><p class="ob-hint">"Use a layout name of 1–120 characters."</p></Show>
                    <Show when=move || current_id.get().is_some()>
                        <div class="ob-toolbar ob-url">
                            <label for="ob-field-obs-url">"OBS URL"</label>
                            <input id="ob-field-obs-url" type="text" readonly prop:value=move || editor.obs_url().unwrap_or_default() />
                            <button type="button" class="btn" on:click=move |_| {
                                let Some(url) = editor.obs_url() else { return; };
                                spawn_local(async move {
                                    let result = net::copy_text(&url).await;
                                    let _ = editor.notice.try_set(if result.is_ok() { "OBS URL copied." } else { "Clipboard unavailable. Select and copy the OBS URL above." });
                                });
                            }>"Copy OBS URL"</button>
                        </div>
                    </Show>
                    <canvas::Canvas editor=editor />
                    <inspector::Inspector editor=editor />
                </fieldset>
            </Show>
        </section>
    }
}
