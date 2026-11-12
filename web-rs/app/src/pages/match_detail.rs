//! Match detail: header and verdict, draft, scoreboard, level advantage, team XP, timeline, chat, taunts and pings,
//! full data. The data is the rich storm-stats JSON, read as `serde_json::Value`.
//! The Replay 2D tab is `replay.rs`.
//! Port of `web/src/pages/MatchDetail.tsx`.

use std::{collections::HashMap, sync::Arc};

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde_json::Value;
use storm_web_core::{
    aggregates::has_map_art,
    herodetail::{decamelize, DimTalent},
    matchdetail::{
        announce_label, arr, ban_hero, ban_phase, bar_width, col_maxes, count, fmt_clock, fmt_k,
        get, idx, is_num, level_adv, num, num_eq, pings, score_rows, some, target, text,
        timeline_events, timeline_max, truthy, xp_lines, EvKind, Mark, COLS, MSG_ANNOUNCE,
        MSG_CHAT, MSG_PING,
    },
    net,
    route::uri_component,
    session::fmt_dur,
};

use crate::{
    matchui::{AwardTag, ModeTag},
    ui::{fetch, map_art, use_operator, use_talents, Link, Portrait, SecHead, Tone},
};

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

fn tone_of(team: i64) -> Tone {
    if team == 1 {
        Tone::Red
    } else {
        Tone::Blue
    }
}

/// Talent build of a player as chips in level order.
#[component]
fn TalentStrip(picks: Vec<String>, talents: RwSignal<HashMap<String, DimTalent>>) -> impl IntoView {
    (!picks.is_empty()).then(|| {
        view! {
            <div class="ds-talents">
                {picks.into_iter().enumerate().map(|(i, tid)| {
                    let info = {
                        let tid = tid.clone();
                        move || talents.with(|m| m.get(&tid).cloned())
                    };
                    let (title_info, tier_info, name_info) = (info.clone(), info.clone(), info);
                    let fallback = decamelize(&tid);
                    view! {
                        <span class="ds-talent" title=move || format!("Tier {}{}", i + 1, title_info().map(|t| format!(" · {}", t.name)).unwrap_or_default())>
                            <b>{move || tier_info().map_or(i as i64 + 1, |t| t.tier)}</b>
                            {move || name_info().map_or(fallback.clone(), |t| t.name)}
                        </span>
                    }
                }).collect_view()}
            </div>
        }
    })
}

