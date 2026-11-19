//! Heroes: the hero wall over the filtered set: portrait, games, win rate (coloured gauge).
//! Port of `web/src/pages/Heroes.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    aggregates::{
        agg_to_search, bar_class, hero_pct, hero_roles, hero_rows, tone, AggFilter, HeroSort,
        HeroStat,
    },
    net,
    route::uri_component,
};

use crate::ui::{fetch_operator_stats, use_dim, AggFilterBar, Link, Portrait};

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
    let dim = use_dim();
    let data = fetch_operator_stats::<Vec<HeroStat>>(move || {
        format!("/api/heroes?{}", net::agg_params(&filter.get()))
    });
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
                    <button type="button" class=move || pill(sort.get() == HeroSort::Games) on:click=move |_| sort.set(HeroSort::Games)>"Most played"</button>
                    <button type="button" class=move || pill(sort.get() == HeroSort::WinRate) on:click=move |_| sort.set(HeroSort::WinRate)>"Win rate"</button>
                    {move || (!roles().is_empty()).then(|| view! { <span class="ds-sep"></span> })}
                    {move || (!roles().is_empty()).then(|| view! {
                        <button type="button" class=move || pill(role.with(String::is_empty)) on:click=move |_| role.set(String::new())>"All roles"</button>
                    })}
                    {move || roles().into_iter().map(|r| {
                        let r2 = r.clone();
                        let r3 = r.clone();
                        view! { <button type="button" class=move || pill(role.with(|c| *c == r2)) on:click=move |_| role.set(r3.clone())>{r}</button> }
                    }).collect_view()}
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || match data.get() {
                    None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                    Some(Err(_)) => ().into_any(),
                    Some(Ok(d)) => {
                        let rows = dim.with(|dm| hero_rows(&d, dm, &role.get(), sort.get()));
                        view! {
                            {d.is_empty().then(|| view! { <div class="ds-empty">"No hero for this filter."</div> })}
                            <div class="ds-herowall">
                                {rows.into_iter().enumerate().map(|(i, h)| {
                                    let p = hero_pct(&h);
                                    let cls = tone(p);
                                    let name = h.hero.clone();
                                    let q = agg_to_search(&filter.get());
                                    let q = if q.is_empty() { String::new() } else { format!("?{q}") };
                                    let href = format!("/hero/{}{q}", uri_component(&name));
                                    view! {
                                        <Link to=href class="ds-htile">
                                            <Portrait hero=h.hero.clone() size=64 eager=i < 8 captioned=true />
                                            <strong>{h.hero.clone()}</strong>
                                            <span class=format!("wr {cls}")>{p}<i>"%"</i></span>
                                            <i class=bar_class(cls)><b style=format!("width: {p}%")></b></i>
                                            <em>{format!("{}/{} games", h.wins, h.games)}</em>
                                        </Link>
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
