//! Cast control room, reduced (`/cast`), Dioxus 0.7 web (CSR). Same scope as the Leptos spike: scene tabs
//! (working tab local, arrows only move the focus, `?scene=`), "Put … on air", ON AIR marker, scaled
//! `<iframe>` preview, one team editor in the rail (Blue) with "Save teams", server errors in plain words,
//! WebSocket-driven refresh. Left out: OBS source block, series settings, pool panel, workspaces,
//! player picker, logo upload.

use dioxus::prelude::*;
use spike_common::{control::*, logic::side_label, model::*, net};
use wasm_bindgen::JsCast;

const THEME: Asset = asset!("/assets/theme.css");
const DS: Asset = asset!("/assets/ds.css");
const CAST: Asset = asset!("/assets/cast.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let series = use_signal(|| None::<CastSeries>);
    use_hook(move || {
        let refresh = move || {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(s) = net::fetch_cast().await {
                    let mut series = series;
                    series.set(Some(s));
                }
            })
        };
        refresh();
        net::start_live(refresh);
    });
    rsx! {
        document::Link { rel: "stylesheet", href: THEME }
        document::Link { rel: "stylesheet", href: DS }
        document::Link { rel: "stylesheet", href: CAST }
        if series.read().is_some() {
            Room { series }
        } else {
            div { class: "empty", "loading…" }
        }
    }
}