#[component]
fn ScoreTable(
    players: Arc<Vec<Value>>,
    team: i64,
    label: &'static str,
    adv: RwSignal<bool>,
    me_toon: Option<String>,
    maxes: Vec<f64>,
    talents: RwSignal<HashMap<String, DimTalent>>,
) -> impl IntoView {
    let refs: Vec<&Value> = players.iter().collect();
    let rows = score_rows(&refs, team);
    let red = team == 1;
    let cols = move || {
        COLS.iter()
            .enumerate()
            .filter(|(_, c)| !c.adv || adv.get())
            .collect::<Vec<_>>()
    };
    let total_k: f64 = rows.iter().map(|r| r.kills).sum();
    let total_d: f64 = rows.iter().map(|r| r.deaths).sum();
    let col_totals: Vec<f64> = (0..COLS.len())
        .map(|i| rows.iter().map(|r| r.vals[i]).sum())
        .collect();
    let rows = Arc::new(rows);
    let maxes = Arc::new(maxes);
    let col_totals = Arc::new(col_totals);
    let (rows_b, maxes_b) = ((*rows).clone(), maxes.clone());
    view! {
        <div class="ds-panel flush scroll" style="margin-top: 14px">
            <table class="ds-table">
                <thead>
                    <tr>
                        <th class=if red { "ds-them" } else { "ds-us" }>{label}</th>
                        <th>"K/D/A"</th>
                        {move || cols().into_iter().map(|(_, c)| view! { <th>{c.label}</th> }).collect_view()}
                    </tr>
                </thead>
                <tbody>
                    {rows_b.into_iter().map(|r| {
                        let me = me_toon.as_deref() == Some(r.toon.as_str());
                        let (vals, maxes) = (r.vals.clone(), maxes_b.clone());
                        let cells = move || cols().into_iter().map(|(i, c)| {
                            let v = vals[i];
                            if c.bar {
                                view! {
                                    <td><div class="ds-cell"><span>{fmt_k(v)}</span>
                                        <i class=if red { "ds-bar red" } else { "ds-bar" }><b style=format!("width: {}%", bar_width(v, maxes[i]))></b></i></div></td>
                                }.into_any()
                            } else {
                                view! { <td><span class="ds-num">{net::fmt_n(v)}</span></td> }.into_any()
                            }
                        }).collect_view();
                        let tal = r.talents.clone().map(|picks| {
                            view! {
                                <tr class=if me { "me tal" } else { "tal" }>
                                    <td colspan=move || (cols().len() + 2).to_string()><TalentStrip picks=picks talents=talents /></td>
                                </tr>
                            }
                        });
                        view! {
                            <tr class=if me { "me" } else { "" }>
                                <td>
                                    <Link to=format!("/player/{}", uri_component(&r.toon)) class="ds-who">
                                        <Portrait hero=r.hero.clone() size=36 tone=tone_of(team) />
                                        <div><strong>{r.hero.clone()}</strong><em>{r.name.clone()}</em></div>
                                        <AwardTag raw=r.award.clone() />
                                    </Link>
                                </td>
                                <td class="ds-kda">{r.kills}<i>"/"</i>{r.deaths}<i>"/"</i>{r.assists}</td>
                                {cells}
                            </tr>
                            {tal}
                        }
                    }).collect_view()}
                    <tr class="total">
                        <td>"Team total"</td>
                        <td>{format!("{total_k}/{total_d}")}</td>
                        {move || cols().into_iter().map(|(i, c)| view! { <td>{if c.total { net::fmt_n(col_totals[i]) } else { String::new() }}</td> }.into_any()).collect_view()}
                    </tr>
                </tbody>
            </table>
        </div>
    }
}

/// Timeline of the events: a track of marks plus the list. Colour is the team that scores.
#[component]
fn MatchTimeline(m: Arc<Value>, players: Arc<Value>, n: u32) -> impl IntoView {
    let evs = timeline_events(&m, &players);
    if evs.is_empty() {
        return None;
    }
    let max_t = timeline_max(num(get(&m, "length")), &evs);
    let col = |team: i64| {
        if team == 0 {
            "var(--tm-blue)"
        } else {
            "var(--tm-red)"
        }
    };
    let sub = format!(
        "{} events · kills below the line, 🏰 structures and 🎯 objectives above",
        evs.len()
    );
    let marks = evs.iter().map(|e| {
        let left = format!("{}%", (e.t / max_t) * 100.0);
        let title = format!("{} · {}", fmt_dur(Some(e.t)), e.label);
        match e.kind {
            EvKind::Kill => view! { <span class="k" title=title style=format!("left: {left}; background: {}", col(e.team))></span> }.into_any(),
            k => view! {
                <span class="pin" title=title style=format!("left: {left}; top: {}px; border-bottom: 2px solid {}", if k == EvKind::Obj { 0 } else { 12 }, col(e.team))>{if k == EvKind::Obj { "🎯" } else { "🏰" }}</span>
            }.into_any(),
        }
    }).collect_view();
    let rows = evs.iter().map(|e| {
        let icon = match e.kind {
            EvKind::Kill => "⚔️",
            EvKind::Struct => "🏰",
            EvKind::Obj => "🎯",
        };
        view! {
            <div class=format!("ds-ev-row {}", if e.team == 0 { "blue" } else { "red" })>
                <span class="t">{fmt_dur(Some(e.t))}</span>
                <span>{icon}</span>
                {(e.kind == EvKind::Kill).then(|| e.hero.clone()).flatten().map(|h| view! { <Portrait hero=h size=20 /> })}
                <span>{e.label.clone()}</span>
                <span class=if e.team == 0 { "ds-us" } else { "ds-them" } style="margin-left: auto; font-size: 10px; font-family: var(--mono)">{if e.team == 0 { "BLUE" } else { "RED" }}</span>
            </div>
        }
    }).collect_view();
    Some(view! {
        <section class="ds-sec">
            <SecHead num=n title="Timeline" sub=sub />
            <div class="ds-panel">
                <div class="ds-track"><div class="base"></div>{marks}</div>
                <div class="ds-lvl-legend"><span>"0:00"</span><span>{fmt_dur(Some(max_t))}</span></div>
            </div>
            <div class="ds-panel flush ds-evlist" style="margin-top: 12px">{rows}</div>
        </section>
    })
}

