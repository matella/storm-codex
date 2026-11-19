//! Session: one play session of the operator as the cover (the latest one, or `?s=<id>`), its review against the
//! player's own baseline (`GET /api/sessions/{id}`, spec `docs/specs/2026-11-19-session-review.md`), its games,
//! then the recent form (logic in `core::dashboard` and `core::review`).

use leptos::prelude::*;
use leptos_router::hooks::use_location;
use storm_web_core::{
    dashboard::{
        by_hero, by_mode, games_label, kda, mode_fam, mode_name, my_games, pct_of, verdict,
    },
    net,
    review::{
        better, fmt_delta, fmt_span, mean_deltas, rate, selected, streak_label, Review, ReviewGame,
        Row, SessionList,
    },
    route::uri_component,
    session::{fmt_dur, to_fixed1, MatchSummary},
};

use crate::{
    layout::LiveBus,
    matchui::{AwardTag, GameCard, Pips},
    ui::{
        fetch_operator_stats, fetch_reference, map_art, operator_stats_url, use_operator,
        use_owner_toons, Link, Portrait, SecHead, Tone,
    },
};

#[component]
pub fn Session() -> impl IntoView {
    let loc = use_location();
    let chosen = Memo::new(move |_| selected(&loc.search.get()));
    let latest = fetch_operator_stats::<SessionList>(|| "/api/sessions?limit=1".to_string());
    // outer `None` = still loading; a memo, so a refetch that finds the same session does not remount it
    let latest_id = Memo::new(move |_| {
        latest
            .get()
            .map(|r| r.map(|l| l.sessions.first().map(|s| s.id)))
    });
    view! {
        <div class="ds-page">
            {move || match (chosen.get(), latest_id.get()) {
                (Some(id), _) | (None, Some(Ok(Some(id)))) => view! { <SessionView id=id /> }.into_any(),
                (None, None) => view! { <div class="ds-empty" style="margin-top: 24px">"loading…"</div> }.into_any(),
                (None, Some(Ok(None))) => view! { <NoSession /> }.into_any(),
                (None, Some(Err(_))) => view! { <SessionLoadError /> }.into_any(),
            }}
            <YourForm />
        </div>
    }
}

/// Cover shown when there is no session to read (no operator account yet, or none of its games uploaded).
#[component]
fn NoSession() -> impl IntoView {
    let operator = use_operator();
    let toons = use_owner_toons();
    move || {
        let configured = !operator.with(Vec::is_empty) || !toons.with(Vec::is_empty);
        let sub = if configured {
            "None of your accounts appears in the archive yet — upload replays with the uploader."
        } else {
            "Tell Storm Codex which accounts are yours in Admin → operator names."
        };
        view! {
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Session"</div>
                <h1 class="ds-title" style="cursor: default">"No session yet"</h1>
                <div class="ds-subtitle" style="cursor: default">{sub}</div>
                <div class="ds-cover-foot">
                    <Link class="ds-btn primary" to="/admin">"Open Admin"</Link>
                    <Link class="ds-btn" to="/matches">"Browse matches"</Link>
                </div>
            </header>
        }
    }
}

#[component]
fn SessionLoadError() -> impl IntoView {
    view! {
        <header class="ds-cover" style="margin-top: 18px" role="alert">
            <div class="ds-kicker">"Session"</div>
            <h1 class="ds-title" style="cursor: default">"Unable to load session"</h1>
            <div class="ds-subtitle" style="cursor: default">"Check your connection to Storm Codex and try again."</div>
        </header>
    }
}

/// One session: cover, review, games.
#[component]
fn SessionView(id: i64) -> impl IntoView {
    let url = operator_stats_url(move || format!("/api/sessions/{id}"));
    let review = LocalResource::new(move || {
        let url = url();
        async move { net::get_json_or_not_found::<Review>(&url).await }
    });
    move || {
        let Some(res) = review.get() else {
            return view! { <div class="ds-empty" style="margin-top: 24px">"loading…"</div> }
                .into_any();
        };
        match res {
            Ok(Some(r)) if !r.games.is_empty() => view! { <SessionBody r=r /> }.into_any(),
            Err(_) => view! { <SessionLoadError /> }.into_any(),
            Ok(_) => view! {
                <header class="ds-cover" style="margin-top: 18px">
                    <div class="ds-kicker">"Session"</div>
                    <h1 class="ds-title" style="cursor: default">"Session not found"</h1>
                    <div class="ds-subtitle" style="cursor: default">"No session of yours starts with that game."</div>
                    <div class="ds-cover-foot"><Link class="ds-btn primary" to="/">"Latest session"</Link></div>
                </header>
            }
            .into_any(),
        }
    }
}

fn session_link(id: i64) -> String {
    format!("/?s={id}")
}

