//! Records: the best single games per stat, from every game in the corpus (spec
//! `docs/specs/2026-11-17-local-insights-design.md`, F1).

use leptos::prelude::*;
use storm_web_core::{
    aggregates::{MapStat, MODES},
    insights::{records_query, short_num, Records},
    net,
    route::uri_component,
};

use crate::ui::{fetch_operator_stats, pill_class, Link, Portrait, SecHead};

#[component]
pub fn RecordsPage() -> impl IntoView {
    let mode = RwSignal::new(None::<i64>);
    let mine = RwSignal::new(false);
    let map = RwSignal::new(String::new());
    let hero = RwSignal::new(String::new());
    let hero_txt = RwSignal::new(String::new());
    let maps = fetch_operator_stats::<Vec<MapStat>>(|| "/api/maps".to_string());
    let data = fetch_operator_stats::<Records>(move || {
        format!(
            "/api/records?{}",
            records_query(mode.get(), Some(&map.get()), Some(&hero.get()), mine.get())
        )
    });
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Records"</div>
                <h1 class="ds-title" style="cursor: default">"The best single games"</h1>
                <div class="ds-subtitle" style="cursor: default">"Top 5 per stat across every game here. Click a game to open it."</div>
                <div class="ds-filterbar">
                    {MODES.iter().map(|&(label, m)| view! {
                        <button type="button" class=move || pill_class(mode.get() == m) on:click=move |_| mode.set(m)>{label}</button>
                    }).collect_view()}
                    <span class="ds-sep"></span>
                    <button type="button" class=move || pill_class(mine.get()) on:click=move |_| mine.update(|v| *v = !*v)>"My games only"</button>
                    <select class="ds-input" aria-label="Battleground" on:change=move |e| map.set(event_target_value(&e))>
                        <option value="" selected=move || map.get().is_empty()>"All maps"</option>
                        {move || maps.get().and_then(Result::ok).unwrap_or_default().into_iter().map(|m| {
                            let (v, l, sel) = (m.map.clone(), m.map.clone(), m.map.clone());
                            view! { <option value=v selected=move || map.get() == sel>{l}</option> }
                        }).collect_view()}
                    </select>
                    <input class="ds-input" placeholder="hero, then Enter" prop:value=move || hero_txt.get()
                        on:input=move |e| hero_txt.set(event_target_value(&e))
                        on:keydown=move |e| if e.key() == "Enter" { hero.set(hero_txt.get_untracked().trim().to_string()) } />
                </div>
            </header>
            {move || match data.get() {
                None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                Some(Err(e)) => view! { <div class="ds-empty">{format!("Could not load the records ({e}).")}</div> }.into_any(),
                Some(Ok(r)) if r.stats.iter().all(|s| s.rows.is_empty()) => view! { <div class="ds-empty">"No game for this filter."</div> }.into_any(),
                Some(Ok(r)) => r.stats.into_iter().filter(|s| !s.rows.is_empty()).enumerate().map(|(i, st)| {
                    let rows = st.rows.into_iter().enumerate().map(|(k, row)| {
                        let to = format!("/match/{}", row.match_id);
                        let who = row.name.clone().unwrap_or_else(|| row.toon.clone());
                        view! {
                            <tr class=if row.mine { "ds-me" } else { "" }>
                                <td class="ds-num">{k + 1}</td>
                                <td class="ds-num"><b>{short_num(row.value)}</b></td>
                                <td><Link to=format!("/player/{}", uri_component(&row.toon))>{who}</Link></td>
                                <td><span class="ds-who" style="min-width: 0">
                                    {row.hero.clone().map(|h| view! { <Portrait hero=h size=24 /> })}
                                    <span>{row.hero.clone().unwrap_or_default()}</span>
                                </span></td>
                                <td><Link to=to>{format!("{} · {}", row.map.clone().unwrap_or_default(), net::fmt_time(row.played_at.as_deref()))}</Link></td>
                            </tr>
                        }
                    }).collect_view();
                    view! {
                        <section class="ds-sec">
                            <SecHead num={i as u32 + 1} title=st.label.clone() />
                            <div class="ds-panel flush scroll">
                                <table class="ds-table">
                                    <thead><tr><th>"#"</th><th>"Value"</th><th>"Player"</th><th>"Hero"</th><th>"Game"</th></tr></thead>
                                    <tbody>{rows}</tbody>
                                </table>
                            </div>
                        </section>
                    }
                }).collect_view().into_any(),
            }}
        </div>
    }
}
