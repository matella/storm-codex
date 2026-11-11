//! Session sources: `/ticker` (corner pill), `/widget` (last game card) and `/queue` (between-games panel).
//! Port of `web/src/pages/Ticker.tsx`, `Widget.tsx` and `Queue.tsx`; the computations live in
//! `storm_web_core::session` (tested natively).

use crate::{
    frame::{operator_names, provide_dim, recent_matches, Avatar, OverlayFrame},
    now_playing::{use_now_playing, MusicCard},
};
use leptos::prelude::*;
use storm_web_core::{
    music::parse_track,
    net,
    route::{query_value, Anchor},
    session::*,
};

// ── /ticker ────────────────────────────────────────────────────────────────────

pub fn mount_ticker() {
    mount_to_body(Ticker);
}

#[derive(Clone, PartialEq)]
struct TickerData {
    hero: Option<String>,
    wins: usize,
    losses: usize,
    streak: i32,
}

#[component]
fn Ticker() -> impl IntoView {
    provide_dim();
    let names = operator_names();
    let matches = recent_matches(200);
    // the operator's session: today (local time), else the last day played
    let data = Memo::new(move |_| {
        let names = names.get();
        matches.with(|ms| {
            let all = operator_games(ms, &names, net::local_day);
            let (games, _) = session_games(&all, &net::today_local());
            let last = games.first()?;
            let wins = games.iter().filter(|g| g.won).count();
            let results: Vec<bool> = games.iter().map(|g| g.won).collect();
            Some(TickerData {
                hero: last.me.hero.clone(),
                wins,
                losses: games.len() - wins,
                streak: current_streak(&results),
            })
        })
    });
    view! {
        <OverlayFrame anchor=Anchor::TopRight top=250.0>
            {move || match data.get() {
                None => view! { <div></div> }.into_any(),
                Some(t) => view! {
                    <div style="display:inline-flex;align-items:center;gap:9px;background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:999px;padding:6px 13px 6px 7px;box-shadow:0 6px 20px rgba(0,0,0,.5);font-size:15px">
                        <Avatar hero=t.hero size=24.0 />
                        <span class="mono" style="font-weight:600">
                            <span style="color:var(--win)">{t.wins}"W"</span>
                            <span style="color:var(--text-2)">"–"</span>
                            <span style="color:var(--loss)">{t.losses}"L"</span>
                        </span>
                        {(t.streak != 0).then(|| view! {
                            <span class="mono" style=format!("font-size:12.5px;color:{}", if t.streak > 0 { "var(--win)" } else { "var(--loss)" })>
                                {if t.streak > 0 { format!("W{} ▲", t.streak) } else { format!("L{} ▼", -t.streak) }}
                            </span>
                        })}
                    </div>
                }
                .into_any(),
            }}
        </OverlayFrame>
    }
}

// ── /widget ────────────────────────────────────────────────────────────────────

pub fn mount_widget() {
    mount_to_body(Widget);
}

#[derive(Clone, PartialEq)]
struct WidgetData {
    won: bool,
    hero: Option<String>,
    map: String,
    mode: (&'static str, &'static str),
    kad: (u32, u32, u32),
    kp: Option<u32>,
    award: Option<Award>,
    length: String,
    phrase: String,
    map_bg: Option<String>,
}

/// Summary of the last game from the operator's point of view (`?me=<in-game name>`, else a configured operator
/// name, else the first player).
#[component]
fn Widget() -> impl IntoView {
    provide_dim();
    let names = operator_names();
    let matches = recent_matches(1);
    let me_param = query_value(&net::search(), "me");
    let data = Memo::new(move |_| {
        let names = names.get();
        matches.with(|ms| {
            let m = ms.first()?;
            let players = m.players.clone().unwrap_or_default();
            let me = pick_operator(&players, me_param.as_deref(), &names);
            let won = me.is_some_and(|p| p.team.is_some() && m.winner == p.team);
            let (k, a, d) = me.map(kad).unwrap_or((0, 0, 0));
            let td = me.and_then(|p| p.takedowns).unwrap_or(0);
            let hero = me.and_then(|p| p.hero.clone());
            let phrase = jarvis_phrase(won, hero.as_deref(), d, td, 0);
            Some(WidgetData {
                won,
                kp: me.and_then(|p| kill_participation(&players, p)),
                award: award_label(me.and_then(|p| p.award.as_deref())),
                hero,
                map: m.map.clone().unwrap_or_default(),
                mode: mode_badge(m.mode),
                kad: (k, a, d),
                length: fmt_dur(m.length),
                phrase,
                map_bg: map_image(m.map.as_deref()),
            })
        })
    });
    view! {
        <OverlayFrame anchor=Anchor::TopRight top=250.0>
            {move || match data.get() {
                None => view! { <div></div> }.into_any(),
                Some(w) => widget_card(w).into_any(),
            }}
        </OverlayFrame>
    }
}

