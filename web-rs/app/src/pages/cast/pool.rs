//! Map pool of the series: ban / pick / decider / reset per map, game start, winner. Port of `PoolPanel.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    castctl::{can_change_winner, can_remove_game, can_set_winner},
    logic::{map_card_view, map_image, startable_maps},
    model::{CastSeries, MapStatus, Side},
    net,
};

use super::api::{self, Runner};

/// `(label, status, by, tone)`
const MARKS: [(&str, &str, Option<Side>, &str); 6] = [
    ("Ban Blue", "banned", Some(Side::Blue), "blue"),
    ("Ban Red", "banned", Some(Side::Red), "red"),
    ("Pick Blue", "picked", Some(Side::Blue), "blue"),
    ("Pick Red", "picked", Some(Side::Red), "red"),
    ("Decider", "decider", None, "gold"),
    ("Reset", "available", None, ""),
];

fn status_id(s: MapStatus) -> &'static str {
    match s {
        MapStatus::Available => "available",
        MapStatus::Banned => "banned",
        MapStatus::Picked => "picked",
        MapStatus::Decider => "decider",
    }
}

#[component]
pub fn PoolPanel(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let pick = RwSignal::new(String::new());
    let startable = Memo::new(move |_| {
        s.with(|x| {
            startable_maps(x)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
    });
    let chosen = move || {
        let p = pick.get();
        let st = startable.get();
        if st.contains(&p) {
            p
        } else {
            st.first().cloned().unwrap_or_default()
        }
    };
    let names =
        Memo::new(move |_| s.with(|x| x.pool.iter().map(|m| m.name.clone()).collect::<Vec<_>>()));
    let nums = Memo::new(move |_| s.with(|x| x.games.iter().map(|g| g.number).collect::<Vec<_>>()));
    view! {
        <section class="cr-box cr-pool">
            <div class="cr-k">"Maps"</div>
            {move || names.with(Vec::is_empty).then(|| view! { <div class="cr-note">"Empty pool — start a series with at least one map."</div> })}
            <div class="cr-mapgrid">
                {move || names.get().into_iter().map(|name| {
                    let pm = {
                        let name = name.clone();
                        move || s.with(|x| x.pool.iter().find(|m| m.name == name).cloned())
                    };
                    let view_of = {
                        let pm = pm.clone();
                        move || {
                            let x = s.get();
                            let next = startable_maps(&x).first().map(|m| m.to_string());
                            pm().map(|m| map_card_view(&m, &x.games, next.as_deref(), ("Blue", "Red")))
                        }
                    };
                    let played = {
                        let name = name.clone();
                        move || s.with(|x| x.games.iter().any(|g| g.map == name))
                    };
                    let (v1, v2) = (view_of.clone(), view_of);
                    let img = map_image(&name);
                    let n2 = name.clone();
                    view! {
                        <div class=move || { let v = v1(); format!("cr-map {}{}", v.as_ref().map(|v| v.tone.class()).unwrap_or(""), if v.is_some_and(|v| v.struck) { " struck" } else { "" }) }>
                            <div class="cr-map-art" style=format!("background-image: url({img})")></div>
                            <div class="cr-map-h"><b>{name.clone()}</b><em>{move || v2().map(|v| v.label).unwrap_or_default()}</em></div>
                            <div class="cr-map-btns">
                                {MARKS.iter().map(|(label, status, by, tone)| {
                                    let (label, status, by, tone) = (*label, *status, *by, *tone);
                                    let (pm, n3, played) = (pm.clone(), n2.clone(), played.clone());
                                    let active = std::sync::Arc::new(move || pm().is_some_and(|m| status_id(m.status) == status && m.by == by));
                                    let (active_c, active_p) = (active.clone(), active);
                                    view! {
                                        <button type="button" class=move || format!("ds-btn small {tone}{}", if active_c() { " on" } else { "" })
                                            aria-label=format!("{label} {n2}")
                                            aria-pressed=move || (status != "available").then(|| active_p().to_string())
                                            disabled=played
                                            on:click=move |_| { let n = n3.clone(); run.run(async move { api::mark_map(&n, status, by).await }) }>{label}</button>
                                    }
                                }).collect_view()}
                            </div>
                        </div>
                    }
                }).collect_view()}
            </div>
            <div class="cr-start">
                <select class="ds-input" aria-label="Map to start" disabled=move || startable.with(Vec::is_empty)
                    on:change=move |e| pick.set(event_target_value(&e))>
                    {move || startable.with(Vec::is_empty).then(|| view! { <option value="">"No picked / decider map left"</option> })}
                    {move || startable.get().into_iter().map(|n| { let (n2, c) = (n.clone(), chosen()); view! { <option selected=c == n2>{n}</option> } }).collect_view()}
                </select>
                <button type="button" class="ds-btn primary" disabled=move || chosen().is_empty()
                    on:click=move |_| { let m = chosen(); run.run(async move { api::start_game(&m).await }) }>"Start game"</button>
            </div>
            {move || (!nums.with(Vec::is_empty)).then(|| view! {
                <div class="cr-games">
                    {move || nums.get().into_iter().map(|num| {
                        let game = move || s.with(|x| x.games.iter().find(|g| g.number == num).cloned());
                        view! { {move || game().map(|g| {
                            let w = g.winner;
                            view! {
                                <div class="cr-game">
                                    <span class="cr-gn">{format!("Game {num}")}</span>
                                    <span class="cr-gm">{g.map.clone()}</span>
                                    {match w {
                                        Some(side) => {
                                            let (cls, text, other) = if side == Side::Blue { ("blue", "Blue won", Side::Red) } else { ("red", "Red won", Side::Blue) };
                                            let other_name = if other == Side::Blue { "Blue" } else { "Red" };
                                            view! {
                                                <span class=format!("cr-win tone-{cls}")>{text}</span>
                                                {move || s.with(|x| can_change_winner(x, num)).then(|| view! {
                                                    <button type="button" class="ds-btn small" aria-label=format!("Game {num}: change winner to {other_name}")
                                                        title="Misclick? Give the win to the other side (not possible once a replay is loaded)"
                                                        on:click=move |_| run.run(api::winner(num, other))>"Change winner"</button>
                                                })}
                                            }.into_any()
                                        }
                                        None => [Side::Blue, Side::Red].into_iter().map(|side| {
                                            let (cls, text) = if side == Side::Blue { ("blue", "Blue") } else { ("red", "Red") };
                                            view! {
                                                <button type="button" class=format!("ds-btn small {cls}") aria-label=format!("Game {num}: {text} won")
                                                    disabled=move || !s.with(|x| can_set_winner(x, num))
                                                    on:click=move |_| run.run(api::winner(num, side))>{format!("{text} won")}</button>
                                            }
                                        }).collect_view().into_any(),
                                    }}
                                    {let map = g.map.clone(); move || s.with(|x| can_remove_game(x, num)).then({ let map = map.clone(); move || view! {
                                        <button type="button" class="ds-btn small" aria-label=format!("Remove game {num}")
                                            title="Started by mistake? Remove this game; its map becomes startable again"
                                            on:click=move |_| { if net::confirm(&format!("Remove game {num} ({map})?")) { run.run(api::remove_game(num)) } }>"Remove game"</button>
                                    }})}
                                </div>
                            }
                        })} }
                    }).collect_view()}
                </div>
            })}
        </section>
    }
}
