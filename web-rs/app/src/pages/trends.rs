//! Trends: one HotS build is one patch: how the operator did on it, how much was played, blue-side win rate and
//! game length. Port of `web/src/pages/Trends.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    aggregates::{bar_class, tone},
    net,
    players::pct,
    session::fmt_dur,
    trends::{bar_height, best_patch, chrono, mine, Trend},
};

use crate::ui::{fetch, SecHead};

#[component]
pub fn Trends() -> impl IntoView {
    let data = fetch::<Vec<Trend>>(|| "/api/trends".to_string());
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Trends"</div>
                <h1 class="ds-title" style="cursor: default">"Patch by patch"</h1>
                <div class="ds-subtitle" style="cursor: default">"Each HotS game build is a patch: how you did on it, how much was played, blue-side win rate and game length."</div>
                {move || data.get().and_then(Result::ok).map(|d| {
                    let best = best_patch(&d).map(|b| view! {
                        <div class="ds-stat"><span class="ds-label">"Your best patch · 5+ games"</span><strong>{b.build}</strong>
                            <small>{format!("{}/{} · {}%", b.my_wins, b.my_games, pct(b.my_wins, b.my_games))}</small></div>
                    });
                    view! {
                        <div class="ds-statline">
                            <div class="ds-stat"><span class="ds-label">"Patches"</span><strong>{d.len()}</strong><small>{format!("{} with your games", mine(&d).len())}</small></div>
                            {best}
                        </div>
                    }
                })}
            </header>

            {move || match data.get() {
                None => view! { <div class="ds-empty" style="margin-top: 20px">"loading…"</div> }.into_any(),
                Some(Err(_)) => ().into_any(),
                Some(Ok(d)) if d.is_empty() => view! { <div class="ds-empty" style="margin-top: 20px">"No data."</div> }.into_any(),
                Some(Ok(d)) => {
                    let chart = chrono(&d);
                    let sub = format!("last {} patches · dashed line = 50%", chart.len());
                    let cols = chart.iter().map(|t| {
                        let p = pct(t.my_wins, t.my_games);
                        let title = format!("{} · {}", t.build, if t.my_games > 0 { format!("{}/{} ({p}%)", t.my_wins, t.my_games) } else { "no game of yours".into() });
                        let bar = format!("bar {}", if t.my_games > 0 { tone(p) } else { "none" });
                        view! {
                            <div class="col" title=title>
                                <span class="v">{if t.my_games > 0 { format!("{p}%") } else { String::new() }}</span>
                                <div class=bar style=format!("height: {}", bar_height(t.my_games, p))></div>
                            </div>
                        }
                    }).collect_view();
                    let axis = chart.iter().map(|t| view! { <span title=net::fmt_date_short(t.first_seen.as_deref())>{t.build}</span> }).collect_view();
                    let rows = d.iter().map(|t| {
                        let p = pct(t.my_wins, t.my_games);
                        let (first, last) = (net::fmt_date_short(t.first_seen.as_deref()), net::fmt_date_short(t.last_seen.as_deref()));
                        let played = if first == last { last } else { format!("{first} – {last}") };
                        let you = if t.my_games > 0 {
                            view! { <><span style="color: var(--win)">{t.my_wins}</span>"–"<span style="color: var(--loss-soft)">{t.my_games - t.my_wins}</span></> }.into_any()
                        } else {
                            "—".into_any()
                        };
                        let rate = if t.my_games > 0 {
                            view! { <div class="ds-cell"><span>{format!("{p}%")}</span><i class=format!("ds-bar {}", bar_class(tone(p)))><b style=format!("width: {p}%")></b></i></div> }.into_any()
                        } else {
                            view! { <span class="ds-num">"—"</span> }.into_any()
                        };
                        view! {
                            <tr>
                                <td class="ds-num" style="color: var(--text)">{t.build}</td>
                                <td class="ds-num">{played}</td>
                                <td class="ds-num">{you}</td>
                                <td>{rate}</td>
                                <td class="ds-num">{t.games}</td>
                                <td class="ds-num ds-us">{format!("{}%", pct(t.blue_wins, t.games))}</td>
                                <td class="ds-num">{fmt_dur(t.avg_length)}</td>
                            </tr>
                        }
                    }).collect_view();
                    view! {
                        <section class="ds-sec">
                            <SecHead num=1 title="Your win rate" sub=sub />
                            <div class="ds-panel">
                                <div class="ds-trend">
                                    <div class="mid"></div>
                                    {cols}
                                </div>
                                <div class="ds-trend-x">{axis}</div>
                            </div>
                        </section>
                        <section class="ds-sec">
                            <SecHead num=2 title="Detail" />
                            <div class="ds-panel flush scroll">
                                <table class="ds-table">
                                    <thead><tr><th>"Patch (build)"</th><th>"Played"</th><th>"You"</th><th>"Your win %"</th><th>"Games"</th><th>"Blue win"</th><th>"Avg length"</th></tr></thead>
                                    <tbody>{rows}</tbody>
                                </table>
                            </div>
                        </section>
                    }.into_any()
                }
            }}
        </div>
    }
}