#[component]
fn ChatLog(m: Arc<Value>, players: Arc<Value>, n: u32) -> impl IntoView {
    let show_pings = RwSignal::new(false);
    let show_announces = RwSignal::new(false);
    let all: Vec<Value> = arr(get(&m, "messages")).to_vec();
    let count_of = |t: i64| all.iter().filter(|x| num_eq(get(x, "type"), t)).count();
    let (chat, ping, announce) = (
        count_of(MSG_CHAT),
        count_of(MSG_PING),
        count_of(MSG_ANNOUNCE),
    );
    let all = Arc::new(all);
    view! {
        <section class="ds-sec">
            <SecHead num=n title="Match chat" />
            <div class="ds-panel flush">
                <div class="ds-panel-hd ds-pills">
                    <span class="ds-pill on">{format!("{chat} messages")}</span>
                    <span class=move || pill(show_pings.get()) on:click=move |_| show_pings.update(|p| *p = !*p)>{format!("pings ({ping})")}</span>
                    <span class=move || pill(show_announces.get()) on:click=move |_| show_announces.update(|p| *p = !*p)>{format!("callouts ({announce})")}</span>
                </div>
                {move || {
                    let (pings, announces) = (show_pings.get(), show_announces.get());
                    let mut shown: Vec<&Value> = all.iter().filter(|x| {
                        let t = get(x, "type");
                        num_eq(t, MSG_CHAT) || (pings && num_eq(t, MSG_PING)) || (announces && num_eq(t, MSG_ANNOUNCE))
                    }).collect();
                    shown.sort_by(|a, b| num(get(a, "loop")).total_cmp(&num(get(b, "loop"))));
                    let empty = shown.is_empty().then(|| view! { <div class="ds-empty-row">"no message"</div> });
                    let rows = shown.into_iter().map(|msg| {
                        let toon = text(get(msg, "player")).unwrap_or("");
                        let p = get(&players, toon);
                        let team = get(msg, "team");
                        let who_class = if is_num(team, 0.0) { "who ds-us" } else if is_num(team, 1.0) { "who ds-them" } else { "who muted" };
                        let name = text(get(p, "name")).unwrap_or(toon).to_string();
                        let hero = text(get(p, "hero")).filter(|h| !h.is_empty()).map(str::to_string);
                        let body = if num_eq(get(msg, "type"), MSG_CHAT) {
                            view! {
                                <span class="ds-tag award">{target(get(msg, "recipient")).unwrap_or("?")}</span>
                                <span class="msg">{text(get(msg, "text")).unwrap_or("").to_string()}</span>
                            }.into_any()
                        } else {
                            let label = if num_eq(get(msg, "type"), MSG_PING) {
                                format!("ping {}", target(get(msg, "recipient")).unwrap_or("")).trim().to_string()
                            } else {
                                announce_label(get(msg, "announcement")).to_string()
                            };
                            view! { <span class="muted" style="font-size: 11px; padding-top: 3px">{label}</span> }.into_any()
                        };
                        view! {
                            <div class="ds-chat-row">
                                <span class="t">{fmt_clock(num(get(msg, "time")))}</span>
                                {match hero {
                                    Some(h) => view! { <Portrait hero=h size=22 tone=tone_of(if is_num(team, 1.0) { 1 } else { 0 }) /> }.into_any(),
                                    None => view! { <span style="width: 22px"></span> }.into_any(),
                                }}
                                <span class=who_class title=toon.to_string()>{name}</span>
                                {body}
                            </div>
                        }
                    }).collect_view();
                    view! { {empty}{rows} }
                }}
            </div>
        </section>
    }
}