#[component]
fn SessionBody(r: Review) -> impl IntoView {
    let n = r.games.len();
    let wins = r.record.wins as usize;
    let (tk, td, ta) = r.games.iter().fold((0u32, 0u32, 0u32), |(k, d, a), g| {
        let x = kda(&g.card().1);
        (k + x.0, d + x.1, a + x.2)
    });
    let mvps = r
        .games
        .iter()
        .filter(|g| g.award.as_deref().unwrap_or("").contains("MVP"))
        .count();
    let mut heroes: Vec<String> = Vec::new();
    for h in r
        .games
        .iter()
        .filter_map(|g| g.hero.clone())
        .filter(|h| !h.is_empty())
    {
        if !heroes.contains(&h) {
            heroes.push(h);
        }
    }
    let Some(last) = r.games.last().cloned() else {
        return ().into_any();
    };
    let first_at = r.start.clone();
    let last_at = last.played_at.clone();
    let is_today = r.next.is_none() && net::local_day(&last_at) == net::today_local();
    let title = if is_today {
        "Tonight"
    } else if r.next.is_none() {
        "Last session"
    } else {
        "Session"
    };
    let last_award = last.award.clone();
    let cover_style = format!(
        "margin-top: 18px; {}",
        last.map.as_deref().map(map_art).unwrap_or_default()
    );
    let last_where = format!(
        "{} · {}",
        last.map.clone().unwrap_or_default(),
        mode_name(last.mode)
    );
    let sub_cover = format!(
        "{} — {}, {} played",
        verdict(wins, n),
        games_label(n),
        fmt_dur(Some(r.played_s))
    );
    let pips: Vec<bool> = r.games.iter().map(|g| g.win).collect();
    let cards: Vec<ReviewGame> = r.games.iter().rev().cloned().collect();
    let (prev, next) = (r.prev, r.next);
    view! {
        <header class="ds-cover" style=cover_style>
            <div class="ds-kicker">
                {prev.map(|p| view! { <Link class="ds-sess-nav" to=session_link(p)>"‹ Previous session"</Link> })}
                {format!("Session · {} · {} → {}", net::long_date(&first_at), net::hhmm(&first_at), net::hhmm(&last_at))}
                {next.map(|x| view! { <Link class="ds-sess-nav" to=session_link(x)>"Next session ›"</Link> })}
            </div>
            <div class="ds-cover-grid">
                <div>
                    <h1 class="ds-title" style="cursor: default">{title}</h1>
                    <div class="ds-bigrec">{wins}<span>"–"</span>{n - wins}</div>
                    <Pips games=pips />
                    <div class="ds-subtitle" style="cursor: default; margin-top: 6px">{sub_cover}</div>
                </div>
                <Link class="ds-lastgame" to=format!("/match/{}", last.match_id)>
                    {last.hero.clone().map(|h| view! { <Portrait hero=h size=132 tone=if last.win { Tone::Pick } else { Tone::Loss } /> })}
                    <div>
                        <span class="ds-label">"Last game"</span>
                        <strong>{last.hero.clone().unwrap_or_else(|| "?".into())}</strong>
                        <em>{last_where}</em>
                        <span style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap">
                            <span class=format!("ds-tag {}", if last.win { "win" } else { "loss" })>{if last.win { "WIN" } else { "LOSS" }}</span>
                            <AwardTag raw=last_award />
                            <span class="ds-btn primary small">"Open match ›"</span>
                        </span>
                    </div>
                </Link>
            </div>
            <div class="ds-statline">
                <div class="ds-stat"><span class="ds-label">"Win rate"</span><strong>{pct_of(wins as u32, n as u32)}<span>"%"</span></strong><small>{format!("{wins}/{n}")}</small></div>
                <div class="ds-stat"><span class="ds-label">"K / D / A"</span><strong>{tk}<span>"/"</span>{td}<span>"/"</span>{ta}</strong>
                    <small>{format!("{} KDA ratio", to_fixed1(f64::from(tk + ta) / f64::from(td.max(1))))}</small></div>
                <div class="ds-stat"><span class="ds-label">"MVP"</span><strong>{mvps}</strong><small>{format!("of {}", games_label(n))}</small></div>
                <div class="ds-stat"><span class="ds-label">"Heroes"</span><strong>{heroes.len()}</strong><small>{heroes.join(" · ")}</small></div>
            </div>
        </header>

        <ReviewSection r=r />

        <section class="ds-sec">
            <SecHead num=2 title=if is_today { "Tonight's games" } else { "Session games" } sub="newest first" />
            <div class="ds-gcards">
                {cards.into_iter().map(|g| {
                    let (m, me) = g.card();
                    view! { <GameCard m=m me=me won=g.win /> }
                }).collect_view()}
            </div>
        </section>
    }
    .into_any()
}

fn tone(stat: &str, delta: f64) -> &'static str {
    match better(stat, delta) {
        Some(true) => "ds-delta up",
        Some(false) => "ds-delta down",
        None => "ds-delta",
    }
}

