//! Maps: one tile per map (its art), your record, win rate, blue side, average length.
//! Port of `web/src/pages/Maps.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    aggregates::{bar_class, has_map_art, map_pct, map_rate, tone, AggFilter, MapStat},
    net,
    route::uri_component,
    session::fmt_dur,
};

use crate::ui::{fetch, map_art, AggFilterBar, Link, SecHead};

/// How often getting there first won the game (spec `docs/specs/2026-11-17-local-insights-design.md`, F2).
#[component]
fn EarlyLeadsBlock(filter: RwSignal<AggFilter>) -> impl IntoView {
    use storm_web_core::insights::{pct, EarlyLeads};
    let data = fetch::<EarlyLeads>(move || {
        format!("/api/maps/early-leads?{}", net::agg_params(&filter.get()))
    });
    move || {
        let d = match data.get()? {
            Ok(d) => d,
            Err(e) => return Some(view! { <section class="ds-sec"><div class="ds-empty">{format!("Could not load the early leads ({e}).")}</div></section> }.into_any()),
        };
        if d.games == 0 {
            return Some(view! { <section class="ds-sec"><div class="ds-empty">"No game for the early leads."</div></section> }.into_any());
        }
        let keys: Vec<(String, String)> = d
            .leads
            .iter()
            .map(|l| (l.key.clone(), l.label.clone()))
            .collect();
        let rows = d.leads.iter().map(|l| {
            let p = pct(l.wins, l.games);
            let mine = (l.my_games > 0).then(|| format!("{}% ({}–{})", pct(l.my_wins, l.my_games), l.my_wins, l.my_games - l.my_wins));
            view! {
                <tr>
                    <td>{format!("The team that got {}", l.label.to_lowercase())}</td>
                    <td class="ds-num">{l.games}</td>
                    <td class=format!("ds-num wr {}", tone(p as i64))><b>{format!("{p}%")}</b></td>
                    <td class="ds-num">{mine.unwrap_or_else(|| "—".into())}</td>
                </tr>
            }
        }).collect_view();
        let map_rows = d.maps.iter().map(|m| {
            let cells = keys.iter().map(|(k, _)| {
                match m.leads.iter().find(|l| &l.key == k) {
                    Some(l) => { let p = pct(l.wins, l.games); view! { <td class=format!("ds-num wr {}", tone(p as i64))>{format!("{p}%")}</td> }.into_any() }
                    None => view! { <td class="ds-num">"—"</td> }.into_any(),
                }
            }).collect_view();
            view! { <tr><td>{m.map.clone()}</td><td class="ds-num">{m.games}</td>{cells}</tr> }
        }).collect_view();
        let heads = keys
            .iter()
            .map(|(_, l)| view! { <th>{l.clone()}</th> })
            .collect_view();
        Some(view! {
            <section class="ds-sec">
                <SecHead num=1 title="Early leads" sub=format!("how often getting there first won · {} games", d.games) />
                <div class="ds-panel flush scroll">
                    <table class="ds-table">
                        <thead><tr><th>"Lead"</th><th>"Games"</th><th>"Won"</th><th>"When your team got it"</th></tr></thead>
                        <tbody>{rows}</tbody>
                    </table>
                </div>
                <div class="ds-panel flush scroll" style="margin-top: 12px">
                    <table class="ds-table">
                        <thead><tr><th>"Map"</th><th>"Games"</th>{heads}</tr></thead>
                        <tbody>{map_rows}</tbody>
                    </table>
                </div>
            </section>
        }.into_any())
    }
}

#[component]
pub fn Maps() -> impl IntoView {
    let filter = RwSignal::new(AggFilter::default());
    let data =
        fetch::<Vec<MapStat>>(move || format!("/api/maps?{}", net::agg_params(&filter.get())));
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Maps"</div>
                <h1 class="ds-title" style="cursor: default">"Battlegrounds"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || match data.get() {
                        Some(Ok(d)) => {
                            let (g, w) = d.iter().fold((0, 0), |(g, w), m| (g + m.my_games, w + m.my_wins));
                            view! {
                                {format!("{} maps", d.len())}
                                {(g > 0).then(|| view! {
                                    " · you "<b style="color: var(--win)">{w}</b>"–"<b style="color: var(--loss-soft)">{g - w}</b>{format!(" ({}%)", map_pct(w, g))}
                                })}
                            }.into_any()
                        }
                        _ => "loading…".into_any(),
                    }}
                    " — click a map to see its games."
                </div>
                <AggFilterBar value=filter mine_label="My games" />
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || match data.get() {
                    None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                    Some(Err(_)) => ().into_any(),
                    Some(Ok(d)) => view! {
                        {d.is_empty().then(|| view! { <div class="ds-empty">"No map for this filter."</div> })}
                        <div class="ds-mapts">
                            {d.into_iter().map(|m| {
                                let (mine, p) = map_rate(&m);
                                let t = if mine { tone(p) } else { "" };
                                let art = has_map_art(&m.map);
                                view! {
                                    <Link to=format!("/matches?map={}", uri_component(&m.map))
                                        class=if art { "ds-mapt" } else { "ds-mapt noart" }
                                        style=if art { map_art(&m.map) } else { String::new() }>
                                        <strong>{m.map.clone()}</strong>
                                        <div class="ds-mapt-f">
                                            <span class=format!("wr {t}")>{if mine { p.to_string() } else { "—".to_string() }}{mine.then(|| view! { <i>"%"</i> })}</span>
                                            <em>{if mine { format!("you {}–{}", m.my_wins, m.my_games - m.my_wins) } else { "you haven't played it".to_string() }}</em>
                                        </div>
                                        {mine.then(|| view! { <i class=bar_class(t)><b style=format!("width: {p}%")></b></i> })}
                                        <div class="meta">
                                            <span><b>{m.games}</b>" games"</span>
                                            <span>"blue wins "<b>{format!("{}%", map_pct(m.blue_wins, m.games))}</b></span>
                                            <span>"avg "<b>{fmt_dur(m.avg_length)}</b></span>
                                        </div>
                                    </Link>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any(),
                }}
            </section>
            <EarlyLeadsBlock filter=filter />
        </div>
    }
}
