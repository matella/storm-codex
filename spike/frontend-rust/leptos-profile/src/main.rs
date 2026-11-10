//! Player profile (`/player/:toon`), Leptos 0.8 CSR + leptos_router. Port of `web/src/pages/Player.tsx`:
//! API loading, Stats / Identity tabs driven by `?tab=identity`, tiles, hero pool bars, recent games,
//! loading and "not found" states. Chat section and hero portraits (needs `/api/dim/heroes`, which
//! comes from the box) are left out of the spike: portraits fall back to initials.

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    hooks::{use_navigate, use_params_map, use_query_map},
    path, NavigateOptions,
};
use spike_common::{net, profile::*};


fn main() {
    mount_to_body(|| {
        view! {
            <Router>
                <Routes fallback=|| view! { <div class="ds-page"><div class="ds-empty">"Not found"</div></div> }>
                    <Route path=path!("/player/:toon") view=Player />
                </Routes>
            </Router>
        }
    });
}

#[component]
fn Portrait(hero: String, size: u32, #[prop(optional)] class: &'static str, #[prop(default = "plain")] tone: &'static str) -> impl IntoView {
    // subscribed to the reference data: re-renders when it arrives, like `useDimHeroes` in React
    let dim = expect_context::<RwSignal<Dim>>();
    let h = hero.clone();
    let style = move || format!("--s:{size}px;--ring:{}", dim.with(|d| d.ring(Some(&h))));
    let (h2, h3) = (hero.clone(), hero.clone());
    let (broken, set_broken) = signal(false); // image failed to load: fall back to initials
    let body = move || match dim.with(|d| d.icon(&h2).map(str::to_string)).filter(|_| !broken.get()) {
        Some(src) => view! { <img src=src alt=h3.clone() loading="lazy" on:error=move |_| set_broken.set(true) /> }.into_any(),
        None => view! { <span class="ds-ini">{initials(&h3)}</span> }.into_any(),
    };
    view! { <span class=format!("ds-pt ds-pt-{tone} {class}").trim().to_string() style=style title=hero>{body}</span> }
}

#[component]
fn SecHead(num: u32, title: &'static str, #[prop(optional)] children: Option<Children>) -> impl IntoView {
    view! { <div class="ds-sec-hd"><span class="num">{format!("{num:02}")}</span><h2>{title}</h2>{children.map(|c| view! { <small>{c()}</small> })}</div> }
}