fn widget_card(w: WidgetData) -> impl IntoView {
    let tone = if w.won { "var(--win)" } else { "var(--loss)" };
    let bg = match &w.map_bg {
        // map art under a heavy veil so the text stays readable
        Some(url) => format!("background-image:linear-gradient(90deg, rgba(14,16,22,.96) 0%, rgba(14,16,22,.86) 55%, rgba(14,16,22,.68) 100%), url({url});"),
        None => String::new(),
    };
    let (k, a, d) = w.kad;
    let hero_name = w.hero.clone().unwrap_or_else(|| "?".into());
    view! {
        <div style=format!("max-width:360px;background-color:rgba(14,16,22,.92);{bg}background-size:cover;background-position:center 30%;border:1px solid {tone};border-radius:12px;padding:12px 14px;display:flex;align-items:center;gap:12px;box-shadow:0 8px 30px rgba(0,0,0,.5)")>
            <Avatar hero=w.hero.clone() size=44.0 />
            <div style="flex:1">
                <div style="font-size:13px;font-weight:600;display:flex;align-items:center;gap:7px">
                    <span style=format!("color:{tone}")>{if w.won { "VICTORY" } else { "DEFEAT" }}</span>
                    <span style="color:var(--text-2);font-weight:400">{format!("· {hero_name} · {}", w.map)}</span>
                    {w.award.map(|aw| view! {
                        <span style=format!("margin-left:auto;font-size:10.5px;font-weight:700;padding:2px 7px;border-radius:999px;color:{};background:{};box-shadow:{};white-space:nowrap",
                            if aw.mvp { "#1a1500" } else { "var(--text)" },
                            if aw.mvp { "linear-gradient(90deg,#f5c542,#e0a818)" } else { "rgba(255,255,255,.1)" },
                            if aw.mvp { "0 0 10px rgba(245,197,66,.5)" } else { "none" })>
                            {if aw.mvp { "👑 MVP".to_string() } else { format!("{} {}", aw.icon, aw.label) }}
                        </span>
                    })}
                </div>
                <div class="mono" style="font-size:12px;color:#cfd3e0;margin-top:3px">
                    {format!("{k}/{a}/{d}")}
                    {w.kp.map(|kp| view! { <span style="color:var(--text-2)">{format!(" · KP {kp}%")}</span> })}
                    <span style="color:var(--text-2)">" · "</span>
                    <span class=format!("bdg {}", w.mode.1)>{w.mode.0}</span>
                    <span style="color:var(--text-2)">{format!(" · {}", w.length)}</span>
                </div>
                <div style="font-size:11px;color:var(--u-nexus);font-style:italic;margin-top:5px">
                    {format!("« {} » — Jarvis", w.phrase)}
                </div>
            </div>
        </div>
    }
}

// ── /queue ─────────────────────────────────────────────────────────────────────

pub fn mount_queue() {
    mount_to_body(Queue);
}

#[derive(Clone, PartialEq)]
struct QueueRow {
    won: bool,
    map: String,
    mode: (&'static str, &'static str),
    hero: Option<String>,
    account: Option<String>,
    award: Option<Award>,
    kad: (u32, u32, u32),
}

#[derive(Clone, PartialEq)]
struct QueueData {
    label: &'static str,
    day: Option<String>,
    wins: usize,
    losses: usize,
    games: usize,
    wr: u32,
    streak: i32,
    by_mode: Vec<(Option<i64>, u32, u32)>,
    /// Oldest first, for the W/L dots.
    chrono: Vec<(i64, bool, String)>,
    rows: Vec<QueueRow>,
    heroes: Vec<(String, u32, u32)>,
    totals: SessionTotals,
    jarvis: Option<String>,
}

