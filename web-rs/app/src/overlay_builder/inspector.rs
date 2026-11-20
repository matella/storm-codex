use super::{
    canvas::{kind, label},
    Editor,
};
use leptos::{prelude::*, web_sys};
use storm_web_app::overlay_builder_model::Rect;
use storm_web_core::overlay_layout::{
    NowPlayingOptions, PlayerName, WidgetPlacement, WidgetSettings, GRID_COLS, GRID_ROWS,
};

#[component]
pub(super) fn Inspector(editor: Editor) -> impl IntoView {
    let items = Memo::new(move |_| {
        editor.draft.with(|draft| {
            draft
                .widgets
                .iter()
                .map(|w| (w.id.clone(), label(kind(&w.placement.settings))))
                .collect::<Vec<_>>()
        })
    });
    view! {
        <section class="ob-inspector" aria-label="Widget settings">
            <label for="ob-field-selected-widget">"Selected widget"</label>
            <select id="ob-field-selected-widget" on:change=move |event| {
                let id = event_target_value(&event);
                editor.selected.set((!id.is_empty()).then_some(id));
            }>
                <option value="" selected=move || editor.selected.with(Option::is_none)>"Select a widget"</option>
                {move || items.get().into_iter().enumerate().rev().map(|(index, (id, label))| view! {
                    <option value=id.clone() selected=move || editor.selected.with(|s| s.as_ref() == Some(&id))>
                        {format!("Layer {} — {label}", index + 1)}
                    </option>
                }).collect_view()}
            </select>
            {move || editor.selected.get().map(|id| view! { <Options editor=editor id=id /> })}
        </section>
    }
}

#[component]
fn Options(editor: Editor, id: String) -> impl IntoView {
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
    let update = move |edit: &dyn Fn(&mut WidgetPlacement)| {
        editor.draft.update(|draft| {
            id.with_value(|id| {
                if let Some(w) = draft.widgets.iter_mut().find(|w| &w.id == id) {
                    edit(&mut w.placement);
                }
            })
        })
    };
    let widget = placement.with_untracked(|p| p.as_ref().map(|p| kind(&p.settings)));
    let edit_name = move |event| {
        let input = event_target::<web_sys::HtmlInputElement>(&event);
        let text = input.value();
        if let Ok(name) = PlayerName::try_from(text.clone()) {
            update(&|p| {
                if let WidgetSettings::Widget(options) = &mut p.settings {
                    options.me = (!text.is_empty()).then(|| name.clone());
                }
            });
        } else {
            input.set_value(&player_name(placement));
        }
    };
    view! {
        <div class="ob-fields">
            {[("X (cells)", "ob-field-x", false), ("Y (cells)", "ob-field-y", false), ("Width (cells)", "ob-field-w", true), ("Height (cells)", "ob-field-h", true)]
                .into_iter().enumerate().map(|(index, (label, field_id, resize))| {
                    let value = move || placement.with(|p| p.as_ref().map(field_values).unwrap_or_default()[index]);
                    let horizontal = index % 2 == 0;
                    view! {
                        <label for=field_id>{label}</label>
                        <input id=field_id type="number" step="1" min=if resize { 1 } else { 0 }
                            max=move || placement.with(|p| p.as_ref().map(|p| match index {
                                0 => GRID_COLS - p.grid_w, 1 => GRID_ROWS - p.grid_h,
                                2 => GRID_COLS - p.grid_x, _ => GRID_ROWS - p.grid_y,
                            }).unwrap_or_default())
                            prop:value=move || value().to_string()
                            on:change=move |event| {
                                let input = event_target::<web_sys::HtmlInputElement>(&event);
                                let proposed = input.value_as_number();
                                if proposed.is_finite() {
                                    update(&|p| {
                                        let delta = (proposed.round() as i32).saturating_sub(field_values(p)[index]);
                                        Rect::from(&*p).drag(if horizontal { delta } else { 0 }, if horizontal { 0 } else { delta }, resize).apply(p);
                                    });
                                }
                                input.set_value(&value().to_string());
                            } />
                    }
                }).collect_view()}
            {(widget == Some(storm_web_core::overlay_layout::WidgetType::Widget)).then(|| view! {
                <label for="ob-field-me">"Player name (me)"</label>
                <input id="ob-field-me" type="text" maxlength="120" placeholder="Configured operator"
                    prop:value=move || player_name(placement) on:input=edit_name />
            })}
            {(widget == Some(storm_web_core::overlay_layout::WidgetType::NowPlaying)).then(|| view! {
                <label for="ob-field-music-mode">"Music mode"</label>
                <select id="ob-field-music-mode" prop:value=move || placement.with(|p| match &p.as_ref().map(|p| &p.settings) {
                    Some(WidgetSettings::NowPlaying(options)) if options.reveal == Some(true) => "reveal",
                    Some(WidgetSettings::NowPlaying(options)) if options.mini == Some(true) => "mini",
                    _ => "full",
                }) on:change=move |event| {
                    let mode = event_target_value(&event);
                    update(&|p| p.settings = WidgetSettings::NowPlaying(NowPlayingOptions {
                        mini: (mode == "mini").then_some(true), reveal: (mode == "reveal").then_some(true),
                    }));
                }>
                    <option value="full">"Full"</option>
                    <option value="mini">"Mini"</option>
                    <option value="reveal">"Reveal"</option>
                </select>
            })}
        </div>
        <div class="ob-toolbar">
            <button type="button" class="btn" disabled=move || placement.with(|p| p.as_ref().is_none_or(|p| p.z_order == 0))
                on:click=move |_| editor.draft.update(|draft| id.with_value(|id| draft.layer(id, -1)))>"Move layer back"</button>
            <button type="button" class="btn" disabled=move || placement.with(|p| p.as_ref().is_none_or(|p| editor.draft.with(|draft| p.z_order as usize + 1 >= draft.widgets.len())))
                on:click=move |_| editor.draft.update(|draft| id.with_value(|id| draft.layer(id, 1)))>"Move layer forward"</button>
            <button type="button" class="btn" on:click=move |_| {
                editor.selected.set(None);
                editor.draft.update(|draft| {
                    id.with_value(|id| draft.widgets.retain(|widget| &widget.id != id));
                    draft.normalize_layers();
                });
            }>"Remove widget"</button>
        </div>
    }
}

fn field_values(p: &WidgetPlacement) -> [i32; 4] {
    [p.grid_x, p.grid_y, p.grid_w, p.grid_h]
}

fn player_name(placement: Memo<Option<WidgetPlacement>>) -> String {
    placement.with(|p| match p.as_ref().map(|p| &p.settings) {
        Some(WidgetSettings::Widget(options)) => {
            options.me.clone().map(String::from).unwrap_or_default()
        }
        _ => String::new(),
    })
}