#[component]
fn BmTable(players: Arc<Vec<Value>>, m: Arc<Value>, n: u32) -> impl IntoView {
    let messages = get(&m, "messages");
    let tally = |p: &Value| {
        let toon = text(get(p, "ToonHandle")).unwrap_or("");
        (
            count(get(p, "taunts")),
            count(get(p, "dances")),
            count(get(p, "sprays")),
            count(get(p, "voiceLines")),
            pings(messages, toon),
        )
    };
    if !players.iter().any(|p| {
        let t = tally(p);
        t.0 + t.1 + t.2 + t.3 + t.4 > 0
    }) {
        return None;
    }
    let rows = players.iter().map(|p| {
        let (taunts, dances, sprays, voice, pings) = tally(p);
        let team = if is_num(get(p, "team"), 1.0) { 1 } else { 0 };
        let hero = text(get(p, "hero")).unwrap_or("").to_string();
        view! {
            <tr>
                <td><span class="ds-who" style="min-width: 0"><Portrait hero=hero.clone() size=24 tone=tone_of(team) /><span>{hero}</span></span></td>
                <td class="ds-num">{taunts}</td><td class="ds-num">{dances}</td><td class="ds-num">{sprays}</td>
                <td class="ds-num">{voice}</td><td class="ds-num">{pings}</td>
            </tr>
        }
    }).collect_view();
    Some(view! {
        <section class="ds-sec">
            <SecHead num=n title="Taunts & pings" />
            <div class="ds-panel flush scroll">
                <table class="ds-table">
                    <thead><tr><th>"Player"</th><th>"Taunts"</th><th>"Dances"</th><th>"Sprays"</th><th>"Voice"</th><th>"Pings"</th></tr></thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        </section>
    })
}

#[component]
fn LevelAdvantage(svg: storm_web_core::matchdetail::LevelAdvSvg) -> impl IntoView {
    let (w, h, mid) = (svg.w, svg.h, svg.mid);
    view! {
        <svg class="ds-lvl" viewBox=format!("0 0 {w} {h}") preserveAspectRatio="none" role="img" aria-label="Level advantage over time">
            <defs>
                <clipPath id="lvl-up"><rect x="0" y="0" width=w height=mid></rect></clipPath>
                <clipPath id="lvl-dn"><rect x="0" y=mid width=w height=mid></rect></clipPath>
            </defs>
            <line x1="0" x2=w y1=mid y2=mid class="axis"></line>
            {svg.ticks.iter().map(|x| view! { <line x1=*x x2=*x y1=h - 6.0 y2=h class="axis"></line> }).collect_view()}
            <path d=format!("{} Z", svg.d) class="up" clip-path="url(#lvl-up)"></path>
            <path d=format!("{} Z", svg.d) class="dn" clip-path="url(#lvl-dn)"></path>
            <path d=svg.d.clone() class="line"></path>
            {svg.marks.iter().map(|(x, mk)| view! { <line x1=*x x2=*x y1="4" y2=h - 4.0 class="mark"><title>{mk.label.clone()}</title></line> }).collect_view()}
            {svg.rects.iter().map(|(x, rw, title)| view! { <rect x=*x width=*rw y="0" height=h fill="transparent"><title>{title.clone()}</title></rect> }).collect_view()}
        </svg>
        <div class="ds-lvl-legend">
            <span>"0:00"</span>
            {svg.marks.iter().map(|(_, mk)| view! { <span><b>"┆"</b>{format!(" {} · {}", mk.label, fmt_dur(Some(mk.t)))}</span> }).collect_view()}
            <span>{fmt_dur(Some(svg.length))}</span>
        </div>
    }
}