/// Between-games panel: tonight's record, recent games, heroes played, KDA and a Jarvis line, next to framed
/// slots for the camera and the game capture, and the music card. English UI, transparent background.
#[component]
fn Queue() -> impl IntoView {
    provide_dim();
    let names = operator_names();
    let matches = recent_matches(200);
    let np = use_now_playing(5000);
    let data = Memo::new(move |_| {
        let names = names.get();
        matches.with(|ms| {
            let all = operator_games(ms, &names, net::local_day);
            let (games, is_today) = session_games(&all, &net::today_local());
            let multi = names.len() > 1;
            let wins = games.iter().filter(|g| g.won).count();
            let results: Vec<bool> = games.iter().map(|g| g.won).collect();
            let streak = current_streak(&results);
            let n = games.len();
            QueueData {
                label: if is_today || n == 0 {
                    "TODAY'S SESSION"
                } else {
                    "LAST SESSION"
                },
                day: (!is_today && n > 0).then(|| games[0].day.clone()),
                wins,
                losses: n - wins,
                games: n,
                wr: if n > 0 {
                    (100.0 * wins as f64 / n as f64).round() as u32
                } else {
                    0
                },
                streak,
                by_mode: record_by(&games, |g| g.m.mode),
                chrono: games
                    .iter()
                    .rev()
                    .map(|g| (g.m.id, g.won, g.m.map.clone().unwrap_or_default()))
                    .collect(),
                rows: games
                    .iter()
                    .take(8)
                    .map(|g| QueueRow {
                        won: g.won,
                        map: g.m.map.clone().unwrap_or_default(),
                        mode: mode_badge(g.m.mode),
                        hero: g.me.hero.clone(),
                        account: if multi {
                            g.me.name.clone().filter(|n| !n.is_empty())
                        } else {
                            None
                        },
                        award: award_label(g.me.award.as_deref()),
                        kad: kad(g.me),
                    })
                    .collect(),
                heroes: record_by(&games, |g| g.me.hero.clone().unwrap_or_else(|| "?".into()))
                    .into_iter()
                    .take(10)
                    .collect(),
                totals: session_totals(&games),
                jarvis: games.first().map(|g| {
                    jarvis_phrase(
                        g.won,
                        g.me.hero.as_deref(),
                        g.me.deaths.unwrap_or(0),
                        g.me.takedowns.unwrap_or(0),
                        streak,
                    )
                }),
            }
        })
    });
    view! {
        <div style="width:1920px;height:1080px;display:grid;grid-template-columns:minmax(0,1.5fr) minmax(0,1fr);gap:18px;padding:24px;box-sizing:border-box">
            {move || queue_panel(data.get())}
            // Right column: camera and game are framed slots to drop the OBS sources into; music is built in.
            <div style="display:grid;grid-template-rows:1fr 1fr auto;gap:18px;height:100%;min-height:0;box-sizing:border-box">
                <Frame label="CAMERA" />
                <Frame label="GAME · IN QUEUE" />
                {move || { let t = np.with(parse_track); let authenticated = np.with(|v| v.get("authenticated").is_some_and(|a| !matches!(a, serde_json::Value::Null | serde_json::Value::Bool(false)))); view! { <MusicCard t authenticated /> } }}
            </div>
        </div>
    }
}

