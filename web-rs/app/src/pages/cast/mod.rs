//! Cast control room (`/cast`): series rail on the left, scene bar, OBS preview, workspace. Port of `Cast.tsx`
//! and `web/src/cast/*`; spec: docs/specs/2026-10-25-cast-control-room-design.md. Server errors (409:
//! `NotInPool`, `SeriesInactive`, `MapAlreadyPlayed`...) are shown to the operator as they come.

mod api;
mod draft;
mod pool;
mod rail;
mod workspace;

use leptos::{ev, prelude::*, wasm_bindgen::JsCast, web_sys};
use storm_web_core::{
    castctl::{
        friendly_error, obs_overlay_url, scene_from_search, scene_id, scene_label, tab_key_target,
        OBS_SKINS, SCENES,
    },
    logic::{badge_game_number, series_score},
    model::{CastSeries, Scene},
    net,
    scenes::team_initials,
};

use crate::{
    layout::LiveBus,
    ui::{fetch, use_dim},
};
use api::Runner;

/// Hero portrait of the console (`HeroPortrait`): initials when the reference data does not know the hero.
#[component]
pub fn HeroPortrait(
    #[prop(into)] hero: String,
    #[prop(optional, into)] title: Option<String>,
) -> impl IntoView {
    let dim = use_dim();
    let h = hero.clone();
    let icon = move || dim.with(|d| d.icon(&h).map(str::to_string));
    let title = title.unwrap_or_else(|| hero.clone());
    let ini = team_initials(&hero);
    move || match icon() {
        Some(src) => {
            view! { <img class="hp-i " src=src alt=hero.clone() title=title.clone() /> }.into_any()
        }
        None => view! { <i class="hp-i ph " title=title.clone()>{ini.clone()}</i> }.into_any(),
    }
}

