//! Heroes: the hero wall over the filtered set: portrait, games, win rate (coloured gauge).
//! Port of `web/src/pages/Heroes.tsx`.

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use storm_web_core::{
    aggregates::{
        agg_to_search, bar_class, hero_pct, hero_roles, hero_rows, tone, AggFilter, HeroSort,
        HeroStat,
    },
    net,
    route::uri_component,
};

use crate::ui::{fetch, use_dim, AggFilterBar, Portrait};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

#[component]
pub fn Heroes() -> impl IntoView {
    let sort = RwSignal::new(HeroSort::Games);
    let role = RwSignal::new(String::new());
    let filter = RwSignal::new(AggFilter::default());
    let nav = use_navigate();
    let dim = use_dim();
    let data =
        fetch::<Vec<HeroStat>>(move || format!("/api/heroes?{}", net::agg_params(&filter.get())));
    // roles of the loaded rows, read with the reference data once it has arrived
    let roles = move || match data.get() {
        Some(Ok(d)) => dim.with(|dm| hero_roles(&d, dm)),
        _ => Vec::new(),
    };
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Heroes"</div>
                <h1 class="ds-title" style="cursor: default">"Hero pool"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || match data.get() {
                        Some(Ok(d)) => format!("{} heroes · {} picks over the filtered set", d.len(), d.iter().map(|h| h.games).sum::<i64>()),
                        _ => "loading…".to_string(),
                    }}
                    " — “My heroes” keeps only your accounts."
                </div>
                <AggFilterBar value=filter mine_label="My heroes" />
                <div class="ds-filterbar" style="margin-top: 10px">
                    <span class="ds-label" style="margin: 0">"Sort"</span>
                    <span class=move || pill(sort.get() == HeroSort::Games) on:click=move |_| sort.set(HeroSort::Games)>"Most played"</span>
                    <span class=move || pill(sort.get() == HeroSort::WinRate) on:click=move |_| sort.set(HeroSort::WinRate)>"Win rate"</span>
                    {move || (!roles().is_empty()).then(|| view! { <span class="ds-sep"></span> })}
                    {move || (!roles().is_empty()).then(|| view! {
                        <span class=move || pill(role.with(String::is_empty)) on:click=move |_| role.set(String::new())>"All roles"</span>
                    })}
                    {move || roles().into_iter().map(|r| {
                        let r2 = r.clone();
                        let r3 = r.clone();
                        view! { <span class=move || pill(role.with(|c| *c == r2)) on:click=move |_| role.set(r3.clone())>{r}</span> }
                    }).collect_view()}
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || match data.get() {
                    None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                    Some(Err(_)) => ().into_any(),
                    Some(Ok(d)) => {
                        let rows = dim.with(|dm| hero_rows(&d, dm, &role.get(), sort.get()));
                        let nav = nav.clone();
                        view! {
                            {d.is_empty().then(|| view! { <div class="ds-empty">"No hero for this filter."</div> })}
                            <div class="ds-herowall">
                                {rows.into_iter().map(|h| {
                                    let p = hero_pct(&h);
                                    let cls = tone(p);
                                    let go = {
                                        let nav = nav.clone();
                                        let name = h.hero.clone();
                                        move || {
                                            let q = agg_to_search(&filter.get_untracked());
                                            let q = if q.is_empty() { String::new() } else { format!("?{q}") };
                                            nav(&format!("/hero/{}{q}", uri_component(&name)), Default::default());
                                        }
                                    };
                                    let go2 = go.clone();
                                    view! {
                                        <div class="ds-htile" role="link" tabindex="0" on:click=move |_| go()
                                            on:keydown=move |e| if e.key() == "Enter" { go2() }>
                                            <Portrait hero=h.hero.clone() size=64 />
                                            <strong>{h.hero.clone()}</strong>
                                            <span class=format!("wr {cls}")>{p}<i>"%"</i></span>
                                            <i class=bar_class(cls)><b style=format!("width: {p}%")></b></i>
                                            <em>{format!("{}/{} games", h.wins, h.games)}</em>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }
                }}
            </section>
        </div>
    }
}