#[component]
fn Room(series: Signal<Option<CastSeries>>) -> Element {
    let s = series.read().clone().expect("series is loaded while the room is mounted");
    let mut err = use_signal(|| None::<String>);
    // working tab: local, `?scene=` at arrival, None = follows the scene on air
    let mut local = use_signal(|| scene_from_search(&net::search()));
    let on_air = s.scene;
    let working = local().unwrap_or(on_air);

    // Blue team editor: the input, the last server version seen, "unsaved" = input differs from the server
    let mut blue = use_signal(|| s.blue.clone());
    let mut base = use_signal(|| s.blue.clone());
    use_effect(move || {
        let server = series.read().as_ref().map(|s| s.blue.clone());
        if let Some(server) = server {
            let old = base.peek().clone();
            base.set(server.clone());
            let cur = blue.peek().clone();
            blue.set(resync_team(&cur, &old, &server));
        }
    });
    let unsaved = *blue.read() != s.blue;

    let scale = use_signal(|| None::<f64>);
    use_hook(move || {
        // after the first render the preview box exists: follow its width
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            net::observe_width("cr-prev-box", move |w| {
                let mut scale = scale;
                scale.set(Some(w / 1920.0));
            });
        });
    });

    // runs a server action: a returned series feeds the state, any error is shown (never swallowed)
    let mut run = move |fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<CastSeries, String>>>>, on_ok: bool| {
        err.set(None);
        spawn(async move {
            match fut.await {
                Ok(s) => {
                    series.set(Some(s));
                    if on_ok {
                        local.set(None);
                    }
                }
                Err(e) => {
                    err.set(Some(e));
                    if let Ok(s) = net::fetch_cast().await {
                        series.set(Some(s));
                    }
                }
            }
        });
    };

    let put_on_air = move |_| {
        let Some(next) = local() else { return };
        run(Box::pin(async move { net::send_json::<_, CastSeries>("POST", "/api/cast/scene", &serde_json::json!({ "scene": scene_id(next) })).await }), true);
    };
    let red = s.red.clone();
    let save = move |_| {
        let (b, r) = (blue.peek().clone(), red.clone());
        run(Box::pin(async move { net::send_json::<_, CastSeries>("PUT", "/api/cast/teams", &serde_json::json!({ "blue": b, "red": r })).await }), false);
    };
    // Dioxus applies `style` as individual properties and never resets one that disappears from the string,
    // so the properties dropped by the measured scale are reset explicitly.
    let (box_style, frame_style) = preview_styles(scale());
    let (box_style, frame_style) = if scale().is_some() {
        (format!("aspect-ratio:auto;{box_style}"), format!("visibility:visible;{frame_style}"))
    } else {
        (box_style, frame_style)
    };

    rsx! {
        div { class: "cr",
            div { class: "cr-top",
                span { class: "cr-br", "CAST ", b { "CONTROL ROOM" } }
                SceneTabs { on_air, working, local }
                if let Some(l) = local().filter(|l| *l != on_air) {
                    button { r#type: "button", class: "ds-btn primary small", onclick: put_on_air, "Put {scene_label(l)} on air" }
                }
                if !s.active { span { class: "cr-inactive", "no active series" } }
            }
            if let Some(e) = err() {
                {
                    let friendly = friendly_error(&e);
                    rsx! {
                        div { class: "cr-err", role: "alert",
                            "{friendly.unwrap_or(e.as_str())}"
                            if friendly.is_some() { span { class: "cr-note", " ({e})" } }
                            button { r#type: "button", class: "ds-btn small", onclick: move |_| err.set(None), "dismiss" }
                        }
                    }
                }
            }
            div { class: "cr-body",
                aside { class: "cr-rail",
                    div { class: "cr-k", "Teams & lineups" }
                    TeamEditor { side: Side::Blue, team: blue }
                    button { r#type: "button", class: "ds-btn cr-wide", disabled: !unsaved, onclick: save,
                        if unsaved { "Save teams" } else { "Teams saved" }
                    }
                }
                div { class: "cr-main",
                    div { class: "cr-prev",
                        div { class: "cr-prev-k", "Live preview · {scene_label(on_air)} on air" }
                        div { class: "cr-prev-box", id: "cr-prev-box", style: box_style,
                            iframe { title: "Cast overlay preview", src: "/cast/overlay", width: "1920", height: "1080", style: frame_style }
                        }
                    }
                    div { role: "tabpanel", id: "cr-workspace", "aria-labelledby": "cr-tab-{scene_id(working)}",
                        section { class: "cr-box", div { class: "cr-note", "{scene_label(working)} workspace: not part of the spike" } }
                    }
                }
            }
        }
    }
}

/// Scene bar (tabs pattern, manual activation): `aria-selected` = local working tab, the scene on air is said in
/// text ("ON AIR"). Click / Enter / Space select the working tab only; arrows, Home, End only move the focus.
#[component]
fn SceneTabs(on_air: Scene, working: Scene, local: Signal<Option<Scene>>) -> Element {
    rsx! {
        div { class: "cr-scenes", role: "tablist", "aria-label": "Scene (selects the working tab; the button puts it on air)",
            for (i, (scene, label)) in SCENES.iter().enumerate() {
                {
                    let scene = *scene;
                    let id = scene_id(scene);
                    let class = format!("cr-scene{}{}", if on_air == scene { " on" } else { "" }, if working == scene && working != on_air { " ed" } else { "" });
                    rsx! {
                        button { key: "{id}", r#type: "button", role: "tab", id: "cr-tab-{id}", "aria-controls": "cr-workspace",
                            "aria-selected": (working == scene).to_string(), tabindex: if working == scene { "0" } else { "-1" }, class,
                            onkeydown: move |e| {
                                if let Some(t) = tab_key_target(i, &e.key().to_string(), SCENES.len()) {
                                    e.prevent_default();
                                    let target = format!("cr-tab-{}", scene_id(SCENES[t].0));
                                    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(&target)).and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        let _ = el.focus();
                                    }
                                }
                            },
                            onclick: move |_| local.set(if scene == on_air { None } else { Some(scene) }),
                            "{label}"
                            if on_air == scene { i { class: "cr-air", "ON AIR" } }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TeamEditor(side: Side, team: Signal<Team>) -> Element {
    let tone = if side == Side::Blue { "blue" } else { "red" };
    let legend = side_label(side);
    let t = team.read().clone();
    rsx! {
        fieldset { class: "cr-team tone-{tone}",
            legend { "{legend}" }
            input { class: "ds-input", placeholder: "Team {legend}", value: "{t.name}", "aria-label": "{tone} team name",
                oninput: move |e| team.write().name = e.value() }
            input { class: "ds-input", placeholder: "logo URL (optional)", value: "{t.logo.clone().unwrap_or_default()}", "aria-label": "{tone} logo URL",
                oninput: move |e| { let v = e.value(); team.write().logo = if v.trim().is_empty() { None } else { Some(v) } } }
            input { class: "ds-input", placeholder: "record (e.g. 4-1)", value: "{t.record}", "aria-label": "{tone} record",
                oninput: move |e| team.write().record = e.value() }
            for i in 0..5usize {
                {
                    let p = t.players.get(i).cloned().unwrap_or_default();
                    let captain = t.captain == Some(i);
                    rsx! {
                        div { key: "{i}", class: "cr-player",
                            input { class: "ds-input", placeholder: "player name", "aria-label": "{tone} player {i + 1} name", value: "{p.name}",
                                oninput: move |e| if let Some(p) = team.write().players.get_mut(i) { p.name = e.value() } }
                            div { class: "cr-pl2",
                                input { class: "ds-input", placeholder: "tagline", "aria-label": "{tone} player {i + 1} tagline", value: "{p.tagline}",
                                    oninput: move |e| if let Some(p) = team.write().players.get_mut(i) { p.tagline = e.value() } }
                                button { r#type: "button", class: if captain { "ds-btn small on" } else { "ds-btn small" }, title: "Captain", "aria-pressed": captain.to_string(),
                                    onclick: move |_| { let mut t = team.write(); t.captain = if t.captain == Some(i) { None } else { Some(i) }; }, "C" }
                            }
                        }
                    }
                }
            }
        }
    }
}