fn queue_panel(q: QueueData) -> impl IntoView {
    let t = &q.totals;
    let n = q.games.max(1) as f64;
    let kda_color = if t.kda >= 3.0 {
        "var(--win)"
    } else {
        "#cfd3e0"
    };
    let avg = format!(
        "{}/{}/{}",
        to_fixed1(t.kills as f64 / n),
        to_fixed1(t.assists as f64 / n),
        to_fixed1(t.deaths as f64 / n)
    );
    let takedowns = format!("{} takedowns", t.takedowns);
    let avg_len = t.avg_len.map(|l| format!(" · avg {}", fmt_dur(Some(l))));
    let modes = (q.by_mode.len() >= 2).then(|| {
        q.by_mode
            .iter()
            .map(|(mode, w, l)| {
                let mb = mode_badge(*mode);
                view! {
                    <span style="display:inline-flex;align-items:center;gap:6px;font-size:14px">
                        <span class=format!("bdg {}", mb.1)>{mb.0}</span>
                        <span class="mono">
                            <span style="color:var(--win)">{*w}</span>"–"<span style="color:var(--loss)">{*l}</span>
                        </span>
                    </span>
                }
            })
            .collect_view()
    });
    let last_idx = q.chrono.len().saturating_sub(1);
    let dots = q.chrono.iter().enumerate().map(|(i, (_, won, map))| {
        let last = i == last_idx;
        view! {
            <span title=format!("{} · {map}", if *won { "W" } else { "L" })
                style=format!("width:17px;height:17px;border-radius:5px;background:{};opacity:{};box-shadow:{}",
                    if *won { "var(--win)" } else { "var(--loss)" },
                    if last { "1" } else { "0.82" },
                    if last { "0 0 0 2px rgba(255,255,255,.35)" } else { "none" })></span>
        }
    }).collect_view();
    let rows = q.rows.iter().map(|r| {
        let (k, a, d) = r.kad;
        view! {
            <div style="display:flex;align-items:center;gap:10px;padding:7px 0;border-bottom:1px solid var(--hairline);font-size:15px">
                <span class=format!("bdg {}", r.mode.1)>{r.mode.0}</span>
                <Avatar hero=r.hero.clone() size=28.0 />
                <span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
                    {r.map.clone()}
                    {r.account.clone().map(|a| view! { <span class="muted" style="font-size:11px;margin-left:8px">{format!("· {a}")}</span> })}
                </span>
                {r.award.clone().map(|aw| if aw.mvp {
                    view! { <span title="MVP" style="font-size:10px;font-weight:700;padding:1px 6px;border-radius:999px;color:#1a1500;background:linear-gradient(90deg,#f5c542,#e0a818)">"👑 MVP"</span> }.into_any()
                } else {
                    view! { <span title=aw.label.clone() style="font-size:13px">{aw.icon}</span> }.into_any()
                })}
                <span class=format!("bdg {}", if r.won { "b-win" } else { "b-loss" })>{if r.won { "W" } else { "L" }}</span>
                <span class="mono" style="color:#cfd3e0">{format!("{k}/{a}/{d}")}</span>
            </div>
        }
    }).collect_view();
    let heroes = q.heroes.iter().map(|(h, w, l)| {
        view! {
            <div style="display:flex;align-items:center;gap:9px;margin-bottom:6px;font-size:14px">
                <Avatar hero=Some(h.clone()) size=24.0 />
                <span style="flex:1">{h.clone()}</span>
                <span class="mono" style="color:#cfd3e0">
                    <span style="color:var(--win)">{*w}</span>"-"<span style="color:var(--loss)">{*l}</span>
                </span>
            </div>
        }
    }).collect_view();
    view! {
        <div style="background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:16px;padding:22px 26px;box-shadow:0 8px 30px rgba(0,0,0,.5);height:100%;box-sizing:border-box;display:flex;flex-direction:column;overflow:hidden">
            <div style="display:flex;align-items:baseline;gap:10px">
                <span style="font-size:22px;font-weight:600;letter-spacing:.04em">{q.label}</span>
                {q.day.clone().map(|d| view! { <span class="mono" style="font-size:13px;color:var(--text-2)">{net::fmt_day(&d)}</span> })}
                <span class="mono" style="margin-left:auto;font-size:24px">
                    <span style="color:var(--win)">{q.wins}"W"</span>" – "<span style="color:var(--loss)">{q.losses}"L"</span>
                </span>
            </div>
            <div class="mono" style="font-size:13px;color:var(--text-2);margin-top:4px">
                {format!("{} · {} games · {}% win rate", streak_label(q.streak), q.games, q.wr)}
            </div>
            {modes.map(|m| view! { <div style="display:flex;flex-wrap:wrap;gap:12px;margin-top:8px">{m}</div> })}
            {(!q.chrono.is_empty()).then(|| view! { <div style="display:flex;flex-wrap:wrap;gap:5px;margin-top:16px">{dots}</div> })}
            <div class="kick" style="margin:18px 0 6px;font-size:12px">"Recent games"</div>
            {rows}
            {(q.games == 0).then(|| view! { <div style="font-size:14px;color:var(--text-2)">"No games yet today."</div> })}
            <div style="display:flex;gap:28px;margin-top:20px">
                <div style="flex:1">
                    <div class="kick" style="margin:0 0 8px;font-size:12px">"Heroes today"</div>
                    {if q.heroes.is_empty() { view! { <span style="font-size:13px;color:var(--text-2)">"—"</span> }.into_any() } else { heroes.into_any() }}
                </div>
                {(q.games > 0).then(|| view! {
                    <div style="flex:1">
                        <div class="kick" style="margin:0 0 8px;font-size:12px">"Session"</div>
                        <div style="display:flex;align-items:baseline;gap:8px">
                            <span class="mono" style=format!("font-size:28px;font-weight:700;color:{kda_color}")>{to_fixed1(t.kda)}</span>
                            <span class="muted" style="font-size:12px">"KDA ratio"</span>
                        </div>
                        <div class="mono" style="font-size:13px;color:var(--text-2);margin-top:8px;line-height:1.8">
                            <div>"avg "<span style="color:#cfd3e0">{avg}</span>" per game"</div>
                            <div>{takedowns}{avg_len}</div>
                        </div>
                    </div>
                })}
            </div>
            // spacer pushes the Jarvis line to the bottom of the full-height panel
            <div style="flex:1;min-height:16px"></div>
            {q.jarvis.map(|j| view! {
                <div style="font-size:16px;color:var(--u-nexus);font-style:italic;border-top:1px solid var(--hairline);padding-top:14px">
                    {format!("« {j} » — Jarvis")}
                </div>
            })}
        </div>
    }
}

/// Bordered placeholder zone: align the matching OBS source (camera / game capture) inside it.
#[component]
fn Frame(label: &'static str) -> impl IntoView {
    view! {
        <div style="position:relative;border:1.5px dashed var(--hairline-strong);border-radius:12px;min-height:0">
            <span class="kick" style="position:absolute;top:10px;left:14px;font-size:11px;opacity:.55">{label}</span>
        </div>
    }
}
