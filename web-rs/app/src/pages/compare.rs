//! Head to head: two players side by side and the games they shared (spec
//! `docs/specs/2026-11-17-local-insights-design.md`, F6). `?a=<toon>&b=<toon>` in the URL.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{
    hooks::{use_location, use_navigate},
    NavigateOptions,
};
use storm_web_core::{
    insights::{pct, Card, Compare},
    net,
    players::PlayerSearch,
    route::{query_value, uri_component},
};

use crate::ui::{fetch_reference, Link, Portrait, SecHead};

/// Name field + "Find" → a list of accounts to pick from. Never picks the first hit on its own.
#[component]
pub(crate) fn Picker(label: &'static str, on_pick: Callback<String>) -> impl IntoView {
    let q = RwSignal::new(String::new());
    let hits = RwSignal::new(None::<PlayerSearch>);
    let find = move || {
        let text = q.get_untracked();
        if text.trim().chars().count() < 2 {
            return;
        }
        spawn_local(async move {
            let url = format!(
                "/api/players/search?q={}&limit=8",
                uri_component(text.trim())
            );
            hits.set(net::get_json::<PlayerSearch>(&url).await.ok());
        });
    };
    view! {
        <div class="ds-panel">
            <span class="ds-label">{label}</span>
            <div class="ds-filterbar">
                <input class="ds-input" placeholder="player name" prop:value=move || q.get()
                    on:input=move |e| q.set(event_target_value(&e))
                    on:keydown=move |e| if e.key() == "Enter" { find() } />
                <span class="ds-pill" on:click=move |_| find()>"Find"</span>
            </div>
            {move || hits.get().map(|h| if h.hits.is_empty() {
                view! { <div class="ds-empty">"No player found."</div> }.into_any()
            } else {
                h.hits.into_iter().map(|p| {
                    let t = p.toon.clone();
                    view! {
                        <div class="ds-kv" style="cursor: pointer" on:click=move |_| { hits.set(None); on_pick.run(t.clone()) }>
                            <span>{p.name.clone().unwrap_or(p.matched_name.clone())}</span>
                            <span class="ds-num">{format!("{} games", p.games)}</span>
                        </div>
                    }
                }).collect_view().into_any()
            })}
        </div>
    }
}

#[component]
fn PlayerCard(c: Card) -> impl IntoView {
    let wr = pct(c.wins, c.games);
    let name = c.name.clone().unwrap_or_else(|| c.toon.clone());
    view! {
        <div class="ds-panel">
            <div class="ds-kicker"><Link to=format!("/player/{}", uri_component(&c.toon))>{name}</Link>
                " · "<Link to=format!("/matches?with={}", uri_component(&c.toon))>"your games with ›"</Link>
                " · "<Link to=format!("/matches?against={}", uri_component(&c.toon))>"against ›"</Link></div>
            <div class="ds-statline">
                <div class="ds-stat"><span class="ds-label">"Win rate"</span><strong>{wr}<span>"%"</span></strong><small>{format!("{}–{} · {} games", c.wins, c.games.saturating_sub(c.wins), c.games)}</small></div>
                <div class="ds-stat"><span class="ds-label">"Avg K / D / A"</span><strong>
                    {c.kills.map_or("—".into(), |v| v.to_string())}<span>"/"</span>
                    {c.deaths.map_or("—".into(), |v| v.to_string())}<span>"/"</span>
                    {c.assists.map_or("—".into(), |v| v.to_string())}
                </strong></div>
            </div>
            <div class="ds-chips" style="margin-top: 12px">
                {c.heroes.into_iter().map(|h| view! {
                    <span class="ds-chip"><Portrait hero=h.hero.clone() size=24 />{format!(" {} · {}g · {}%", h.hero, h.games, pct(h.wins, h.games))}</span>
                }).collect_view()}
            </div>
        </div>
    }
}

