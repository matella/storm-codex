//! Matches: the filterable archive. Filters live in the URL, so they survive the back button.
//! Port of `web/src/pages/Matches.tsx` (logic in `core::dashboard`).

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{
    hooks::{use_location, use_navigate},
    NavigateOptions,
};
use storm_web_core::{
    aggregates::{has_map_art, HeroStat, MapStat, MODES},
    dashboard::{
        archive_player,
        paging::{ArchivePages, PageIssue},
        set_params, MatchFilter, SEARCH_STATS,
    },
    net,
    session::{award_label, fmt_dur, MatchSummary},
};

use crate::{
    matchui::{ModeTag, SearchSelect},
    pages::compare::Picker,
    ui::{
        archive_generation, fetch, fetch_reference, map_art, use_account_options, Link, Portrait,
        Tone,
    },
};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

const LIMIT: &str = "100";

#[component]
pub fn Matches() -> impl IntoView {
    let accounts = use_account_options();
    let nav = StoredValue::new_local(use_navigate());
    let loc = use_location();

    let search = move || loc.search.get();
    let filter = Memo::new(move |_| MatchFilter::from_search(&search()));
    // sets URL parameters (empty value = removed), replacing the history entry
    let set_params_url = StoredValue::new_local(move |kv: Vec<(String, String)>| {
        let kv: Vec<(&str, &str)> = kv.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let q = set_params(&loc.search.get_untracked(), &kv);
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
    let set_param_url = StoredValue::new_local(move |k: &str, v: String| {
        set_params_url.with_value(|f| f(vec![(k.to_string(), v)]))
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
    let retry_generation = RwSignal::new(0u64);
    let generation = archive_generation(move || {
        retry_generation.track();
        let f = filter.get();
        let (a, b) = bounds(&f);
        format!(
            "/api/matches?{}",
            f.api_query(a.as_deref(), b.as_deref(), &[("limit", LIMIT)])
        )
    });
    let paging = RwSignal::new(ArchivePages::default());
    let request_page = move |(epoch, url): (u64, String)| {
        let request = paging
            .try_update(|p| (p.generation == epoch).then(|| p.start()).flatten())
            .flatten();
        if let Some(request) = request {
            let url = format!("{url}&offset={}", request.offset);
            spawn_local(async move {
                let result = net::get_json_counted::<Vec<MatchSummary>>(&url)
                    .await
                    .and_then(|(rows, total)| {
                        total
                            .map(|total| (rows, total))
                            .ok_or_else(|| "Missing match count".to_string())
                    });
                // Compare the live generation, not just the last completed page or scheduled reset.
                if generation.try_with_untracked(|(current, _)| *current == epoch) == Some(true) {
                    paging.update(|p| p.finish(epoch, request, result));
                }
            });
        }
    };
    Effect::new(move |_| {
        let key = generation.get();
        paging.set(ArchivePages::new(key.0));
        request_page(key);
    });
    let current = move || generation.with(|(epoch, _)| paging.with(|p| p.generation == *epoch));
    let total = move || current().then(|| paging.with(|p| p.total)).flatten();
    let issue = move || {
        current()
            .then(|| paging.with(|p| p.issue.clone()))
            .flatten()
    };
    let loading = move || total().is_none() && issue().is_none();
    let list = move || {
        if current() {
            paging.with(|p| p.rows.clone())
        } else {
            Vec::new()
        }
    };
    let load_more = move || request_page(generation.get_untracked());
    let has_more = move || current() && paging.with(ArchivePages::has_more);
    let more_busy = move || current() && paging.with(ArchivePages::busy);
    let reload = move || retry_generation.update(|n| *n += 1);
    let maps = fetch::<Vec<MapStat>>(|| "/api/maps".to_string());
    let heroes = fetch_reference::<Vec<HeroStat>>(|| "/api/heroes".to_string());
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
    // display name of a toon in the loaded games (the URL only carries the toon)
    let toon_name = move |toon: &str| {
        list()
            .iter()
            .flat_map(|m| m.players.iter().flatten())
            .find(|p| p.toon.as_deref() == Some(toon))
            .and_then(|p| p.name.clone())
            .unwrap_or_else(|| toon.to_string())
    };
    let chips = move || filter.with(|f| f.chips(toon_name));
    // the stat row edits one stat at a time; it follows the URL, and keeps a stat picked without bounds yet
    let stat = RwSignal::new(filter.with_untracked(|f| f.bound_stat()));
    Effect::new(move |_| {
        filter.with(|f| {
            if !f.bounds.is_empty() {
                stat.set(f.bound_stat());
            }
        })
    });
    let export_q = move |limit: &'static str| {
        let f = filter.get();
        let (a, b) = bounds(&f);
        f.api_query(a.as_deref(), b.as_deref(), &[("limit", limit)])
    };
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Archive"</div>
                <h1 class="ds-title" style="cursor: default">"Matches"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || {
                        if loading() {
                            return "loading…".into_any();
                        }
                        let l = list();
                        let known: Vec<bool> = l.iter().filter_map(|m| archive_player(m).and_then(|p| p.win)).collect();
                        let w = known.iter().filter(|x| **x).count();
                        let n = total().map_or(l.len(), |t| t as usize);
                        view! {
                            {format!("{n} game{}{}", if n == 1 { "" } else { "s" }, if l.len() < n { format!(" (showing {})", l.len()) } else { String::new() })}
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
                                <button type="button" class=move || pill(filter.with(|f| f.mode == m))
                                    aria-pressed=move || filter.with(|f| f.mode == m).to_string()
                                    on:click=move |_| set("mode", m.map(|x| x.to_string()).unwrap_or_default())>{label}</button>
                            }
                        }).collect_view()}
                        <span class="ds-sep"></span>
                        {[("", "W+L"), ("win", "Wins"), ("loss", "Losses")].into_iter().map(|(r, label)| {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <button type="button" class=move || pill(filter.with(|f| f.result == r))
                                aria-pressed=move || filter.with(|f| f.result == r).to_string()
                                on:click=move |_| set("result", r.to_string())>{label}</button> }
                        }).collect_view()}
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <button type="button" class=move || pill(filter.with(|f| f.mvp))
                                aria-pressed=move || filter.with(|f| f.mvp).to_string()
                                on:click=move |_| set("mvp", if filter.with_untracked(|f| f.mvp) { String::new() } else { "true".into() })>"👑 MVP"</button> }
                        }
                    </div>
                    <div class="ds-pills">
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <SearchSelect id="dl-map" clear_label="Clear map filter" style="width: 170px" placeholder="search map…" options=map_options
                                value=Signal::derive(move || filter.with(|f| f.map.clone())) on_change=move |v| set("map", v) /> }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            view! { <SearchSelect id="dl-hero" clear_label="Clear my hero filter" style="width: 170px" placeholder="my hero…" options=hero_options
                                value=Signal::derive(move || filter.with(|f| f.my_hero.clone())) on_change=move |v| set("my_hero", v) /> }
                        }
                        {
                            let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                            move || {
                                let list = accounts.get();
                                (list.len() > 1).then(|| view! {
                                    <select class="ds-input" on:change=move |e| set("account", event_target_value(&e))>
                                        <option value="" selected=move || filter.with(|f| f.account.is_empty())>"All my accounts"</option>
                                        {list.into_iter().map(|(a1, a)| {
                                            let a2 = a1.clone();
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
                            <button type="button" class="ds-pill" on:click=move |_| reset()>"✕ reset"</button>
                        </Show>
                        <span style="margin-left: auto; display: flex; gap: 6px">
                            <a href=move || format!("/api/matches.csv?{}", export_q("5000")) class="ds-btn">"CSV ↓"</a>
                            <a href=move || format!("/api/matches?{}", export_q("5000")) class="ds-btn" target="_blank" rel="noreferrer">"JSON ↓"</a>
                        </span>
                    </div>
                    <details class="ds-more" open=move || !chips().is_empty()>
                        <summary class="ds-pill">"More filters"</summary>
                        <div class="ds-pills">
                            {
                                let set = move |k: &'static str, v: String| set_param_url.with_value(|f| f(k, v));
                                view! { <SearchSelect id="dl-anyhero" clear_label="Clear any player’s hero filter" style="width: 190px" placeholder="any player's hero…" options=hero_options
                                    value=Signal::derive(move || filter.with(|f| f.hero.clone())) on_change=move |v| set("hero", v) /> }
                            }
                            <select class="ds-input" aria-label="stat" on:change=move |e| {
                                let next = event_target_value(&e);
                                let Some(&(next, _)) = SEARCH_STATS.iter().find(|(k, _)| *k == next) else { return };
                                let prev = stat.get_untracked();
                                stat.set(next);
                                let (lo, hi) = filter.with_untracked(|f| (f.bound(&format!("min_{prev}")), f.bound(&format!("max_{prev}"))));
                                if !lo.is_empty() || !hi.is_empty() {
                                    set_params_url.with_value(|f| f(vec![
                                        (format!("min_{prev}"), String::new()), (format!("max_{prev}"), String::new()),
                                        (format!("min_{next}"), lo), (format!("max_{next}"), hi),
                                    ]));
                                }
                            }>
                                {SEARCH_STATS.iter().map(|&(k, label)| view! { <option value=k selected=move || stat.get() == k>{label}</option> }).collect_view()}
                            </select>
                            {["min", "max"].into_iter().map(|side| view! {
                                <label>{if side == "min" { "≥ " } else { "≤ " }}
                                    <input type="number" min="0" class="ds-input" style="width: 90px"
                                        prop:value=move || filter.with(|f| f.bound(&format!("{side}_{}", stat.get())))
                                        on:change=move |e| set_param_url.with_value(|f| f(&format!("{side}_{}", stat.get_untracked()), event_target_value(&e))) />
                                </label>
                            }).collect_view()}
                            <span class="ds-sep"></span>
                            {[("min_len", "length ≥ "), ("max_len", "≤ ")].into_iter().map(|(k, label)| view! {
                                <label>{label}
                                    <input type="number" min="0" class="ds-input" style="width: 70px"
                                        prop:value=move || filter.with(|f| if k == "min_len" { f.min_len.clone() } else { f.max_len.clone() })
                                        on:change=move |e| set_param_url.with_value(|f| f(k, event_target_value(&e))) />
                                </label>
                            }).collect_view()}
                            " min"
                        </div>
                        <div class="ds-more-players">
                            <Picker label="Played with" on_pick=Callback::new(move |t: String| set_param_url.with_value(|f| f("with", t))) />
                            <Picker label="Played against" on_pick=Callback::new(move |t: String| set_param_url.with_value(|f| f("against", t))) />
                        </div>
                    </details>
                    {move || {
                        let c = chips();
                        (!c.is_empty()).then(|| view! {
                            <div class="ds-chips">
                                {c.into_iter().map(|(k, label)| view! {
                                    <span class="ds-chip text">{label.clone()}" "
                                        <button type="button" aria-label=format!("Remove {label}")
                                            on:click=move |_| set_param_url.with_value(|f| f(&k, String::new()))>"✕"</button>
                                    </span>
                                }).collect_view()}
                            </div>
                        })
                    }}
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || loading().then(|| view! { <div class="ds-empty">"loading…"</div> })}
                {move || (total() == Some(0) && issue().is_none()).then(|| view! { <div class="ds-empty">"No match for this filter."</div> })}
                <div class="ds-mlist">
                    {move || {
                        list().into_iter().map(|m| {
                            let own = archive_player(&m);
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
                            let tone = match win { Some(true) => Tone::Pick, Some(false) => Tone::Loss, None => Tone::Plain };
                            let what = hero.clone().unwrap_or_else(|| format!("{} team wins", match m.winner { Some(0) => "Blue", Some(1) => "Red", _ => "?" }));
                            view! {
                                <Link to=format!("/match/{id}") class=cls.to_string() style=style>
                                    <span class="when">{net::fmt_time(m.played_at.as_deref())}</span>
                                    <span><ModeTag mode=m.mode short=true /></span>
                                    {match hero { Some(h) => view! { <Portrait hero=h size=40 tone=tone /> }.into_any(), None => view! { <span></span> }.into_any() }}
                                    <span class="what"><span class="map">{if map.is_empty() { "—".to_string() } else { map }}</span><span class="hero">{what}</span></span>
                                    <span style="display: flex; gap: 6px">
                                        {win.map(|w| view! { <span class=format!("ds-tag {}", if w { "win" } else { "loss" })>{if w { "WIN" } else { "LOSS" }}</span> })}
                                        {aw.filter(|a| a.mvp).map(|_| view! { <span class="ds-tag mvp">"👑 MVP"</span> })}
                                    </span>
                                    <span class="len">{format!("{} ›", fmt_dur(m.length))}</span>
                                </Link>
                            }
                        }).collect_view()
                    }}
                </div>
                {move || issue().map(|issue| {
                    let (message, action, restart) = match issue {
                        PageIssue::Failed(e) => (match e.as_str() {
                            "perspective_required" => "These filters apply to your own games: set your in-game name first (Admin → My identity).".to_string(),
                            "bad_filter" => "A filter value is not valid.".to_string(),
                            _ => format!("Could not load the matches ({e})."),
                        }, "Retry", false),
                        PageIssue::NoProgress { .. } => ("This page did not add any matches.".to_string(), "Reload results", true),
                    };
                    view! {
                        <div class="ds-empty" role="status">{message} " "
                            <button type="button" class="ds-btn" on:click=move |_| if restart { reload() } else { load_more() }>{action}</button>
                        </div>
                    }
                })}
                <Show when=move || has_more() && !matches!(issue(), Some(PageIssue::Failed(_)))>
                    <div style="display: flex; justify-content: center; margin-top: 14px">
                        <button type="button" class="ds-btn" disabled=more_busy on:click=move |_| load_more()>
                            {move || if more_busy() { "loading…" } else { "Load more" }}
                        </button>
                    </div>
                </Show>
            </section>
        </div>
    }
}