/// Scene bar (tab pattern, manual activation): `aria-selected` = the local working tab; the scene on air is said in
/// text ("ON AIR"). Click / Enter / Space only select the working tab; going on air is the "Put … on air" button.
/// Arrows, Home and End only move the focus.
#[component]
fn SceneTabs(on_air: Memo<Scene>, local: Memo<Scene>, on_pick: Callback<Scene>) -> impl IntoView {
    view! {
        <div class="cr-scenes" role="tablist" aria-label="Scene (selects the working tab; the button puts it on air)">
            {SCENES.iter().enumerate().map(|(i, (sc, id, label))| {
                let (sc, id, label) = (*sc, *id, *label);
                let class = move || format!(
                    "cr-scene{}{}",
                    if on_air.get() == sc { " on" } else { "" },
                    if local.get() == sc && local.get() != on_air.get() { " ed" } else { "" },
                );
                let onkey = move |e: ev::KeyboardEvent| {
                    let Some(t) = tab_key_target(i, &e.key(), SCENES.len()) else { return };
                    e.prevent_default();
                    if let Some(el) = document()
                        .get_element_by_id(&format!("cr-tab-{}", SCENES[t].1))
                        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
                    {
                        let _ = el.focus();
                    }
                };
                view! {
                    <button type="button" role="tab" id=format!("cr-tab-{id}") aria-controls="cr-workspace"
                        aria-selected=move || (local.get() == sc).to_string()
                        tabindex=move || if local.get() == sc { "0" } else { "-1" }
                        class=class on:keydown=onkey on:click=move |_| on_pick.run(sc)>
                        {label}{move || (on_air.get() == sc).then(|| view! { <i class="cr-air">"ON AIR"</i> })}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

/// "OBS source" block: URL of the cast overlay, always visible, with copy and the skin choice (the skin preference
/// stays local to the browser).
#[component]
fn ObsSource() -> impl IntoView {
    let skin = RwSignal::new(
        net::storage_get("cast.obsSkin")
            .filter(|v| OBS_SKINS.contains(&v.as_str()))
            .unwrap_or_else(|| "nexus".to_string()),
    );
    let copy = RwSignal::new("idle");
    let origin = net::origin();
    let url = move || obs_overlay_url(&origin, &skin.get());
    let origin2 = net::origin();
    let do_copy = move |_| {
        let u = obs_overlay_url(&origin2, &skin.get_untracked());
        leptos::task::spawn_local(async move {
            copy.set(if net::copy_text(&u).await.is_ok() {
                "copied"
            } else {
                "failed"
            });
            net::after_ms(1800, move || copy.set("idle"));
        });
    };
    view! {
        <section class="cr-obs" aria-label="OBS source">
            <div class="cr-obs-row">
                <span class="cr-k">"OBS source"</span>
                <code class="cr-obs-url" title=url.clone()>{url.clone()}</code>
                <button type="button" class="ds-btn small" on:click=do_copy>
                    {move || match copy.get() { "copied" => "Copied", "failed" => "Copy failed — select the URL", _ => "Copy URL" }}
                </button>
                <label class="cr-note">"Skin"{" "}
                    <select class="ds-input" aria-label="Overlay skin"
                        on:change=move |e| { let v = event_target_value(&e); net::storage_set("cast.obsSkin", &v); skin.set(v); }>
                        {OBS_SKINS.iter().map(|s| { let s = *s; view! { <option value=s selected=move || skin.get() == s>{s}</option> } }).collect_view()}
                    </select>
                </label>
            </div>
            <div class="cr-note">"Add it as a Browser source · 1920×1080 · transparent background. The scene shown is chosen in this console."</div>
        </section>
    }
}

const W: f64 = 1920.0;
const H: f64 = 1080.0;

/// Preview of the OBS output: the real overlay (`/cast/overlay`, 1920×1080 logical) in an iframe scaled to the width of
/// the pane. Until the width is measured the frame keeps the 16:9 ratio and the iframe stays hidden.
#[component]
fn ScenePreview(on_air: Memo<Scene>) -> impl IntoView {
    let scale = RwSignal::new(None::<f64>);
    net::after_ms(0, move || {
        net::observe_width("cr-prev-box", move |w| scale.set(Some(w / W)))
    });
    view! {
        <div class="cr-prev">
            <div class="cr-prev-k">{move || format!("Live preview · {} on air", scene_label(on_air.get()))}</div>
            <div class="cr-prev-box" id="cr-prev-box"
                style=move || match scale.get() { None => format!("aspect-ratio: {W} / {H}"), Some(s) => format!("height: {}px", H * s) }>
                <iframe title="Cast overlay preview" src="/cast/overlay" width=W.to_string() height=H.to_string()
                    style=move || match scale.get() { None => "visibility: hidden".to_string(), Some(s) => format!("transform: scale({s})") }></iframe>
            </div>
        </div>
    }
}

/// In-game scene workspace: score steppers and the on-screen badge.
#[component]
fn InGame(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let set =
        move |nb: i64, nr: i64| run.run(api::score(Some((nb.max(0) as u32, nr.max(0) as u32))));
    let score = move || series_score(&s.get());
    view! {
        <section class="cr-box">
            <div class="cr-k">"In-game · score"</div>
            <div class="cr-steppers">
                {[("blue", true), ("red", false)].into_iter().map(|(side, is_blue)| {
                    let v = move || { let (b, r) = score(); if is_blue { b } else { r } };
                    let step = move |d: i64| {
                        let (b, r) = score();
                        let (b, r) = (i64::from(b), i64::from(r));
                        if is_blue { set(b + d, r) } else { set(b, r + d) }
                    };
                    view! {
                        <div class=format!("cr-stepper tone-{side}")>
                            <button type="button" class="ds-btn small" aria-label=format!("{side} score minus") on:click=move |_| step(-1)>"−"</button>
                            <b>{v}</b>
                            <button type="button" class="ds-btn small" aria-label=format!("{side} score plus") on:click=move |_| step(1)>"+"</button>
                        </div>
                    }
                }).collect_view()}
                {move || s.get().score_override.is_some().then(|| view! {
                    <button type="button" class="ds-btn small" on:click=move |_| run.run(api::score(None))>"Back to computed score"</button>
                })}
            </div>
            <div class="cr-note">
                {move || {
                    let x = s.get();
                    format!("On-screen badge: BO {} · GAME {} · {}{}", x.bo, badge_game_number(&x), x.format.to_uppercase(),
                        if x.score_override.is_some() { " · score set by you" } else { " · score from game winners" })
                }}
            </div>
        </section>
    }
}

#[component]
pub fn Cast() -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let data = fetch::<CastSeries>(move || {
        bus.cast.get();
        "/api/cast".to_string()
    });
    // last good state: a failed refresh keeps what is on screen (react-query keeps the previous data)
    let latest = RwSignal::new(None::<CastSeries>);
    Effect::new(move |_| {
        if let Some(Ok(s)) = data.get() {
            if latest.with_untracked(|l| l.as_ref() != Some(&s)) {
                latest.set(Some(s));
            }
        }
    });
    let ready = Memo::new(move |_| latest.with(Option::is_some));
    let s = Memo::new(move |_| latest.get().unwrap_or_default());
    let on_air = Memo::new(move |_| s.get().scene);

    let err = RwSignal::new(None::<String>);
    let run = Runner {
        err,
        refresh: bus.cast,
    };
    // local working tab: `?scene=` on arrival (redirect from /draft), without touching the scene on air;
    // None = follows the scene on air
    let local = RwSignal::new(scene_from_search(&net::search()));
    let active = Memo::new(move |_| local.get().unwrap_or_else(|| on_air.get()));

    move || {
        if !ready.get() {
            return match data.get() {
                Some(Err(e)) => {
                    view! { <div class="empty">{format!("cast state unavailable: {e}")}</div> }
                        .into_any()
                }
                _ => view! { <div class="empty">"loading…"</div> }.into_any(),
            };
        }
        view! {
            <div class="cr">
                <div class="cr-top">
                    <span class="cr-br">"CAST "<b>"CONTROL ROOM"</b></span>
                    <SceneTabs on_air=on_air local=active
                        on_pick=Callback::new(move |sc| local.set(if sc == on_air.get_untracked() { None } else { Some(sc) })) />
                    {move || local.get().filter(|l| *l != on_air.get()).map(|next| view! {
                        <button type="button" class="ds-btn primary small"
                            on:click=move |_| run.run(async move { let r = api::scene(next).await?; local.set(None); Ok(r) })>
                            {format!("Put {} on air", scene_label(next))}
                        </button>
                    })}
                    {move || (!s.get().active).then(|| view! { <span class="cr-inactive">"no active series"</span> })}
                </div>
                <ObsSource />
                {move || err.get().map(|e| {
                    let friendly = friendly_error(&e);
                    let differs = friendly != e;
                    view! {
                        <div class="cr-err" role="alert">
                            {friendly}{differs.then(|| view! { <span class="cr-note">{format!(" ({e})")}</span> })}
                            <button type="button" class="ds-btn small" on:click=move |_| err.set(None)>"dismiss"</button>
                        </div>
                    }
                })}
                <div class="cr-body">
                    <rail::SeriesRail s=s run=run />
                    <div class="cr-main">
                        <ScenePreview on_air=on_air />
                        <div role="tabpanel" id="cr-workspace" aria-labelledby=move || format!("cr-tab-{}", scene_id(active.get()))>
                            {move || match active.get() {
                                Scene::Hide => ().into_any(),
                                Scene::Series => view! { <workspace::SeriesWorkspace s=s run=run /> }.into_any(),
                                Scene::Lineups => view! { <workspace::LineupsWorkspace s=s run=run /> }.into_any(),
                                Scene::Draft => view! { <draft::DraftWorkspace s=s /> }.into_any(),
                                Scene::Ingame => view! { <InGame s=s run=run /> }.into_any(),
                            }}
                        </div>
                        <pool::PoolPanel s=s run=run />
                    </div>
                </div>
            </div>
        }
        .into_any()
    }
}
