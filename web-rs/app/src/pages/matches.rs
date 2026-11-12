//! Matches: the filterable archive. Filters live in the URL, so they survive the back button.
//! Port of `web/src/pages/Matches.tsx` (logic in `core::dashboard`).

use leptos::prelude::*;
use leptos_router::{
    hooks::{use_location, use_navigate},
    NavigateOptions,
};
use storm_web_core::{
    aggregates::{has_map_art, HeroStat, MapStat, MODES},
    dashboard::{own_player, set_param, MatchFilter},
    net,
    session::{award_label, fmt_dur, MatchSummary},
};

use crate::{
    layout::LiveBus,
    matchui::{ModeTag, SearchSelect},
    ui::{fetch, map_art, use_operator, Portrait, Tone},
};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

const LIMIT: &str = "200";

#[component]
pub fn Matches() -> impl IntoView {
    let accounts = use_operator();
    let nav = StoredValue::new_local(use_navigate());
    let loc = use_location();
    let bus = expect_context::<LiveBus>();
    let search = move || loc.search.get();
    let filter = Memo::new(move |_| MatchFilter::from_search(&search()));
    // sets one URL parameter, replacing the history entry
    let set_param_url = StoredValue::new_local(move |k: &'static str, v: String| {
        let q = set_param(&loc.search.get_untracked(), k, &v);
        let path = loc.pathname.get_untracked();
        let path = storm_web_core::route::app_path(&path).to_string();
        nav.with_value(|nav| {
            nav(
                &if q.is_empty() {
                    path
                } else {
                    format!("{path}?{q}")
                },
                NavigateOptions {
                    replace: true,
                    ..Default::default()
                },
            )
        });
    });
    let reset = move || {
        let path = loc.pathname.get_untracked();
        nav.with_value(|nav| {
            nav(
                storm_web_core::route::app_path(&path),
                NavigateOptions {
                    replace: true,
                    ..Default::default()
                },
            )
        });
    };
    let bounds = move |f: &MatchFilter| {
        (
            (!f.from.is_empty())
                .then(|| net::local_midnight_iso(&f.from, 0))
                .flatten(),
            (!f.to.is_empty())
                .then(|| net::local_midnight_iso(&f.to, 1))
                .flatten(),
        )
    };
    let data = fetch::<Vec<MatchSummary>>(move || {
        bus.matches.track();
        let f = filter.get();
        let (a, b) = bounds(&f);
        format!(
            "/api/matches?{}",
            f.api_query(a.as_deref(), b.as_deref(), &[("limit", LIMIT)])
        )
    });
    let maps = fetch::<Vec<MapStat>>(|| "/api/maps".to_string());
    let heroes = fetch::<Vec<HeroStat>>(|| "/api/heroes".to_string());
    let map_options = Signal::derive(move || {
        let mut v: Vec<String> = maps
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.map)
            .collect();
        v.sort_by_key(|s| s.to_lowercase());
        v
    });
    let hero_options = Signal::derive(move || {
        let mut v: Vec<String> = heroes
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|h| h.hero)
            .collect();
        v.sort_by_key(|s| s.to_lowercase());
        v
    });
    let active = move || !search().trim_start_matches('?').is_empty();
    let export_q = move |limit: &'static str| {
        let f = filter.get();
        let (a, b) = bounds(&f);
        f.api_query(a.as_deref(), b.as_deref(), &[("limit", limit)])
    };
    let list = move || data.get().and_then(Result::ok).unwrap_or_default();

    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Archive"</div>
                <h1 class="ds-title" style="cursor: default">"Matches"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || {
                        if data.get().is_none() {
                            return "loading…".into_any();
                        }
                        let l = list();
                        let names = accounts.get();
                        let known: Vec<bool> = l.iter().filter_map(|m| own_player(m, &names).and_then(|p| p.win)).collect();
                        let w = known.iter().filter(|x| **x).count();
                        view! {
                            {format!("{} game{}{}", l.len(), if l.len() == 1 { "" } else { "s" }, if l.len() == 200 { " (latest 200)" } else { "" })}
                            {(!known.is_empty()).then(|| view! {
                                " · "<b style="color: var(--win)">{w}</b>"–"<b style="color: var(--loss-soft)">{known.len() - w}</b>" for you"
                            })}
                        }.into_any()
                    }}
                    {move || active().then_some(" · filtered")}
                </div>
                <div class="ds-filters">
                    <div class="ds-pills">
                        {MODES.iter().map(|&(label, m)| {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! {
                                <span class=move || pill(filter.with(|f| f.mode == m)) on:click=move |_| set("mode", m.map(|x| x.to_string()).unwrap_or_default())>{label}</span>
                            }
                        }).collect_view()}
                        <span class="ds-sep"></span>
                        {[("", "W+L"), ("win", "Wins"), ("loss", "Losses")].into_iter().map(|(r, label)| {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <span class=move || pill(filter.with(|f| f.result == r)) on:click=move |_| set("result", r.to_string())>{label}</span> }
                        }).collect_view()}
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <span class=move || pill(filter.with(|f| f.mvp)) on:click=move |_| set("mvp", if filter.with_untracked(|f| f.mvp) { String::new() } else { "true".into() })>"👑 MVP"</span> }
                        }
                    </div>
                    <div class="ds-pills">
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <SearchSelect id="dl-map" style="width: 170px" placeholder="search map…" options=map_options
                                value=Signal::derive(move || filter.with(|f| f.map.clone())) on_change=move |v| set("map", v) /> }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <SearchSelect id="dl-hero" style="width: 170px" placeholder="search hero…" options=hero_options
                                value=Signal::derive(move || filter.with(|f| f.hero.clone())) on_change=move |v| set("hero", v) /> }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            move || {
                                let list = accounts.get();
                                                                (list.len() > 1).then(|| view! {
                                    <select class="ds-input" on:change=move |e| set("account", event_target_value(&e))>
                                        <option value="" selected=move || filter.with(|f| f.account.is_empty())>"All my accounts"</option>
                                        {list.into_iter().map(|a| {
                                            let (a1, a2) = (a.clone(), a.clone());
                                            view! { <option value=a1 selected=move || filter.with(|f| f.account == a2)>{a}</option> }
                                        }).collect_view()}
                                    </select>
                                })
                            }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <label>"from "<input type="date" class="ds-input" prop:value=move || filter.with(|f| f.from.clone()) on:input=move |e| set("from", event_target_value(&e)) /></label> }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <label>"to "<input type="date" class="ds-input" prop:value=move || filter.with(|f| f.to.clone()) on:input=move |e| set("to", event_target_value(&e)) /></label> }
                        }
                        <Show when=active>
                            <span class="ds-pill" on:click=move |_| reset()>"✕ reset"</span>
                        </Show>
                        <span style="margin-left: auto; display: flex; gap: 6px">
                            <a href=move || format!("/api/matches.csv?{}", export_q("5000")) class="ds-btn">"CSV ↓"</a>
                            <a href=move || format!("/api/matches?{}", export_q("5000")) class="ds-btn" target="_blank" rel="noreferrer">"JSON ↓"</a>
                        </span>
                    </div>
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || data.get().is_none().then(|| view! { <div class="ds-empty">"loading…"</div> })}
                {move || (data.get().is_some() && list().is_empty()).then(|| view! { <div class="ds-empty">"No match for this filter."</div> })}
                <div class="ds-mlist">
                    {move || {
                        let names = accounts.get();
                        list().into_iter().map(|m| {
                            let own = own_player(&m, &names);
                            let (hero, win, award) = (
                                own.and_then(|p| p.hero.clone()),
                                own.and_then(|p| p.win),
                                own.and_then(|p| p.award.clone()),
                            );
                            let aw = award_label(award.as_deref());
                            let cls = match win { Some(true) => "ds-mrow w", Some(false) => "ds-mrow l", None => "ds-mrow " };
                            let map = m.map.clone().unwrap_or_default();
                            let style = if has_map_art(&map) { map_art(&map) } else { String::new() };
                            let id = m.id;
                            let go = move || nav.with_value(|nav| nav(&format!("/match/{id}"), Default::default()));
                            let go2 = go;
                            let tone = match win { Some(true) => Tone::Pick, Some(false) => Tone::Loss, None => Tone::Plain };
                            let what = hero.clone().unwrap_or_else(|| format!("{} team wins", match m.winner { Some(0) => "Blue", Some(1) => "Red", _ => "?" }));
                            view! {
                                <div class=cls style=style on:click=move |_| go() role="link" tabindex="0" on:keydown=move |e| if e.key() == "Enter" { go2() }>
                                    <span class="when">{net::fmt_time(m.played_at.as_deref())}</span>
                                    <span><ModeTag mode=m.mode short=true /></span>
                                    {match hero { Some(h) => view! { <Portrait hero=h size=40 tone=tone /> }.into_any(), None => view! { <span></span> }.into_any() }}
                                    <span class="what"><span class="map">{if map.is_empty() { "—".to_string() } else { map }}</span><span class="hero">{what}</span></span>
                                    <span style="display: flex; gap: 6px">
                                        {win.map(|w| view! { <span class=format!("ds-tag {}", if w { "win" } else { "loss" })>{if w { "WIN" } else { "LOSS" }}</span> })}
                                        {aw.filter(|a| a.mvp).map(|_| view! { <span class="ds-tag mvp">"👑 MVP"</span> })}
                                    </span>
                                    <span class="len">{format!("{} ›", fmt_dur(m.length))}</span>
                                </div>
                            }
                        }).collect_view()
                    }}
                </div>
            </section>
        </div>
    }
}