/// What the page reads of the match, computed once.
struct Ctx {
    m: Value,
    by_toon: Value,
    players: Vec<Value>,
    id: i64,
    me_toon: Option<String>,
    my_team: Option<i64>,
}

impl Ctx {
    /// `side`: US / THEM from the operator's point of view, BLUE / RED without an operator, a dash when unknown.
    fn side(&self, t: &Value) -> String {
        match t.as_f64() {
            Some(t) if t >= 0.0 => match self.my_team {
                None => (if t == 0.0 { "BLUE" } else { "RED" }).into(),
                Some(mt) => (if t == mt as f64 { "US" } else { "THEM" }).into(),
            },
            _ => "—".into(),
        }
    }
    fn team_label(&self, t: i64) -> &'static str {
        match self.my_team {
            None => {
                if t == 0 {
                    "Blue team"
                } else {
                    "Red team"
                }
            }
            Some(mt) => {
                if t == mt {
                    "Your team"
                } else {
                    "Enemy team"
                }
            }
        }
    }
    /// Operator's team first, else blue first.
    fn order(&self) -> [usize; 2] {
        match self.my_team {
            None => [0, 1],
            Some(t) => [t as usize, 1 - t as usize],
        }
    }
}

fn side_cls(t: &Value) -> &'static str {
    match t.as_f64() {
        Some(t) if t >= 0.0 => {
            if t == 0.0 {
                "ds-us"
            } else {
                "ds-them"
            }
        }
        _ => "",
    }
}

#[component]
pub fn MatchDetail() -> impl IntoView {
    let params = use_params_map();
    let adv = RwSignal::new(false);
    let replay = RwSignal::new(false);
    let operator = use_operator();
    let talents = use_talents();
    let attrs = fetch::<HashMap<String, String>>(|| "/api/dim/hero-attributes".to_string());
    let data = fetch::<Value>(move || {
        format!(
            "/api/matches/{}",
            uri_component(&params.read().get("id").unwrap_or_default())
        )
    });
    move || {
        match data.get() {
        None => view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"loading…"</div></div> }.into_any(),
        Some(Ok(d)) if d.is_object() => {
            let names: Vec<String> = operator.get_untracked().iter().map(|n| n.to_lowercase()).collect();
            let players: Vec<Value> = get(&d, "players").as_object().map(|o| o.values().cloned().collect()).unwrap_or_default();
            let me = players.iter().find(|p| names.contains(&text(get(p, "name")).unwrap_or("").to_lowercase()));
            let ctx = Arc::new(Ctx {
                m: get(&d, "match").clone(),
                by_toon: get(&d, "players").clone(),
                id: num(get(&d, "id")) as i64,
                me_toon: me.and_then(|p| text(get(p, "ToonHandle"))).map(str::to_string),
                my_team: me.and_then(|p| get(p, "team").as_f64()).map(|t| t as i64),
                players,
            });
            header_and_tabs(ctx, adv, replay, talents, attrs).into_any()
        }
        Some(_) => view! {
            <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"Match not found. "<Link to="/matches">"Back to matches"</Link></div></div>
        }.into_any(),
    }
    }
}

