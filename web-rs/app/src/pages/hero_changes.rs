//! Hero changes: the patch notes "by hero": one hero is one row, unfolding it lists their whole history; "Timeline"
//! is the chronological view (every hero mixed). Filters: hero, type, patch. Port of `web/src/pages/HeroChanges.tsx`.

use std::collections::HashSet;

use leptos::{prelude::*, wasm_bindgen::JsCast, web_sys};
use storm_web_core::{
    aggregates::{form_encode, PatchList},
    net,
    patchnotes::{filter_heroes, ptag_class, HeroChangeSummary, HeroPatchSection, CLASSES, PAGE},
    route::uri_component,
};

use crate::{
    matchui::SearchSelect,
    patchui::PatchEntries,
    ui::{fetch, use_dim, Link, Portrait},
};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

/// Whole history of a hero (mounted when the row is first opened).
#[component]
fn HeroHistory(hero: String) -> impl IntoView {
    let data = fetch::<Vec<HeroPatchSection>>(move || {
        format!("/api/hero/{}/patches", uri_component(&hero))
    });
    move || match data.get() {
        None => view! { <div class="ds-empty-row">"loading…"</div> }.into_any(),
        Some(Ok(l)) if !l.is_empty() => view! {
            <PatchEntries list=l />
        }
        .into_any(),
        Some(_) => view! { <div class="ds-empty-row">"no changes"</div> }.into_any(),
    }
}