#[component]
pub fn ComparePage() -> impl IntoView {
    let loc = use_location();
    let nav = StoredValue::new_local(use_navigate());
    let a = Signal::derive(move || query_value(&loc.search.get(), "a").filter(|s| !s.is_empty()));
    let b = Signal::derive(move || query_value(&loc.search.get(), "b").filter(|s| !s.is_empty()));
    let set = move |key: &'static str, toon: String| {
        let (mut na, mut nb) = (a.get_untracked(), b.get_untracked());
        if key == "a" {
            na = Some(toon)
        } else {
            nb = Some(toon)
        }
        let q = format!(
            "/compare?a={}&b={}",
            uri_component(&na.unwrap_or_default()),
            uri_component(&nb.unwrap_or_default())
        );
        nav.with_value(|n| {
            n(
                &q,
                NavigateOptions {
                    replace: true,
                    ..Default::default()
                },
            )
        });
    };
    let data = fetch_reference::<Compare>(move || match (a.get(), b.get()) {
        (Some(a), Some(b)) => format!(
            "/api/compare?a={}&b={}",
            uri_component(&a),
            uri_component(&b)
        ),
        // nothing picked yet: a cheap 400 from the API (the page shows a prompt, not the error)
        _ => "/api/compare".to_string(),
    });
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Head to head"</div>
                <h1 class="ds-title" style="cursor: default">"Compare two players"</h1>
                <div class="ds-subtitle" style="cursor: default">"Records, top heroes, and every game they shared."</div>
            </header>
            <section class="ds-sec ds-cmp" style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px">
                <Picker label="Player A" on_pick=Callback::new(move |t| set("a", t)) />
                <Picker label="Player B" on_pick=Callback::new(move |t| set("b", t)) />
            </section>
            {move || {
                if a.get().is_none() || b.get().is_none() {
                    return view! { <div class="ds-empty">"Pick two players to compare."</div> }.into_any();
                }
                match data.get() {
                    None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                    Some(Err(e)) => view! { <div class="ds-empty">{format!("Could not compare ({e}).")}</div> }.into_any(),
                    Some(Ok(c)) => {
                        let an = c.a.name.clone().unwrap_or(c.a.toon.clone());
                        let bn = c.b.name.clone().unwrap_or(c.b.toon.clone());
                        let rows = c.shared.into_iter().map(|g| {
                            let won = g.a_win == Some(true);
                            view! {
                                <tr>
                                    <td><Link to=format!("/match/{}", g.match_id)>{net::fmt_time(g.played_at.as_deref())}</Link></td>
                                    <td>{g.map.clone().unwrap_or_default()}</td>
                                    <td>{if g.together { "together" } else { "against" }}</td>
                                    <td>{g.a_hero.clone().unwrap_or_default()}</td>
                                    <td>{g.b_hero.clone().unwrap_or_default()}</td>
                                    <td class=if won { "ds-us" } else { "ds-them" }>{if g.together { if won { "won" } else { "lost" } } else if won { "A won" } else { "B won" }}</td>
                                </tr>
                            }
                        }).collect_view();
                        view! {
                            <section class="ds-sec" style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px">
                                <PlayerCard c=c.a />
                                <PlayerCard c=c.b />
                            </section>
                            <section class="ds-sec">
                                <SecHead num=1 title="Shared games" />
                                <div class="ds-panel">
                                    <div class="ds-statline">
                                        <div class="ds-stat"><span class="ds-label">"Together"</span><strong>{c.together.wins}<span>"–"</span>{c.together.games.saturating_sub(c.together.wins)}</strong><small>{format!("{} games · {}%", c.together.games, pct(c.together.wins, c.together.games))}</small></div>
                                        <div class="ds-stat"><span class="ds-label">"Against"</span><strong>{c.against.a_wins}<span>"–"</span>{c.against.games.saturating_sub(c.against.a_wins)}</strong><small>{format!("{an} – {bn}")}</small></div>
                                    </div>
                                </div>
                                <div class="ds-panel flush scroll" style="margin-top: 12px">
                                    <table class="ds-table">
                                        <thead><tr><th>"When"</th><th>"Map"</th><th>""</th><th>"A"</th><th>"B"</th><th>"Result"</th></tr></thead>
                                        <tbody>{rows}</tbody>
                                    </table>
                                </div>
                            </section>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}
