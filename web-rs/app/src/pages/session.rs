//! Session: the last evening of the operator as the cover, its games, then the recent form.
//! Port of `web/src/pages/Dashboard.tsx` (logic in `core::dashboard`).

use leptos::prelude::*;
use storm_web_core::{
    dashboard::{
        by_hero, by_mode, games_label, kda, last_session, mode_fam, mode_name, my_games, pct_of,
        verdict,
    },
    net,
    route::uri_component,
    session::{fmt_dur, to_fixed1, MatchSummary},
};

use crate::{
    layout::LiveBus,
    matchui::{AwardTag, GameCard, Pips},
    ui::{fetch, map_art, use_operator, Link, Portrait, SecHead, Tone},
};

#[component]
pub fn Session() -> impl IntoView {
    let operator = use_operator();
    let bus = expect_context::<LiveBus>();
    let matches = fetch::<Vec<MatchSummary>>(move || {
        bus.matches.track(); // a parsed match re-reads the list
        "/api/matches?limit=500".to_string()
    });
    view! {
        <div class="ds-page">
            {move || {
                let Some(res) = matches.get() else {
                    return view! { <div class="ds-empty" style="margin-top: 24px">"loading…"</div> }.into_any();
                };
                let list = res.unwrap_or_default();
                let names = operator.get();
                let configured = !names.is_empty();
                let games = my_games(&list, &names);
                if !configured || games.is_empty() {
                    let sub = if configured {
                        "None of your accounts appears in the archive yet — upload replays with the uploader."
                    } else {
                        "Tell Storm Codex which accounts are yours in Admin → operator names."
                    };
                    return view! {
                        <header class="ds-cover" style="margin-top: 18px">
                            <div class="ds-kicker">"Session"</div>
                            <h1 class="ds-title" style="cursor: default">"No session yet"</h1>
                            <div class="ds-subtitle" style="cursor: default">{sub}</div>
                            <div class="ds-cover-foot">
                                <Link class="ds-btn primary" to="/admin">"Open Admin"</Link>
                                <Link class="ds-btn" to="/matches">"Browse matches"</Link>
                            </div>
                        </header>
                    }.into_any();
                }
                let session = last_session(&games, net::ts);
                let n = session.len();
                let wins = session.iter().filter(|g| g.won).count();
                let (Some(first), Some(last)) = (session.first().cloned(), session.last().cloned()) else {
                    return ().into_any();
                };
                let played: f64 = session.iter().map(|g| g.m.length.unwrap_or(0.0)).sum();
                let (tk, td, ta) = session.iter().fold((0, 0, 0), |(k, d, a), g| {
                    let x = kda(g.me);
                    (k + x.0, d + x.1, a + x.2)
                });
                let mvps = session.iter().filter(|g| g.me.award.as_deref().unwrap_or("").contains("MVP")).count();
                let mut heroes: Vec<String> = Vec::new();
                for h in session.iter().filter_map(|g| g.me.hero.clone()).filter(|h| !h.is_empty()) {
                    if !heroes.contains(&h) {
                        heroes.push(h);
                    }
                }
                let first_at = first.m.played_at.clone().unwrap_or_default();
                let last_at = last.m.played_at.clone().unwrap_or_default();
                let is_today = !last_at.is_empty() && net::local_day(&last_at) == net::today_local();
                let form: Vec<bool> = games.iter().map(|g| g.won).collect();
                let form_wins = form.iter().filter(|w| **w).count();
                let heroes6: Vec<_> = by_hero(&games).into_iter().take(6).collect();
                let max_hero = heroes6.first().map_or(1, |h| h.games);
                let modes = by_mode(&games, mode_name);
                let last_won = last.won;
                let cover_style = format!("margin-top: 18px; {}", last.m.map.as_deref().map(map_art).unwrap_or_default());
                let last_id = last.m.id;
                let last_where = format!("{} · {}", last.m.map.clone().unwrap_or_default(), mode_name(last.m.mode));
                let last_award = last.me.award.clone();
                let last_hero = last.me.hero.clone();
                let sub_cover = format!(
                    "{} — {}, {} played",
                    verdict(wins, n),
                    games_label(n),
                    fmt_dur(Some(played))
                );
                let session_cards: Vec<_> = session.iter().rev().map(|g| (g.m.clone(), g.me.clone(), g.won)).collect();
                let games_n = games.len();
                view! {
                    <header class="ds-cover" style=cover_style>
                        <div class="ds-kicker">
                            {format!("Session · {} · {} → {}", net::long_date(&first_at), net::hhmm(&first_at), net::hhmm(&last_at))}
                        </div>
                        <div class="ds-cover-grid">
                            <div>
                                <h1 class="ds-title" style="cursor: default">{if is_today { "Tonight" } else { "Last session" }}</h1>
                                <div class="ds-bigrec">{wins}<span>"–"</span>{n - wins}</div>
                                <Pips games=session.iter().map(|g| g.won).collect() />
                                <div class="ds-subtitle" style="cursor: default; margin-top: 6px">{sub_cover}</div>
                            </div>
                            <Link class="ds-lastgame" to=format!("/match/{last_id}")>
                                {last_hero.clone().map(|h| view! { <Portrait hero=h size=132 tone=if last_won { Tone::Pick } else { Tone::Loss } /> })}
                                <div>
                                    <span class="ds-label">"Last game"</span>
                                    <strong>{last_hero.clone().unwrap_or_else(|| "?".into())}</strong>
                                    <em>{last_where}</em>
                                    <span style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap">
                                        <span class=format!("ds-tag {}", if last_won { "win" } else { "loss" })>{if last_won { "WIN" } else { "LOSS" }}</span>
                                        <AwardTag raw=last_award />
                                        <span class="ds-btn primary small">"Open match ›"</span>
                                    </span>
                                </div>
                            </Link>
                        </div>
                        <div class="ds-statline">
                            <div class="ds-stat"><span class="ds-label">"Win rate"</span><strong>{pct_of(wins as u32, n as u32)}<span>"%"</span></strong><small>{format!("{wins}/{n}")}</small></div>
                            <div class="ds-stat"><span class="ds-label">"K / D / A"</span><strong>{tk}<span>"/"</span>{td}<span>"/"</span>{ta}</strong>
                                <small>{format!("{} KDA ratio", to_fixed1((tk + ta) as f64 / td.max(1) as f64))}</small></div>
                            <div class="ds-stat"><span class="ds-label">"MVP"</span><strong>{mvps}</strong><small>{format!("of {}", games_label(n))}</small></div>
                            <div class="ds-stat"><span class="ds-label">"Heroes"</span><strong>{heroes.len()}</strong><small>{heroes.join(" · ")}</small></div>
                        </div>
                    </header>

                    <section class="ds-sec">
                        <SecHead num=1 title=if is_today { "Tonight's games" } else { "Session games" } sub="newest first" />
                        <div class="ds-gcards">
                            {session_cards.into_iter().map(|(m, me, won)| view! { <GameCard m=m me=me won=won /> }).collect_view()}
                        </div>
                    </section>

                    <section class="ds-sec">
                        <SecHead num=2 title="Your form" sub=format!("last {games_n} games in the archive") />
                        <div class="ds-grid2">
                            <div class="ds-panel">
                                <span class="ds-label">{format!("Last {} results · newest left", form.len().min(30))}</span>
                                <Pips games=form.iter().take(30).copied().collect() />
                                <div class="ds-formstats">
                                    <div><strong>{pct_of(form_wins as u32, form.len() as u32)}<span>"%"</span></strong><em>{format!("win rate · {form_wins}/{}", form.len())}</em></div>
                                    <div><strong>{form.iter().take(10).filter(|w| **w).count()}<span>{format!("/{}", form.len().min(10))}</span></strong><em>"last ten"</em></div>
                                </div>
                                <span class="ds-label" style="margin-top: 22px">"By mode"</span>
                                {modes.into_iter().map(|t| {
                                    let p = pct_of(t.wins, t.games);
                                    view! {
                                        <div class="ds-modrow">
                                            <span><span class=format!("ds-mode {}", mode_fam(&t.key))>{t.key.clone()}</span></span>
                                            <i class="ds-bar"><b style=format!("width: {p}%")></b></i>
                                            <em>{format!("{p}% · {}/{}", t.wins, t.games)}</em>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                            <div class="ds-panel">
                                <span class="ds-label">"Most played"</span>
                                {heroes6.into_iter().map(|h| view! {
                                    <Link to=format!("/hero/{}", uri_component(&h.key)) class="ds-hrow">
                                        <Portrait hero=h.key.clone() size=34 />
                                        <span>{h.key.clone()}</span>
                                        <i class="ds-bar"><b style=format!("width: {}%", ((h.games * 100) as f64 / max_hero as f64).round())></b></i>
                                        <em>{format!("{}g · {}%", h.games, pct_of(h.wins, h.games))}</em>
                                    </Link>
                                }).collect_view()}
                            </div>
                        </div>
                    </section>
                }.into_any()
            }}
        </div>
    }
}
