//! Match page sections of the local insights (spec `docs/specs/2026-11-17-local-insights-design.md`):
//! team fights (F3), the time window A → B (F4) and camps (F5). Data: `/api/matches/{id}/insights`.

use std::collections::HashMap;

use leptos::prelude::*;
use leptos_router::{
    hooks::{use_location, use_navigate},
    NavigateOptions,
};
use storm_web_core::{
    insights::{camp_table, clock, parse_clock, short_num, MatchInsights},
    route::query_value,
};

use crate::ui::{fetch, Portrait, SecHead};

/// A player of the match, as the sections need it.
#[derive(Clone)]
pub struct Who {
    pub name: String,
    pub hero: String,
    pub team: u8,
}

fn team_name(my_team: Option<i64>, t: u8) -> &'static str {
    match my_team {
        Some(m) if m == t as i64 => "Your team",
        Some(_) => "Enemy",
        None if t == 0 => "Blue",
        None => "Red",
    }
}

fn team_cls(t: u8) -> &'static str {
    if t == 0 {
        "ds-us"
    } else {
        "ds-them"
    }
}

#[component]
pub fn MatchInsightsSections(
    id: i64,
    my_team: Option<i64>,
    who: HashMap<String, Who>,
    first: u32,
) -> impl IntoView {
    let loc = use_location();
    let nav = StoredValue::new_local(use_navigate());
    let bound =
        move |k: &str| query_value(&loc.search.get(), k).and_then(|v| v.parse::<f64>().ok());
    let from = Signal::derive(move || bound("from"));
    let to = Signal::derive(move || bound("to"));
    let set_window = move |a: Option<f64>, b: Option<f64>| {
        let q = match (a, b) {
            (Some(a), Some(b)) => format!("?from={}&to={}", a.round(), b.round()),
            _ => String::new(),
        };
        let path = format!("/match/{id}{q}");
        nav.with_value(|n| {
            n(
                &path,
                NavigateOptions {
                    replace: true,
                    scroll: false,
                    ..Default::default()
                },
            )
        });
    };
    let data = fetch::<MatchInsights>(move || match (from.get(), to.get()) {
        (Some(a), Some(b)) => format!("/api/matches/{id}/insights?from={a}&to={b}"),
        _ => format!("/api/matches/{id}/insights"),
    });
    let a_txt = RwSignal::new(String::new());
    let b_txt = RwSignal::new(String::new());
    Effect::new(move |_| {
        a_txt.set(from.get().map(clock).unwrap_or_default());
        b_txt.set(to.get().map(clock).unwrap_or_default());
    });
    let who = StoredValue::new(who);
    // order: the operator's team first, else blue first
    let order: [u8; 2] = if my_team == Some(1) { [1, 0] } else { [0, 1] };

    move || {
        let d = match data.get() {
            None => return view! { <section class="ds-sec"><div class="ds-empty">"loading fights…"</div></section> }.into_any(),
            Some(Err(e)) => return view! {
                <section class="ds-sec"><div class="ds-empty">{format!("Could not read the fights ({e}). ")}
                    <span class="ds-pill" on:click=move |_| set_window(None, None)>"reset the window"</span></div></section>
            }.into_any(),
            Some(Ok(d)) => d,
        };
        let fights = d.fights.clone();
        let won = |t: u8| fights.iter().filter(|f| f.winner == Some(t)).count();
        let me = order[0];
        let summary = format!(
            "{} won {} of {} · {} deaths outside fights",
            team_name(my_team, me),
            won(me),
            fights.len(),
            d.isolated_deaths.len()
        );
        let fight_rows = fights.iter().map(|f| {
            let (a, b) = (f.start, f.end);
            let result = match f.winner {
                Some(t) => view! { <span class=team_cls(t)>{format!("{} won", team_name(my_team, t))}</span> }.into_any(),
                None => view! { <span>"even"</span> }.into_any(),
            };
            let mut then = Vec::new();
            for t in order {
                let i = t as usize;
                let n = team_name(my_team, t);
                if f.structures_after[i] > 0 { then.push(format!("{n}: {} building(s)", f.structures_after[i])); }
                if f.camps_after[i] > 0 { then.push(format!("{n}: {} camp(s)", f.camps_after[i])); }
                if f.objectives_after[i] > 0 { then.push(format!("{n}: objective")); }
            }
            let victims = f.deaths.iter().map(|x| {
                let h = x.victim_hero.clone();
                view! { <Portrait hero=h size=22 /> }
            }).collect_view();
            view! {
                <tr style="cursor: pointer" on:click=move |_| set_window(Some(a - 2.0), Some(b + 2.0)) title="Show this fight in the time window">
                    <td class="ds-num">{format!("{}–{}", clock(a), clock(b))}</td>
                    <td class="ds-num">{f.lost[order[0] as usize]}</td>
                    <td class="ds-num">{f.lost[order[1] as usize]}</td>
                    <td>{victims}</td>
                    <td>{result}</td>
                    <td>{then.join(" · ")}</td>
                </tr>
            }
        }).collect_view();

        let window = d.window.clone().map(|w| {
            let mut ps: Vec<_> = w.players.clone();
            who.with_value(|m| ps.sort_by_key(|p| m.get(&p.toon).map_or(9, |x| if Some(x.team as i64) == my_team { 0 } else { 1 + x.team })));
            let player_rows = ps.into_iter().map(|p| {
                let x = who.with_value(|m| m.get(&p.toon).cloned());
                let (name, hero, team) = x.map_or((p.toon.clone(), String::new(), 0), |x| (x.name, x.hero, x.team));
                let tf = d.teamfight_totals.iter().find(|t| t.toon == p.toon).cloned();
                let f = |v: Option<f64>| v.map_or("—".into(), short_num);
                view! {
                    <tr>
                        <td><span class="ds-who" style="min-width: 0"><Portrait hero=hero.clone() size=24 /><span class=team_cls(team)>{name}</span></span></td>
                        <td class="ds-num">{p.takedowns}</td>
                        <td class="ds-num">{p.deaths}</td>
                        <td class="ds-num">{f(tf.as_ref().and_then(|t| t.hero_damage))}</td>
                        <td class="ds-num">{f(tf.as_ref().and_then(|t| t.damage_taken))}</td>
                        <td class="ds-num">{f(tf.as_ref().and_then(|t| t.healing))}</td>
                    </tr>
                }
            }).collect_view();
            let team_rows = order.map(|t| {
                let tw = w.teams[t as usize].clone();
                view! {
                    <tr>
                        <td class=team_cls(t)>{team_name(my_team, t)}</td>
                        <td class="ds-num">{format!("{} → {}", tw.level_from, tw.level_to)}</td>
                        <td class="ds-num">{tw.xp.map_or("—".into(), short_num)}</td>
                        <td class="ds-num">{tw.structures}</td>
                        <td class="ds-num">{tw.camps}</td>
                        <td class="ds-num">{tw.objectives}</td>
                    </tr>
                }
            }).collect_view();
            let xp_note = match (w.xp_from, w.xp_to) {
                (Some(a), Some(b)) => format!("XP is sampled every minute: counted from {} to {}.", clock(a), clock(b)),
                _ => "No XP samples for this window.".into(),
            };
            view! {
                <div class="ds-panel flush scroll">
                    <table class="ds-table">
                        <thead><tr><th>"Team"</th><th>"Level"</th><th>"XP gained"</th><th>"Buildings"</th><th>"Camps"</th><th>"Objectives"</th></tr></thead>
                        <tbody>{team_rows}</tbody>
                    </table>
                </div>
                <div class="ds-panel flush scroll" style="margin-top: 12px">
                    <table class="ds-table">
                        <thead><tr><th>"Player"</th><th>"Takedowns"</th><th>"Deaths"</th>
                            <th title="whole game, not this window">"Teamfight dmg*"</th><th title="whole game, not this window">"Teamfight taken*"</th><th title="whole game, not this window">"Teamfight heal*"</th></tr></thead>
                        <tbody>{player_rows}</tbody>
                    </table>
                </div>
                <div class="ds-subtitle" style="cursor: default; margin-top: 8px">{xp_note}" * whole game: the replay only stores damage and healing as end-of-game totals, so they can't be cut to a window."</div>
            }
        });

        let camps = camp_table(&d.camps);
        let camp_rows = camps.into_iter().filter(|(_, c)| c[0] + c[1] > 0).map(|(label, c)| view! {
            <tr><td>{label}</td><td class="ds-num">{c[order[0] as usize]}</td><td class="ds-num">{c[order[1] as usize]}</td></tr>
        }).collect_view();
        let n_camps = d.camps.len();
        let camp_sub = format!("{n_camps} camps · timing only, captures carry no position");

        view! {
            <section class="ds-sec">
                <SecHead num=first title="Team fights" sub=summary />
                {if fights.is_empty() {
                    view! { <div class="ds-empty">"No team fight (two deaths or more, close in time and place)."</div> }.into_any()
                } else {
                    view! {
                        <div class="ds-panel flush scroll">
                            <table class="ds-table">
                                <thead><tr><th>"When"</th><th class=team_cls(order[0])>{format!("{} lost", team_name(my_team, order[0]))}</th><th class=team_cls(order[1])>{format!("{} lost", team_name(my_team, order[1]))}</th><th>"Deaths"</th><th>"Result"</th><th>"Within 30 s"</th></tr></thead>
                                <tbody>{fight_rows}</tbody>
                            </table>
                        </div>
                    }.into_any()
                }}
            </section>
            <section class="ds-sec">
                <SecHead num=first + 1 title="Time window" sub="click a fight, or pick a range" />
                <div class="ds-filterbar">
                    <label>"from "<input class="ds-input" style="width: 70px" placeholder="m:ss" prop:value=move || a_txt.get() on:input=move |e| a_txt.set(event_target_value(&e)) /></label>
                    <label>"to "<input class="ds-input" style="width: 70px" placeholder="m:ss" prop:value=move || b_txt.get() on:input=move |e| b_txt.set(event_target_value(&e)) /></label>
                    <span class="ds-pill" on:click=move |_| {
                        if let (Some(a), Some(b)) = (parse_clock(&a_txt.get_untracked()), parse_clock(&b_txt.get_untracked())) {
                            set_window(Some(a), Some(b));
                        }
                    }>"Show"</span>
                    <Show when=move || from.get().is_some()>
                        <span class="ds-pill" on:click=move |_| set_window(None, None)>"✕ clear"</span>
                    </Show>
                </div>
                {match window {
                    Some(w) => w.into_any(),
                    None => view! { <div class="ds-empty">"No window selected."</div> }.into_any(),
                }}
            </section>
            <section class="ds-sec">
                <SecHead num=first + 2 title="Camps" sub=camp_sub />
                {if n_camps == 0 {
                    view! { <div class="ds-empty">"No camp taken."</div> }.into_any()
                } else {
                    view! {
                        <div class="ds-panel flush scroll">
                            <table class="ds-table">
                                <thead><tr><th>"Why"</th><th class=team_cls(order[0])>{team_name(my_team, order[0])}</th><th class=team_cls(order[1])>{team_name(my_team, order[1])}</th></tr></thead>
                                <tbody>{camp_rows}</tbody>
                            </table>
                        </div>
                    }.into_any()
                }}
            </section>
        }.into_any()
    }
}
