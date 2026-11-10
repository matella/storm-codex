//! Cast control room, reduced (`/cast`), Leptos 0.8 CSR. Port of the essentials of `web/src/pages/Cast.tsx`:
//! scene tabs (working tab is local, arrows only move the focus, `?scene=`), "Put … on air", ON AIR marker,
//! scaled `<iframe>` preview, one team editor in the rail (Blue) with "Save teams", server errors in plain
//! words, WebSocket-driven refresh. Left out: OBS source block, series settings, pool panel, workspaces,
//! player picker, logo upload.

use leptos::{prelude::*, task::spawn_local};
use spike_common::{control::*, logic::side_label, model::*, net};
use wasm_bindgen::JsCast;

fn main() {
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let series = RwSignal::new(None::<CastSeries>);
    let err = RwSignal::new(None::<String>);
    let refresh = move || {
        spawn_local(async move {
            if let Ok(s) = net::fetch_cast().await {
                series.set(Some(s));
            }
        })
    };
    refresh();
    net::start_live(refresh);
    let loaded = Memo::new(move |_| series.with(|s| s.is_some()));
    view! {
        <Show when=move || loaded.get() fallback=|| view! { <div class="empty">"loading…"</div> }>
            <Room series err refresh />
        </Show>
    }
}

#[component]
fn Room(series: RwSignal<Option<CastSeries>>, err: RwSignal<Option<String>>, refresh: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let get = move || series.get().expect("series is loaded while the room is mounted");
    // working tab: local, `?scene=` at arrival, None = follows the scene on air
    let local = RwSignal::new(scene_from_search(&net::search()));
    let on_air = Memo::new(move |_| get().scene);
    let working = move || local.get().unwrap_or(on_air.get());

    // runs a server action: a returned series feeds the state, any error is shown (never swallowed)
    let run = move |fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<CastSeries, String>>>>, after: Option<Box<dyn FnOnce()>>| {
        err.set(None);
        spawn_local(async move {
            match fut.await {
                Ok(s) => {
                    series.set(Some(s));
                    if let Some(f) = after {
                        f();
                    }
                }
                Err(e) => {
                    err.set(Some(e));
                    refresh();
                }
            }
        });
    };

    let put_on_air = move |_| {
        let Some(next) = local.get_untracked() else { return };
        run(
            Box::pin(async move { net::send_json::<_, CastSeries>("POST", "/api/cast/scene", &serde_json::json!({ "scene": scene_id(next) })).await }),
            Some(Box::new(move || local.set(None))),
        );
    };

    // Blue team editor: the input, the last server version seen, "unsaved" = input differs from the server
    let blue = RwSignal::new(get().blue);
    let base = StoredValue::new(get().blue);
    Effect::new(move |_| {
        let server = get().blue;
        let old = base.get_value();
        base.set_value(server.clone());
        blue.update(|t| *t = resync_team(t, &old, &server));
    });
    let unsaved = move || blue.with(|t| *t != get().blue);
    let save = move |_| {
        let (b, r) = (blue.get_untracked(), get().red);
        run(Box::pin(async move { net::send_json::<_, CastSeries>("PUT", "/api/cast/teams", &serde_json::json!({ "blue": b, "red": r })).await }), None);
    };

    let scale = RwSignal::new(None::<f64>);
    Effect::new(move |_| net::observe_width("cr-prev-box", move |w| scale.set(Some(w / 1920.0))));

    view! {
        <div class="cr">
            <div class="cr-top">
                <span class="cr-br">"CAST "<b>"CONTROL ROOM"</b></span>
                <SceneTabs on_air local />
                {move || local.get().filter(|l| *l != on_air.get()).map(|l| view! {
                    <button type="button" class="ds-btn primary small" on:click=put_on_air>{format!("Put {} on air", scene_label(l))}</button>
                })}
                {move || (!get().active).then(|| view! { <span class="cr-inactive">"no active series"</span> })}
            </div>
            {move || err.get().map(|e| {
                let friendly = friendly_error(&e).map(str::to_string);
                let raw = e.clone();
                view! {
                    <div class="cr-err" role="alert">
                        {friendly.clone().unwrap_or(raw.clone())}
                        {friendly.map(|_| view! { <span class="cr-note">{format!(" ({raw})")}</span> })}
                        <button type="button" class="ds-btn small" on:click=move |_| err.set(None)>"dismiss"</button>
                    </div>
                }
            })}
            <div class="cr-body">
                <aside class="cr-rail">
                    <div class="cr-k">"Teams & lineups"</div>
                    <TeamEditor side=Side::Blue team=blue />
                    <button type="button" class="ds-btn cr-wide" disabled=move || !unsaved() on:click=save>
                        {move || if unsaved() { "Save teams" } else { "Teams saved" }}
                    </button>
                </aside>
                <div class="cr-main">
                    <div class="cr-prev">
                        <div class="cr-prev-k">{move || format!("Live preview · {} on air", scene_label(on_air.get()))}</div>
                        <div class="cr-prev-box" id="cr-prev-box" style=move || preview_styles(scale.get()).0>
                            <iframe title="Cast overlay preview" src="/cast/overlay" width="1920" height="1080" style=move || preview_styles(scale.get()).1></iframe>
                        </div>
                    </div>
                    <div role="tabpanel" id="cr-workspace" aria-labelledby=move || format!("cr-tab-{}", scene_id(working()))>
                        <section class="cr-box"><div class="cr-note">{move || format!("{} workspace: not part of the spike", scene_label(working()))}</div></section>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Scene bar (tabs pattern, manual activation): `aria-selected` = local working tab, the scene on air is said in
/// text ("ON AIR"). Click / Enter / Space select the working tab only; arrows, Home, End only move the focus.
#[component]
fn SceneTabs(on_air: Memo<Scene>, local: RwSignal<Option<Scene>>) -> impl IntoView {
    let working = move || local.get().unwrap_or(on_air.get());
    view! {
        <div class="cr-scenes" role="tablist" aria-label="Scene (selects the working tab; the button puts it on air)">
            {SCENES.iter().enumerate().map(|(i, (scene, label))| {
                let scene = *scene;
                let id = scene_id(scene);
                view! {
                    <button type="button" role="tab" id=format!("cr-tab-{id}") aria-controls="cr-workspace"
                        aria-selected=move || (working() == scene).to_string()
                        tabindex=move || if working() == scene { "0" } else { "-1" }
                        class=move || format!("cr-scene{}{}", if on_air.get() == scene { " on" } else { "" }, if working() == scene && working() != on_air.get() { " ed" } else { "" })
                        on:keydown=move |e| {
                            if let Some(t) = tab_key_target(i, &e.key(), SCENES.len()) {
                                e.prevent_default();
                                let target = format!("cr-tab-{}", scene_id(SCENES[t].0));
                                if let Some(el) = document().get_element_by_id(&target).and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
                                    let _ = el.focus();
                                }
                            }
                        }
                        on:click=move |_| local.set(if scene == on_air.get_untracked() { None } else { Some(scene) })>
                        {*label}
                        {move || (on_air.get() == scene).then(|| view! { <i class="cr-air">"ON AIR"</i> })}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
fn TeamEditor(side: Side, team: RwSignal<Team>) -> impl IntoView {
    let tone = if side == Side::Blue { "blue" } else { "red" };
    let legend = side_label(side);
    let set_player = move |i: usize, f: Box<dyn FnOnce(&mut CastPlayer)>| team.update(|t| if let Some(p) = t.players.get_mut(i) { f(p) });
    view! {
        <fieldset class=format!("cr-team tone-{tone}")>
            <legend>{legend}</legend>
            <input class="ds-input" placeholder=format!("Team {legend}") prop:value=move || team.with(|t| t.name.clone())
                aria-label=format!("{tone} team name") on:input=move |e| team.update(|t| t.name = event_target_value(&e)) />
            <input class="ds-input" placeholder="logo URL (optional)" prop:value=move || team.with(|t| t.logo.clone().unwrap_or_default())
                aria-label=format!("{tone} logo URL")
                on:input=move |e| { let v = event_target_value(&e); team.update(|t| t.logo = if v.trim().is_empty() { None } else { Some(v) }) } />
            <input class="ds-input" placeholder="record (e.g. 4-1)" prop:value=move || team.with(|t| t.record.clone())
                aria-label=format!("{tone} record") on:input=move |e| team.update(|t| t.record = event_target_value(&e)) />
            {(0..5).map(|i| view! {
                <div class="cr-player">
                    <input class="ds-input" placeholder="player name" aria-label=format!("{tone} player {} name", i + 1)
                        prop:value=move || team.with(|t| t.players.get(i).map(|p| p.name.clone()).unwrap_or_default())
                        on:input=move |e| { let v = event_target_value(&e); set_player(i, Box::new(move |p| p.name = v)) } />
                    <div class="cr-pl2">
                        <input class="ds-input" placeholder="tagline" aria-label=format!("{tone} player {} tagline", i + 1)
                            prop:value=move || team.with(|t| t.players.get(i).map(|p| p.tagline.clone()).unwrap_or_default())
                            on:input=move |e| { let v = event_target_value(&e); set_player(i, Box::new(move |p| p.tagline = v)) } />
                        <button type="button" class=move || format!("ds-btn small{}", if team.with(|t| t.captain == Some(i)) { " on" } else { "" })
                            title="Captain" aria-pressed=move || team.with(|t| t.captain == Some(i)).to_string()
                            on:click=move |_| team.update(|t| t.captain = if t.captain == Some(i) { None } else { Some(i) })>"C"</button>
                    </div>
                </div>
            }).collect_view()}
        </fieldset>
    }
}