#[component]
fn Player() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let navigate = use_navigate();
    let toon = Memo::new(move |_| params.read().get("toon").unwrap_or_default());
    let tab = Memo::new(move |_| Tab::from_query(query.read().get("tab").as_deref()));
    let (all_heroes, set_all_heroes) = signal(false);
    let dim = RwSignal::new(Dim::default());
    provide_context(dim);
    leptos::task::spawn_local(async move {
        if let Ok(m) = net::get_json::<std::collections::HashMap<String, DimHero>>("/api/dim/heroes").await {
            dim.set(Dim::new(m));
        }
    });

    let player = LocalResource::new(move || {
        let t = toon.get();
        async move { net::get_json::<PlayerSummary>(&format!("/api/players/{t}")).await }
    });
    // loaded apart from the summary (slower), shared by the header and the Identity tab
    let identity = LocalResource::new(move || {
        let t = toon.get();
        async move { net::get_json::<PlayerIdentity>(&format!("/api/players/{t}/identity")).await.ok() }
    });

    // `Callback` is `Copy`, so `go` can be used from inside the reactive closure below
    let navigate = Callback::new(move |to: String| navigate(&to, NavigateOptions::default()));
    let go = move |to: String| {
        move |e: leptos::ev::MouseEvent| {
            e.prevent_default();
            navigate.run(to.clone());
        }
    };

    move || match player.get() {
        None => view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"loading…"</div></div> }.into_any(),
        Some(Err(_)) => view! {
            <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"Player not found. "<a href="/matches">"Back to matches"</a></div></div>
        }.into_any(),
        Some(Ok(d)) => {
            let main = d.heroes.first().map(|h| h.hero.clone());
            let main_ring = main.clone();
            let wr = pct(d.wins, d.matches);
            let ratio = kda_ratio(d.avg_takedowns, d.avg_deaths);
            let max_g = d.heroes.first().map(|h| h.games).unwrap_or(1).max(1);
            let others: Vec<String> = d.names.iter().filter(|n| Some(*n) != d.name.as_ref()).cloned().collect();
            let t = toon.get_untracked();
            let (stats_href, id_href) = (format!("/player/{t}"), format!("/player/{t}?tab=identity"));
            let alias = move || identity.get().flatten().and_then(|i| likely_alias(&i));
            let id_for_link = id_href.clone();
            let id_for_alias = id_href.clone();
            let n_heroes = d.heroes.len();
            let heroes = d.heroes.clone();
            let recent = d.recent.clone();
            let d2 = d.clone();
            view! {
                <div class="ds-page">
                    <header class="ds-hhead" style=move || format!("--ring:{}", dim.with(|d| d.ring(main_ring.as_deref())))>
                        {main.clone().map(|m| view! { <Portrait hero=m size=160 class="xl" /> })}
                        <div class="ds-hhead-txt">
                            <div class="ds-kicker">"Player · "{d.toon.clone()}</div>
                            <h1 class="ds-title" style="cursor: default">{d.name.clone().unwrap_or(d.toon.clone())}</h1>
                            {(!others.is_empty()).then(|| {
                                let more = (others.len() > 3).then(|| {
                                    let href = id_for_link.clone();
                                    view! { " " <a href="?tab=identity" on:click=go(href)>{format!("+{} more", others.len() - 3)}</a> }
                                });
                                view! { <div class="ds-subtitle" style="cursor: default">{format!("also seen as {}", others.iter().take(3).cloned().collect::<Vec<_>>().join(", "))}{more}</div> }
                            })}
                            {move || alias().map(|(toon, name)| {
                                let href = id_for_alias.clone();
                                view! { <div class="ds-subtitle" style="cursor: default"><a href="?tab=identity" on:click=go(href)>{format!("Also plays as {} →", name.unwrap_or(toon))}</a></div> }
                            })}
                            <div class="ds-statline" style="margin-top: 16px">
                                <div class="ds-stat"><span class="ds-label">"Win rate"</span><strong>{wr}<span>"%"</span></strong><small>{format!("{}–{} · {} games", d.wins, d.matches - d.wins, d.matches)}</small></div>
                                <div class="ds-stat"><span class="ds-label">"Avg K / D / T"</span><strong>{num(d.avg_kills)}<span>"/"</span>{num(d.avg_deaths)}<span>"/"</span>{num(d.avg_takedowns)}</strong><small>{format!("KDA ratio {ratio}")}</small></div>
                                <div class="ds-stat"><span class="ds-label">"Avg APM"</span><strong>{num(d.avg_apm)}</strong><small>{if d.apm_games > 0 { format!("{} games measured", d.apm_games) } else { "re-process pending".to_string() }}</small></div>
                                <div class="ds-stat"><span class="ds-label">"Heroes"</span><strong>{n_heroes}</strong><small>{main.as_ref().map(|m| format!("main: {m}")).unwrap_or_default()}</small></div>
                            </div>
                        </div>
                    </header>

                    <nav class="ds-nav" aria-label="Player sections">
                        <a class:map=move || tab.get() == Tab::Stats href="?" on:click=go(stats_href.clone())>"Stats"</a>
                        <a class:map=move || tab.get() == Tab::Identity href="?tab=identity" on:click=go(id_href.clone())>"Identity"</a>
                    </nav>

                    {move || if tab.get() == Tab::Identity {
                        view! { <Identity identity /> }.into_any()
                    } else {
                        let heroes = heroes.clone();
                        let recent = recent.clone();
                        let d2 = d2.clone();
                        view! {
                            <section class="ds-sec">
                                <SecHead num=1 title="Hero pool">
                                    {move || if n_heroes > 18 {
                                        view! { <span class="ds-pill" on:click=move |_| set_all_heroes.update(|a| *a = !*a)>{move || if all_heroes.get() { "top 18".to_string() } else { format!("show all {n_heroes}") }}</span> }.into_any()
                                    } else { "games · win rate · avg K/D/T".into_any() }}
                                </SecHead>
                                <div class="ds-panel"><div class="ds-pool">
                                    {move || heroes.iter().take(if all_heroes.get() { usize::MAX } else { 18 }).map(|h| {
                                        let p = pct(h.wins, h.games);
                                        let wr_class = format!("wr {}", if p >= 55 { "hot" } else if p < 45 { "cold" } else { "" });
                                        view! {
                                            <a class="ds-poolrow" href=format!("/hero/{}", h.hero)>
                                                <Portrait hero=h.hero.clone() size=34 />
                                                <span>{h.hero.clone()}</span>
                                                <i class="ds-bar"><b style=format!("width:{}%", (h.games * 100 + max_g / 2) / max_g) /></i>
                                                <span class=wr_class>{format!("{p}% · {}g", h.games)}</span>
                                                <em>{format!("{}/{}/{}", num(h.avg_kills), num(h.avg_deaths), num(h.avg_takedowns))}</em>
                                            </a>
                                        }
                                    }).collect_view()}
                                </div></div>
                            </section>
                            <section class="ds-sec">
                                <SecHead num=2 title="Recent games" />
                                <div class="ds-mlist">
                                    {recent.iter().map(|g| recent_row(g)).collect_view()}
                                </div>
                            </section>
                            {let _ = &d2; ()}
                        }.into_any()
                    }}
                </div>
            }.into_any()
        }
    }
}