/// Query string the way `URLSearchParams` writes it: only the filters that are set.
fn query(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

#[component]
pub fn HeroChanges() -> impl IntoView {
    let dim = use_dim();
    let timeline = RwSignal::new(false);
    let hero = RwSignal::new(String::new());
    let klass = RwSignal::new(String::new());
    let patch = RwSignal::new(String::new());
    let limit = RwSignal::new(PAGE);
    let opened = RwSignal::new(HashSet::<String>::new());
    let patches = fetch::<PatchList>(|| "/api/patches".to_string());
    let hero_options = Signal::derive(move || dim.with(|d| d.names()));

    // one request per view: the other one waits until it is shown
    let heroes = LocalResource::new(move || {
        let on = !timeline.get();
        let url = format!(
            "/api/hero-changes/heroes?{}",
            query(&[("class", klass.get()), ("patch", patch.get())])
        );
        async move {
            if on {
                Some(
                    net::get_json::<Vec<HeroChangeSummary>>(&url)
                        .await
                        .unwrap_or_default(),
                )
            } else {
                None
            }
        }
    });
    let feed = LocalResource::new(move || {
        let on = timeline.get();
        let url = format!(
            "/api/hero-changes?{}",
            query(&[
                ("hero", hero.get()),
                ("class", klass.get()),
                ("patch", patch.get()),
                ("limit", limit.get().to_string()),
            ])
        );
        async move {
            if on {
                Some(
                    net::get_json::<Vec<HeroPatchSection>>(&url)
                        .await
                        .unwrap_or_default(),
                )
            } else {
                None
            }
        }
    });
    let reset = move || limit.set(PAGE);

    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Patch notes · by hero"</div>
                <h1 class="ds-title" style="cursor: default">"Hero changes"</h1>
                <div class="ds-subtitle" style="cursor: default">"Open a hero for their full patch history — “Timeline” lists every change chronologically."</div>
                <div class="ds-filterbar">
                    <span class=move || pill(!timeline.get()) on:click=move |_| timeline.set(false)>"By hero"</span>
                    <span class=move || pill(timeline.get()) on:click=move |_| timeline.set(true)>"Timeline"</span>
                    <span class="ds-sep"></span>
                    <SearchSelect options=hero_options value=hero.into() on_change=move |v| { hero.set(v); reset(); } placeholder="hero…" style="width: 160px" id="hc-heroes" />
                    <span class="ds-sep"></span>
                    <span class=move || pill(klass.with(String::is_empty)) on:click=move |_| { klass.set(String::new()); reset(); }>"All"</span>
                    {CLASSES.iter().map(|c| view! {
                        <span class=move || pill(klass.with(|k| k == c)) on:click=move |_| { klass.set(c.to_string()); reset(); }>{*c}</span>
                    }).collect_view()}
                    <select class="ds-input" on:change=move |e| { patch.set(event_target_value(&e)); reset(); }>
                        <option value="" selected=move || patch.with(String::is_empty)>"All patches"</option>
                        {move || patches.get().and_then(Result::ok).map(|l| l.items).unwrap_or_default().into_iter().map(|p| {
                            let id = p.internal_id.clone();
                            view! { <option value=p.internal_id.clone() selected=move || patch.with(|v| *v == id)>{p.patch_name}</option> }
                        }).collect_view()}
                    </select>
                    <Link to="/patches" class="ds-btn" style="margin-left: auto">"‹ Patch list"</Link>
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || if !timeline.get() {
                    let rows = heroes.get().flatten();
                    let shown: Vec<HeroChangeSummary> = rows.as_deref().map(|r| filter_heroes(r, &hero.get()).into_iter().cloned().collect()).unwrap_or_default();
                    view! {
                        {rows.is_none().then(|| view! { <div class="ds-empty">"loading…"</div> })}
                        {(rows.is_some() && shown.is_empty()).then(|| view! { <div class="ds-empty">"No hero for this filter."</div> })}
                        <div class="ds-hclist">
                            <For each=move || shown.clone() key=|h| h.hero_name.clone() children=move |h| {
                                let name = h.hero_name.clone();
                                let on_toggle = {
                                    let name = name.clone();
                                    move |e: web_sys::Event| {
                                        let is_open = e.current_target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlDetailsElement>().ok())
                                            .is_some_and(|d| d.open());
                                        if is_open {
                                            opened.update(|s| { s.insert(name.clone()); });
                                        }
                                    }
                                };
                                let history = {
                                    let name = name.clone();
                                    move || opened.with(|s| s.contains(&name)).then(|| view! { <HeroHistory hero=name.clone() /> })
                                };
                                let latest = h.latest_class.clone().filter(|c| !c.is_empty());
                                view! {
                                    <details class="ds-hc" on:toggle=on_toggle>
                                        <summary>
                                            <Portrait hero=name.clone() size=34 />
                                            <strong>{name.clone()}</strong>
                                            <span class="ds-tag award">{format!("{} change{}", h.count, if h.count > 1 { "s" } else { "" })}</span>
                                            {latest.map(|c| view! { <span class=format!("ds-ptag {}", ptag_class(Some(&c))) style="padding: 3px 8px">{c.to_uppercase()}</span> })}
                                            <Link to=format!("/hero/{}", uri_component(&name)) class="ds-pill" style="padding: 3px 10px">"hero page ›"</Link>
                                            <span class="last">{format!("last {}", net::fmt_time(h.latest_date.as_deref()))}</span>
                                        </summary>
                                        {history}
                                    </details>
                                }
                            } />
                        </div>
                    }.into_any()
                } else {
                    let rows = feed.get().flatten();
                    let list = rows.clone().unwrap_or_default();
                    let more = rows.is_some() && list.len() >= limit.get();
                    view! {
                        {rows.is_none().then(|| view! { <div class="ds-empty">"loading…"</div> })}
                        {(rows.is_some() && list.is_empty()).then(|| view! { <div class="ds-empty">"No hero change for this filter."</div> })}
                        <PatchEntries list=list with_hero=true />
                        {more.then(|| view! {
                            <div style="text-align: center; margin-top: 14px"><span class="ds-pill" on:click=move |_| limit.update(|l| *l += PAGE)>"Load more"</span></div>
                        })}
                    }.into_any()
                }}
            </section>
        </div>
    }
}