fn header_and_tabs(
    ctx: Arc<Ctx>,
    adv: RwSignal<bool>,
    replay: RwSignal<bool>,
    talents: RwSignal<HashMap<String, DimTalent>>,
    attrs: LocalResource<Result<HashMap<String, String>, String>>,
) -> impl IntoView {
    let m = &ctx.m;
    let winner = get(m, "winner");
    let won = ctx.my_team.is_some_and(|t| is_num(winner, t as f64));
    let order = ctx.order();
    let lvl = |t: usize| -> f64 {
        get(get(m, "levelTimes"), &t.to_string())
            .as_object()
            .map(|o| {
                o.keys()
                    .filter_map(|k| k.parse::<f64>().ok())
                    .fold(0.0, f64::max)
            })
            .unwrap_or(0.0)
    };
    let tk = |t: usize| num(get(m, &format!("team{t}Takedowns")));
    let map = text(get(m, "map")).unwrap_or("").to_string();
    let header_style = if has_map_art(&map) {
        map_art(&map)
    } else {
        String::new()
    };
    let verdict = match ctx.my_team {
        Some(_) => view! { <span class=format!("ds-verdict {}", if won { "w" } else { "l" })>{if won { "VICTORY" } else { "DEFEAT" }}</span> }.into_any(),
        None => view! { <span class="ds-verdict n">{if is_num(winner, 0.0) { "BLUE" } else { "RED" }}" TEAM WINS"</span> }.into_any(),
    };
    let (first_obj, first_fort, first_keep) = (
        get(m, "firstObjective"),
        get(m, "firstFort"),
        get(m, "firstKeep"),
    );
    let cls = |t: usize| side_cls(&Value::from(t as f64));
    let version = get(get(m, "version"), "m_build");
    let build = if version.is_null() {
        String::new()
    } else {
        version.to_string()
    };
    let raw_href = format!("/api/matches/{}/raw?stream=tracker", ctx.id);
    let tab_ctx = ctx.clone();
    view! {
        <div class="ds-page">
            <header class="ds-mhead" style=header_style>
                <div class="ds-kicker"><ModeTag mode=get(m, "mode").as_i64() />" "{net::fmt_time(text(get(m, "date")))}" · "{fmt_dur(Some(num(get(m, "length"))))}" · build "{build}</div>
                <h1 class="ds-title" style="cursor: default">{map}</h1>
                {verdict}
                <div class="ds-score">
                    <div><span class="ds-label">"Takedowns"</span><strong class=cls(order[0])>{tk(order[0])}</strong><span>"–"</span><strong class=cls(order[1])>{tk(order[1])}</strong></div>
                    <div><span class="ds-label">"Team level"</span><strong class=cls(order[0])>{lvl(order[0])}</strong><span>"–"</span><strong class=cls(order[1])>{lvl(order[1])}</strong></div>
                    <div><span class="ds-label">"First objective"</span><strong class=side_cls(first_obj)>{ctx.side(first_obj)}</strong></div>
                    <div><span class="ds-label">"First fort"</span><strong class=side_cls(first_fort)>{ctx.side(first_fort)}</strong></div>
                    {first_keep.as_f64().is_some_and(|k| k >= 0.0).then(|| view! { <div><span class="ds-label">"First keep"</span><strong class=side_cls(first_keep)>{ctx.side(first_keep)}</strong></div> })}
                </div>
            </header>

            <div class="ds-tabs">
                <span class=move || pill(!replay.get()) on:click=move |_| replay.set(false)>"Score"</span>
                <span class=move || pill(replay.get()) on:click=move |_| replay.set(true)>"Replay 2D"</span>
                <a class="ds-pill" style="margin-left: auto" href=raw_href target="_blank" rel="noreferrer">"raw dump ›"</a>
            </div>

            {move || if replay.get() {
                view! { <div style="margin-top: 16px"><crate::replay::Replay2D id=ctx.id.to_string() /></div> }.into_any()
            } else {
                score_tab(tab_ctx.clone(), adv, talents, attrs).into_any()
            }}
        </div>
    }
}