fn rows_panel(label: &'static str, rows: Vec<Row>) -> impl IntoView {
    view! {
        <div class="ds-panel">
            <span class="ds-label">{label}</span>
            {rows.into_iter().map(|x| {
                let base = match rate(x.base_wins, x.base_games) {
                    Some(p) => format!("usual {p}% · {}g", x.base_games),
                    None => "no baseline".to_string(),
                };
                view! {
                    <div class="ds-revrow">
                        <span>{x.name.clone()}</span>
                        <strong>{format!("{}–{}", x.wins, x.games.saturating_sub(x.wins))}</strong>
                        <em>{base}{x.small_sample.then_some(" · small sample")}</em>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

/// "Review": record against the usual win rate, streaks and duration, stat deltas, heroes and maps, best and
/// worst game.
#[component]
fn ReviewSection(r: Review) -> impl IntoView {
    let b = r.baseline.clone();
    let session_rate = rate(r.record.wins, r.games.len() as u32).unwrap_or(0);
    let base_rate = rate(b.wins, b.games);
    let modes: Vec<String> = b.modes.iter().map(|m| mode_name(*m)).collect();
    let sub = format!(
        "vs your previous 90 days · {} · {}",
        modes.join(" · "),
        games_label(b.games as usize)
    );
    let deltas = mean_deltas(&r.games);
    let pick =
        |id: Option<i64>| id.and_then(|id| r.games.iter().find(|g| g.match_id == id).cloned());
    let picks = [("Best game", pick(r.best)), ("Worst game", pick(r.worst))];
    let has_picks = picks.iter().any(|(_, g)| g.is_some());
    let s = r.streaks.clone();
    view! {
        <section class="ds-sec">
            <SecHead num=1 title="Review" sub=sub />
            {b.small_sample.then(|| view! { <div class="ds-empty">{format!("Small sample: {} games. Read these as hints.", b.games)}</div> })}
            <div class="ds-statline">
                <div class="ds-stat"><span class="ds-label">"Win rate vs usual"</span>
                    <strong>{session_rate}<span>"%"</span></strong>
                    <small>{match base_rate { Some(p) => format!("usual {p}% · {}/{}", b.wins, b.games), None => "no baseline".into() }}</small></div>
                <div class="ds-stat"><span class="ds-label">"Streak at the end"</span>
                    <strong>{streak_label(s.current)}</strong>
                    <small>{format!("longest W{} · L{}", s.best_win, s.worst_loss)}</small></div>
                <div class="ds-stat"><span class="ds-label">"Duration"</span>
                    <strong>{fmt_span(r.span_s)}</strong>
                    <small>{format!("{} in game", fmt_span(r.played_s))}</small></div>
            </div>
            <div class="ds-panel">
                <span class="ds-label">"Per game vs your average · same hero with 5+ games, else overall"</span>
                <div class="ds-deltas">
                    {deltas.into_iter().map(|(k, label, v)| view! {
                        <div><em>{label}</em><strong class=tone(k, v)>{fmt_delta(v)}</strong></div>
                    }).collect_view()}
                </div>
            </div>
            <div class="ds-grid2" style="margin-top: 14px">
                {rows_panel("Heroes · session vs usual", r.heroes.clone())}
                {rows_panel("Maps · session vs usual", r.maps.clone())}
            </div>
            {has_picks.then(|| view! {
                <div class="ds-gcards" style="margin-top: 14px">
                    {picks.into_iter().filter_map(|(label, g)| {
                        let g = g?;
                        let (m, me) = g.card();
                        let head = format!("{label} · {}/{} stats above usual", g.above(), g.deltas.len());
                        Some(view! {
                            <div>
                                <span class="ds-label">{head}</span>
                                <GameCard m=m me=me won=g.win />
                            </div>
                        })
                    }).collect_view()}
                </div>
            })}
        </section>
    }
}

/// "Your form" over the last 500 games in the archive (unchanged by the session review).
#[component]
fn YourForm() -> impl IntoView {
    let operator = use_operator();
    let toons = use_owner_toons();
    let bus = expect_context::<LiveBus>();
    let matches = fetch_reference::<Vec<MatchSummary>>(move || {
        bus.matches.track(); // a parsed match re-reads the list
        "/api/matches?limit=500".to_string()
    });
    move || {
        let list = matches.get()?.ok()?;
        let names = operator.get();
        let handles = toons.get();
        let games = my_games(&list, &names, &handles);
        if games.is_empty() {
            return None;
        }
        let form: Vec<bool> = games.iter().map(|g| g.won).collect();
        let form_wins = form.iter().filter(|w| **w).count();
        let heroes6: Vec<_> = by_hero(&games).into_iter().take(6).collect();
        let max_hero = heroes6.first().map_or(1, |h| h.games);
        let modes = by_mode(&games, mode_name);
        let games_n = games.len();
        Some(view! {
            <section class="ds-sec">
                <SecHead num=3 title="Your form" sub=format!("last {games_n} games in the archive") />
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
        })
    }
}
