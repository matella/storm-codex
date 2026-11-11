//! Hero: cover with the large portrait (universe colour), maps, winning builds, patch history.
//! Port of `web/src/pages/Hero.tsx`.

use std::collections::HashMap;

use leptos::prelude::*;
use leptos_router::{
    hooks::{use_location, use_navigate, use_params_map},
    NavigateOptions,
};
use storm_web_core::{
    aggregates::{agg_to_search, bar_class, has_map_art, search_to_agg, tone, AggFilter},
    herodetail::{best_map, build_picks, decamelize, maps_by_games, DimTalent, HeroDetail},
    net,
    patchnotes::HeroPatchSection,
    players::{kda_ratio, num_or_dash, pct},
    route::{app_path, uri_component},
};

use crate::{
    patchui::PatchEntries,
    ui::{fetch, map_art, use_dim, use_talents, AggFilterBar, Link, Portrait, SecHead},
};

/// Talent build (`TierNChoice` → tree id) as steps in level order. Names come from the reference data once loaded.
#[component]
fn BuildSteps(
    talents: std::collections::BTreeMap<String, String>,
    dim: RwSignal<HashMap<String, DimTalent>>,
) -> impl IntoView {
    view! {
        <div class="ds-bsteps">
            {build_picks(&talents).into_iter().map(|tid| {
                let lookup = {
                    let tid = tid.clone();
                    move || dim.with(|m| m.get(&tid).cloned())
                };
                let lookup2 = lookup.clone();
                let fallback = decamelize(&tid);
                view! {
                    <div class="ds-bstep" title=tid>
                        <span>"LV "{move || lookup().map_or("?".to_string(), |i| i.tier.to_string())}</span>
                        <strong>{move || lookup2().map_or(fallback.clone(), |i| i.name)}</strong>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

/// Hero to patch: the adjustments of this hero across the patch notes, newest first.
#[component]
fn HeroPatches(hero: Signal<String>, num: Signal<u32>) -> impl IntoView {
    let data = fetch::<Vec<HeroPatchSection>>(move || {
        format!("/api/hero/{}/patches", uri_component(&hero.get()))
    });
    move || {
        let list = data.get()?.ok().filter(|l| !l.is_empty())?;
        let sub = format!(
            "{} change{} · from HotsPatchNotes",
            list.len(),
            if list.len() > 1 { "s" } else { "" }
        );
        Some(view! {
            <section class="ds-sec">
                <SecHead num=num.get() title="Patch history" sub=sub />
                <PatchEntries list=list />
            </section>
        })
    }
}

#[component]
pub fn Hero() -> impl IntoView {
    let params = use_params_map();
    let name = Signal::derive(move || params.read().get("name").unwrap_or_default());
    let loc = use_location();
    let nav = StoredValue::new_local(use_navigate());
    let dim = use_dim();
    let talents = use_talents();

    // the filter lives in the URL (carried from the lists), both ways
    let filter = RwSignal::new(search_to_agg(&loc.search.get_untracked()));
    Effect::new(move |_| {
        let f = search_to_agg(&loc.search.get());
        if f != filter.get_untracked() {
            filter.set(f);
        }
    });
    Effect::new(move |_| {
        let f: AggFilter = filter.get();
        if f == search_to_agg(&loc.search.get_untracked()) {
            return;
        }
        let path = app_path(&loc.pathname.get_untracked()).to_string();
        let q = agg_to_search(&f);
        let to = if q.is_empty() {
            path
        } else {
            format!("{path}?{q}")
        };
        nav.with_value(|nav| {
            nav(
                &to,
                NavigateOptions {
                    replace: true,
                    ..Default::default()
                },
            )
        });
    });

    let data = fetch::<HeroDetail>(move || {
        format!(
            "/api/hero/{}?{}",
            uri_component(&name.get()),
            net::agg_params(&filter.get())
        )
    });
    let loaded = move || data.get().and_then(Result::ok);
    let ring = move || dim.with(|d| d.ring(Some(&name.get())));
    let kicker = move || {
        let n = name.get();
        dim.with(|d| {
            let universe = d
                .get(&n)
                .and_then(|h| h.universe.clone())
                .unwrap_or_else(|| "Nexus".into());
            match d.role(&n) {
                Some(r) => format!("Hero · {universe} · {r}"),
                None => format!("Hero · {universe}"),
            }
        })
    };
    let has_games = move || loaded().is_some_and(|d| d.games > 0);
    let num = Signal::derive(move || match loaded() {
        Some(d) if d.games > 0 => {
            if d.builds.is_empty() {
                2
            } else {
                3
            }
        }
        _ => 1,
    });

    view! {
        <div class="ds-page">
            <header class="ds-hhead" style=move || format!("--ring: {}", ring())>
                {move || view! { <Portrait hero=name.get() size=180 class="xl" /> }}
                <div class="ds-hhead-txt">
                    <div class="ds-kicker">{kicker}</div>
                    <h1 class="ds-title" style="cursor: default">{move || name.get()}</h1>
                    <div class="ds-statline" style="margin-top: 16px">
                        <div class="ds-stat"><span class="ds-label">"Win rate"</span>
                            <strong>{move || match data.get() { None => "…".to_string(), Some(_) => loaded().map_or(0, |d| pct(d.wins, d.games)).to_string() }}<span>"%"</span></strong>
                            <small>{move || loaded().map(|d| format!("{}–{} · {} games", d.wins, d.games - d.wins, d.games)).unwrap_or_default()}</small></div>
                        <div class="ds-stat"><span class="ds-label">"Avg K / D / T"</span>
                            <strong>{move || num_or_dash(loaded().and_then(|d| d.avg_kills))}<span>"/"</span>{move || num_or_dash(loaded().and_then(|d| d.avg_deaths))}<span>"/"</span>{move || num_or_dash(loaded().and_then(|d| d.avg_takedowns))}</strong>
                            <small>{move || format!("KDA ratio {}", kda_ratio(loaded().and_then(|d| d.avg_takedowns), loaded().and_then(|d| d.avg_deaths)))}</small></div>
                        {move || {
                            let d = loaded()?;
                            let maps = maps_by_games(&d.by_map);
                            let b = best_map(&maps)?;
                            Some(view! {
                                <div class="ds-stat"><span class="ds-label">"Best map · 5+ games"</span><strong style="font-size: 24px">{b.map.clone().unwrap_or_default()}</strong>
                                    <small>{format!("{}/{} · {}%", b.wins, b.games, pct(b.wins, b.games))}</small></div>
                            })
                        }}
                        <Link class="ds-btn primary" style="align-self: center" to=format!("/matches?hero={}", uri_component(&name.get_untracked()))>"See games ›"</Link>
                    </div>
                    <AggFilterBar value=filter mine_label="My games" />
                </div>
            </header>

            {move || (data.get().is_some() && !has_games()).then(|| view! {
                <div class="ds-empty" style="margin-top: 20px">{format!("No game on {} for this filter.", name.get())}</div>
            })}

            {move || {
                let d = loaded().filter(|d| d.games > 0)?;
                let hero = name.get();
                let maps = maps_by_games(&d.by_map);
                let builds = d.builds.clone();
                Some(view! {
                    <section class="ds-sec">
                        <SecHead num=1 title="Maps" sub="green ≥ 55% · red < 45%" />
                        <div class="ds-mapts">
                            {maps.into_iter().map(|x| {
                                let p = pct(x.wins, x.games);
                                let map = x.map.clone().unwrap_or_default();
                                let art = has_map_art(&map);
                                let t = tone(p);
                                view! {
                                    <Link to=format!("/matches?hero={}&map={}", uri_component(&hero), uri_component(x.map.as_deref().unwrap_or("null")))
                                        class=if art { "ds-mapt" } else { "ds-mapt noart" }
                                        style=if art { map_art(&map) } else { String::new() }>
                                        <strong>{map}</strong>
                                        <div class="ds-mapt-f"><span class=format!("wr {t}")>{p}<i>"%"</i></span><em>{format!("{}/{}", x.wins, x.games)}</em></div>
                                        <i class=format!("ds-bar {}", bar_class(t))><b style=format!("width: {p}%")></b></i>
                                    </Link>
                                }
                            }).collect_view()}
                        </div>
                    </section>
                    {(!builds.is_empty()).then(|| view! {
                        <section class="ds-sec">
                            <SecHead num=2 title="Builds that win" sub="most played talent paths" />
                            <div class="ds-builds">
                                {builds.into_iter().map(|b| {
                                    let p = pct(b.wins, b.games);
                                    view! {
                                        <div class="ds-build">
                                            <div class="ds-build-hd"><strong class=tone(p)>{p}<i>"%"</i></strong><em>{format!("{}/{} games", b.wins, b.games)}</em></div>
                                            <BuildSteps talents=b.talents.clone() dim=talents />
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        </section>
                    })}
                })
            }}

            <HeroPatches hero=name num=num />
        </div>
    }
}