/// Score tab. Sections are numbered in order like React: a section that renders nothing still takes its number.
fn score_tab(
    ctx: Arc<Ctx>,
    adv: RwSignal<bool>,
    talents: RwSignal<HashMap<String, DimTalent>>,
    attrs: LocalResource<Result<HashMap<String, String>, String>>,
) -> impl IntoView {
    let m = &ctx.m;
    let order = ctx.order();
    let winner = get(m, "winner");
    let bans = get(m, "bans");
    let picks_v = get(m, "picks");
    let length = num(get(m, "length"));
    let players = Arc::new(ctx.players.clone());
    let refs: Vec<&Value> = ctx.players.iter().collect();
    let maxes = col_maxes(&refs);
    let fort = {
        let mut forts: Vec<&Value> = get(m, "structures")
            .as_object()
            .map(|o| {
                o.values()
                    .filter(|s| some(get(s, "destroyed")) && text(get(s, "name")) == Some("Fort"))
                    .collect()
            })
            .unwrap_or_default();
        forts.sort_by(|a, b| num(get(a, "destroyed")).total_cmp(&num(get(b, "destroyed"))));
        forts.first().copied()
    };
    let marks: Vec<Mark> = fort
        .map(|f| {
            let owner = if is_num(get(f, "team"), 0.0) {
                1.0
            } else {
                0.0
            };
            vec![Mark {
                t: num(get(f, "destroyed")),
                label: format!(
                    "first fort · {} {}",
                    ctx.side(&Value::from(owner)).to_lowercase(),
                    if ctx.my_team.is_none() {
                        "team takes it"
                    } else {
                        "take it"
                    }
                ),
            }]
        })
        .unwrap_or_default();
    let has_draft = truthy(picks_v) || count(idx(bans, 0)) > 0 || count(idx(bans, 1)) > 0;
    let mut sec = 0u32;
    let mut next = || {
        sec += 1;
        sec
    };

    let draft = has_draft.then(|| {
        let n = next();
        let first_pick = get(m, "firstPickWin");
        let sub = format!("bans struck through · picks in team order{}", if some(first_pick) { format!(" · first pick {}", if first_pick.as_bool().unwrap_or(false) { "won" } else { "lost" }) } else { String::new() });
        let sides = order.iter().map(|&t| {
            let ps: Vec<String> = arr(idx(picks_v, t)).iter().filter_map(|x| text(x).map(str::to_string)).collect();
            let in_team: Vec<&Value> = ctx.players.iter().filter(|p| is_num(get(p, "team"), t as f64)).collect();
            let picks: Vec<String> = if ps.is_empty() { in_team.iter().map(|p| text(get(p, "hero")).unwrap_or("").to_string()).collect() } else { ps };
            let team_bans = arr(idx(bans, t)).to_vec();
            let ban_view = (!team_bans.is_empty()).then(|| view! {
                <div class="ds-dbans">
                    {team_bans.into_iter().map(|b| {
                        let phase = ban_phase(&b);
                        move || {
                            let a = attrs.get().and_then(Result::ok).unwrap_or_default();
                            match ban_hero(&b, &a) {
                                Some(h) => view! { <span class="ds-dban"><Portrait hero=h size=40 tone=Tone::Ban /><em>{phase}</em></span> }.into_any(),
                                None => view! { <span class="ds-dban"><span class="ds-tag award" style="margin-top: 12px">"no ban"</span></span> }.into_any(),
                            }
                        }
                    }).collect_view()}
                </div>
            });
            let first = is_num(get(picks_v, "first"), t as f64);
            let pick_views = picks.iter().map(|h| {
                let p = in_team.iter().find(|x| text(get(x, "hero")) == Some(h.as_str())).copied();
                let toon = p.and_then(|p| text(get(p, "ToonHandle"))).map(str::to_string);
                let you = toon.is_some() && toon == ctx.me_toon;
                let inner = view! {
                    <Portrait hero=h.clone() size=52 tone=tone_of(t as i64) />
                    <div><strong>{h.clone()}</strong><em>{p.and_then(|p| text(get(p, "name"))).unwrap_or("").to_string()}</em></div>
                    {you.then(|| view! { <span class="ds-tag you">"YOU"</span> })}
                };
                match toon {
                    Some(tn) => view! { <Link to=format!("/player/{}", uri_component(&tn)) class="ds-dpick">{inner}</Link> }.into_any(),
                    None => view! { <a href="#" class="ds-dpick">{inner}</a> }.into_any(),
                }
            }).collect_view();
            view! {
                <div class=format!("ds-dside {}", if t == 0 { "blue" } else { "red" })>
                    <div class="ds-dside-hd">
                        <span class=format!("ds-label {}", if t == 0 { "ds-us" } else { "ds-them" }) style="margin: 0">{format!("{} · {}", ctx.team_label(t as i64), if t == 0 { "blue" } else { "red" })}</span>
                        {first.then(|| view! { <span class="ds-tag fp">"FIRST PICK"</span> })}
                    </div>
                    {ban_view}
                    <div class="ds-dpicks">{pick_views}</div>
                </div>
            }
        }).collect_view();
        view! {
            <section class="ds-sec">
                <SecHead num=n title="Draft" sub=sub />
                <div class="ds-draft">{sides}</div>
            </section>
        }
    });
    let board_n = next();
    let tables = order.iter().map(|&t| view! {
        <ScoreTable players=players.clone() team=t as i64 label=ctx.team_label(t as i64) adv=adv me_toon=ctx.me_toon.clone() maxes=maxes.clone() talents=talents />
    }).collect_view();
    let level = {
        let tl = arr(get(m, "levelAdvTimeline"));
        (tl.len() > 1).then(|| {
            let n = next();
            let side_team = if ctx.my_team == Some(1) { 1 } else { 0 };
            let sub = if ctx.my_team.is_none() { "above the line = blue team ahead" } else { "above the line = you are ahead" };
            let svg = level_adv(tl, length, side_team, &marks);
            view! {
                <section class="ds-sec">
                    <SecHead num=n title="Level advantage" sub=sub />
                    <div class="ds-panel">{svg.map(|svg| view! { <LevelAdvantage svg=svg /> })}</div>
                </section>
            }
        })
    };
    let xp = {
        let data = arr(get(m, "XPBreakdown"));
        (data.len() > 1).then(|| {
            let n = next();
            let (blue, red) = xp_lines(data).unwrap_or_default();
            view! {
                <section class="ds-sec">
                    <SecHead num=n title="Team XP">
                        <span class="ds-us">"blue"</span>" / "<span class="ds-them">"red"</span>
                    </SecHead>
                    <div class="ds-panel">
                        <svg width="100%" height="130" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="Team XP over time">
                            <polyline points=blue fill="none" stroke="var(--tm-blue)" stroke-width="2" vector-effect="non-scaling-stroke"></polyline>
                            <polyline points=red fill="none" stroke="var(--tm-red)" stroke-width="2" vector-effect="non-scaling-stroke"></polyline>
                        </svg>
                    </div>
                </section>
            }
        })
    };
    let timeline_n = next();
    let chat_n = next();
    let bm_n = next();
    let full_n = next();
    let kills_total = count(get(m, "takedowns"));
    let blue_wins = is_num(winner, 0.0);
    let raw_href = format!("/api/matches/{}/raw?stream=tracker", ctx.id);
    let (mm, by_toon) = (Arc::new(ctx.m.clone()), Arc::new(ctx.by_toon.clone()));
    view! {
        {draft}
        <section class="ds-sec">
            <SecHead num=board_n title="Scoreboard">
                <span class=move || pill(adv.get()) on:click=move |_| adv.update(|a| *a = !*a)>{move || if adv.get() { "− basic stats" } else { "+ advanced stats" }}</span>
            </SecHead>
            {tables}
        </section>
        {level}
        {xp}
        <MatchTimeline m=mm.clone() players=by_toon.clone() n=timeline_n />
        <ChatLog m=mm.clone() players=by_toon n=chat_n />
        <BmTable players=players m=mm n=bm_n />
        <section class="ds-sec">
            <SecHead num=full_n title="Full data" />
            <div class="ds-panel flush">
                <div class="ds-kv"><span>"Winner"</span><span class=if blue_wins { "ds-us" } else { "ds-them" }>{if blue_wins { "Blue team" } else { "Red team" }}</span></div>
                <div class="ds-kv"><span>"Takedowns"</span><span class="ds-num">{kills_total}</span></div>
                <div class="ds-kv"><span>"Raw decoded dump"</span><a style="color: var(--accent-2)" href=raw_href target="_blank" rel="noreferrer">"tracker events ›"</a></div>
            </div>
        </section>
    }
}
