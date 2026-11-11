//! Synergies: from the operator's perspective, recurring teammates and the enemy heroes faced.
//! Port of `web/src/pages/Synergies.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    players::{name_initials, pct, sort_synergies, wr_tone, Synergies as SynergyData, SynergySort},
    route::uri_component,
};

use crate::ui::{fetch, Link, Portrait, SecHead, Tone};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

#[component]
pub fn Synergies() -> impl IntoView {
    let data = fetch::<SynergyData>(|| "/api/synergies".to_string());
    let sort = RwSignal::new(SynergySort::WinRate);
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Synergies"</div>
                <h1 class="ds-title" style="cursor: default">"With & against"</h1>
                <div class="ds-subtitle" style="cursor: default">"From your perspective, across all your accounts — allies you played 3+ games with, enemy heroes you faced 3+ times."</div>
                <div class="ds-filterbar">
                    <span class="ds-label" style="margin: 0">"Sort"</span>
                    <span class=move || pill(sort.get() == SynergySort::WinRate) on:click=move |_| sort.set(SynergySort::WinRate)>"Win rate"</span>
                    <span class=move || pill(sort.get() == SynergySort::Games) on:click=move |_| sort.set(SynergySort::Games)>"Games"</span>
                </div>
            </header>
            {move || match data.get() {
                None => view! { <div class="ds-empty" style="margin-top: 20px">"loading…"</div> }.into_any(),
                Some(Err(_)) => ().into_any(),
                Some(Ok(d)) => {
                    let max_t = d.teammates.iter().map(|t| t.games).max().unwrap_or(1).max(1);
                    let max_e = d.enemies.iter().map(|e| e.games).max().unwrap_or(1).max(1);
                    let by = sort.get();
                    let teammates = sort_synergies(&d.teammates, by, |t| (t.games, t.wins));
                    let enemies = sort_synergies(&d.enemies, by, |e| (e.games, e.wins));
                    view! {
                        <div class="ds-grid2" style="margin-top: 10px">
                            <section class="ds-sec" style="margin-top: 30px">
                                <SecHead num=1 title="Teammates" sub=format!("{} recurring", d.teammates.len()) />
                                <div class="ds-panel">
                                    <div class="ds-rows">
                                        {teammates.into_iter().map(|t| {
                                            let p = pct(t.wins, t.games);
                                            view! {
                                                <div class="ds-srow">
                                                    <span class="ds-initial">{name_initials(&t.name)}</span>
                                                    <span class="nm">{t.name.clone()}</span>
                                                    <i class="ds-bar"><b style=format!("width: {}%", ((t.games * 100) as f64 / max_t as f64).round())></b></i>
                                                    <span class=format!("wr {}", wr_tone(p))>{format!("{p}%")}</span>
                                                    <em>{format!("{}–{}", t.wins, t.games - t.wins)}</em>
                                                </div>
                                            }
                                        }).collect_view()}
                                        {d.teammates.is_empty().then(|| view! { <div class="ds-empty-row">"no recurring teammates"</div> })}
                                    </div>
                                </div>
                            </section>
                            <section class="ds-sec" style="margin-top: 30px">
                                <SecHead num=2 title="Against heroes" sub="your win rate when they are in front" />
                                <div class="ds-panel">
                                    <div class="ds-rows">
                                        {enemies.into_iter().map(|e| {
                                            let p = pct(e.wins, e.games);
                                            view! {
                                                <Link to=format!("/hero/{}", uri_component(&e.hero)) class="ds-srow">
                                                    <Portrait hero=e.hero.clone() size=34 tone=if p < 45 { Tone::Red } else { Tone::Plain } />
                                                    <span class="nm">{e.hero.clone()}</span>
                                                    <i class="ds-bar"><b style=format!("width: {}%", ((e.games * 100) as f64 / max_e as f64).round())></b></i>
                                                    <span class=format!("wr {}", wr_tone(p))>{format!("{p}%")}</span>
                                                    <em>{format!("{}–{}", e.wins, e.games - e.wins)}</em>
                                                </Link>
                                            }
                                        }).collect_view()}
                                        {d.enemies.is_empty().then(|| view! { <div class="ds-empty-row">"not enough data"</div> })}
                                    </div>
                                </div>
                            </section>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