fn recent_row(g: &PlayerGame) -> impl IntoView {
    let win = g.win == Some(true);
    let aw = award_label(g.award.as_deref());
    let a = (g.takedowns.unwrap_or(0.0) - g.kills.unwrap_or(0.0)).max(0.0);
    let (short, long, fam) = mode_tag(g.mode);
    let art = has_map_art(g.map.as_deref()).then(|| g.map.as_deref().map(|m| format!("--img:url('/images/minimaps/{}.jpg')", spike_common::logic::map_slug(m)))).flatten();
    let id = g.match_id;
    let go = use_navigate();
    let go2 = go.clone();
    view! {
        <div class=format!("ds-mrow {}", if win { "w" } else { "l" }) style=art role="link" tabindex="0"
            on:click=move |_| go(&format!("/match/{id}"), NavigateOptions::default())
            on:keydown=move |e| if e.key() == "Enter" { go2(&format!("/match/{id}"), NavigateOptions::default()) }>
            <span class="when">{net::fmt_time(g.played_at.as_deref())}</span>
            <span><span class=format!("ds-mode {fam}") title=long>{short}</span></span>
            {match g.hero.clone() { Some(h) => view! { <Portrait hero=h size=40 tone=if win { "pick" } else { "loss" } /> }.into_any(), None => view! { <span /> }.into_any() }}
            <span class="what"><span class="map">{g.map.clone().unwrap_or("—".into())}</span>
                <span class="hero">{g.hero.clone().unwrap_or_default()}" · "<span class="ds-kda">{num(g.kills)}<i>"/"</i>{num(g.deaths)}<i>"/"</i>{num(Some(a))}</span></span></span>
            <span style="display: flex; gap: 6px">
                {g.apm.map(|apm| view! { <span class="ds-tag" title="actions per minute">{format!("{} APM", num(Some(apm)))}</span> })}
                <span class=format!("ds-tag {}", if win { "win" } else { "loss" })>{if win { "WIN" } else { "LOSS" }}</span>
                {aw.filter(|a| a.1).map(|_| view! { <span class="ds-tag mvp">"👑 MVP"</span> })}
            </span>
            <span class="len">"›"</span>
        </div>
    }
}

#[component]
fn Identity(identity: LocalResource<Option<PlayerIdentity>>) -> impl IntoView {
    view! {
        <section class="ds-sec">
            {move || match identity.get().flatten() {
                None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                Some(i) => view! {
                    <div class="ds-panel">
                        <ul>{i.names.iter().map(|n| view! { <li>{format!("{} · {} games", n.name, n.games)}</li> }).collect_view()}</ul>
                        <p>{format!("{} confirmed link(s), {} suggestion(s)", i.confirmed.len(), i.suggestions.len())}</p>
                    </div>
                }.into_any(),
            }}
        </section>
    }
}
