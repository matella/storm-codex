//! Scouting report: the "dossier" page of one report. Port of `web/src/pages/ScoutingReport.tsx` and of the `ds`
//! components it uses. The HTML export is not ported yet (see docs/STATUS.md).

use std::{collections::HashMap, sync::Arc};

use leptos::{ev, html, prelude::*, task::spawn_local};
use leptos_router::hooks::{use_navigate, use_params_map};
use serde_json::Value;
use storm_web_core::{
    net,
    players::ChatHabits,
    route::uri_component,
    scouting::{
        compact_evidence, day, fmt_len, fmt_rate, humanize, kn, map_anchor, maps_with_plans, pct,
        pid_names, upload_label, Analysis, Count, Evidence, Facts, GameRow, HeroCall, HeroRow,
        MapCall, MapFacts, MapPlan, Point, Rate, ScoutGame, ScoutingReport as Report,
    },
    session::to_fixed1,
};

use crate::ui::{fetch, map_art, Link, Portrait, SecHead, Tone};

type Names = Arc<HashMap<String, String>>;

fn fixed0(x: f64) -> String {
    format!("{}", (x + 0.5).floor() as i64)
}

fn pct_or_dash(r: Option<Rate>) -> impl IntoView {
    match pct(r) {
        Some(p) => view! { {p}<span>"%"</span> }.into_any(),
        None => "—".into_any(),
    }
}

// ── ds components ───────────────────────────────────────────────────────────

#[component]
fn Pips(games: Vec<bool>) -> impl IntoView {
    view! { <span class="ds-pips">{games.into_iter().map(|w| view! { <i class=if w { "w" } else { "l" }></i> }).collect_view()}</span> }
}

#[component]
fn Meter(conf: Option<String>) -> impl IntoView {
    let lv = match conf.as_deref() {
        Some("high") => 3,
        Some("medium") => 2,
        Some("low") => 1,
        _ => 0,
    };
    let conf = conf.unwrap_or_default();
    (lv > 0).then(|| {
        view! {
            <span class="ds-meter" title=format!("confidence: {conf}")>
                {(1..=3).map(|n| { let on = if lv >= n { "on" } else { "" }; view! { <i class=on></i> } }).collect_view()}
                <em>{format!("{conf} confidence")}</em>
            </span>
        }
    })
}

#[component]
fn EvLine(
    ev: Vec<Evidence>,
    unsupported: bool,
    names: Names,
    map: Option<String>,
) -> impl IntoView {
    if unsupported {
        return Some(
            view! { <div class="ds-ev bad">"⚠ unverified — no supporting fact"</div> }.into_any(),
        );
    }
    let known: Vec<&Evidence> = ev.iter().filter(|e| e.known).take(2).collect();
    let unknown: Vec<&Evidence> = ev.iter().filter(|e| !e.known).collect();
    if known.is_empty() && unknown.is_empty() {
        return None;
    }
    let spans = known
        .into_iter()
        .map(|e| {
            let (label, text) = compact_evidence(e, &names, map.as_deref());
            view! { <span class=if e.low == Some(true) { "low" } else { "" } title=e.id.clone()>{format!("{label} ")}<b>{text}</b></span> }
        })
        .collect_view();
    let warn = (!unknown.is_empty()).then(|| {
        let ids = unknown.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ");
        let n = unknown.len();
        view! { <span class="bad" title=ids>{format!("⚠ {n} unknown fact id{}", if n > 1 { "s" } else { "" })}</span> }
    });
    Some(view! { <div class="ds-ev">{spans}{warn}</div> }.into_any())
}

#[component]
fn CallCard(h: HeroCall, tone: Tone, names: Names, map: Option<String>) -> impl IntoView {
    let tag = match tone {
        Tone::Ban => {
            let t = match h.phase.as_deref() {
                Some("mid") => "MID BAN",
                Some("first") => "1ST PHASE",
                _ => "BAN",
            };
            Some(view! { <span class="ds-tag ban">{t}</span> }.into_any())
        }
        Tone::Them => h.player.clone().map(|p| {
            let shown = names.get(&p).cloned().unwrap_or(p);
            view! { <span class="ds-tag them">{shown}</span> }.into_any()
        }),
        _ => None,
    };
    view! {
        <div class="ds-call">
            <Portrait hero=h.hero.clone() size=64 tone=tone />
            <div>
                <div class="ds-call-hd"><strong>{h.hero.clone()}</strong>{tag}</div>
                <p>{humanize(&h.why, &names)}</p>
                <EvLine ev=h.evidence.clone() unsupported=h.unsupported names=names.clone() map=map />
            </div>
        </div>
    }
}

#[component]
fn Lane(
    title: &'static str,
    kind: &'static str,
    empty: &'static str,
    count: usize,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=format!("ds-lane {kind}")>
            <div class="ds-lane-hd">{title}</div>
            {if count > 0 { children().into_any() } else { view! { <div class="ds-lane-empty">{empty}</div> }.into_any() }}
        </div>
    }
}

#[component]
fn Consider(
    points: Vec<Point>,
    names: Names,
    #[prop(optional)] map: Option<String>,
) -> impl IntoView {
    (!points.is_empty()).then(|| {
        view! {
            <div class="ds-consider-wrap">
                <div class="ds-lane-hd">"Consider"</div>
                <ul class="ds-consider">
                    {points.into_iter().map(|p| view! {
                        <li><span>{humanize(&p.point, &names)}</span><EvLine ev=p.evidence unsupported=p.unsupported names=names.clone() map=map.clone() /></li>
                    }).collect_view()}
                </ul>
            </div>
        }
    })
}

fn none_dash() -> AnyView {
    view! { <span class="ds-none">"—"</span> }.into_any()
}

#[component]
fn Chips(list: Vec<Count>, #[prop(default = Tone::Plain)] tone: Tone) -> impl IntoView {
    if list.is_empty() {
        return none_dash();
    }
    list.into_iter()
        .take(6)
        .map(|c| view! {
            <span class="ds-chip" title=c.id.clone()><Portrait hero=c.key.clone() size=26 tone=tone /><span>{c.key.clone()}</span><b>{format!("{}×", c.count.k)}</b></span>
        })
        .collect_view()
        .into_any()
}

#[component]
fn TextChips(list: Vec<Count>) -> impl IntoView {
    if list.is_empty() {
        return none_dash();
    }
    list.into_iter()
        .map(|c| view! { <span class="ds-chip text"><span>{c.key.clone()}</span><b>{format!("{}×", c.count.k)}</b></span> })
        .collect_view()
        .into_any()
}

#[component]
fn PickChips(list: Vec<HeroRow>, names: Names) -> impl IntoView {
    if list.is_empty() {
        return none_dash();
    }
    list.into_iter()
        .map(|h| {
            let mut who: Vec<String> = Vec::new();
            for p in &h.by {
                let n = names.get(p).cloned().unwrap_or_else(|| p.clone());
                if !who.contains(&n) {
                    who.push(n);
                }
            }
            let who_view =
                (!who.is_empty()).then(|| view! { <em>{format!(" {}", who.join(", "))}</em> });
            view! {
                <span class="ds-chip" title=h.id.clone()>
                    <Portrait hero=h.hero.clone() size=26 />
                    <span>{h.hero.clone()}{who_view}</span>
                    <b>{format!("{}×", h.picks.k)}</b>
                </span>
            }
        })
        .collect_view()
        .into_any()
}

/// Cards of a list of hero calls.
fn calls(tone: Tone, list: &[HeroCall], names: &Names, map: Option<&str>) -> impl IntoView {
    list.iter()
        .map(|h| view! { <CallCard h=h.clone() tone=tone names=names.clone() map=map.map(str::to_string) /> })
        .collect_view()
}

// ── cover and banners ───────────────────────────────────────────────────────

#[component]
fn Cover(r: Signal<Report>, on_patch: Callback<Value>, children: Children) -> impl IntoView {
    let editing = RwSignal::new(false);
    let title = RwSignal::new(String::new());
    let target = RwSignal::new(String::new());
    let start = move || {
        let rep = r.get_untracked();
        title.set(rep.title.clone());
        target.set(rep.target_name.clone().unwrap_or_default());
        editing.set(true);
    };
    let save = move || {
        editing.set(false);
        let rep = r.get_untracked();
        let (t, tg) = (title.get_untracked(), target.get_untracked());
        if !t.trim().is_empty()
            && (t != rep.title || tg != rep.target_name.clone().unwrap_or_default())
        {
            on_patch.run(serde_json::json!({ "title": t, "target_name": tg }));
        }
    };
    let keys = move |e: ev::KeyboardEvent| match e.key().as_str() {
        "Enter" => save(),
        "Escape" => editing.set(false),
        _ => {}
    };
    let facts = move || r.get().snapshot.map(|s| s.facts);
    let bg = move || {
        facts()
            .and_then(|f| f.maps.first().map(|m| map_art(&m.map)))
            .unwrap_or_default()
    };
    let children = children();
    view! {
        <header class="ds-cover" style=bg>
            <div class="ds-kicker">{move || format!("Scouting dossier · report #{}", r.get().id)}</div>
            {move || if editing.get() {
                view! {
                    <div class="ds-edit">
                        <input autofocus class="ds-input big" placeholder="team name" prop:value=move || target.get()
                            on:input=move |e| target.set(event_target_value(&e)) on:keydown=keys />
                        <input class="ds-input" style="flex: 2; min-width: 220px" placeholder="report title" prop:value=move || title.get()
                            on:input=move |e| title.set(event_target_value(&e)) on:keydown=keys />
                        <span class="ds-btn primary" on:click=move |_| save()>"Save"</span>
                        <span class="ds-btn" on:click=move |_| editing.set(false)>"Cancel"</span>
                    </div>
                }.into_any()
            } else {
                let rep = r.get();
                view! {
                    <h1 class="ds-title" title="Click to rename" on:click=move |_| start()>{rep.target_name.clone().unwrap_or_else(|| "Unnamed team".to_string())}</h1>
                    <div class="ds-subtitle" title="Click to rename" on:click=move |_| start()>{rep.title.clone()}<span class="edit">"✎ RENAME"</span></div>
                }.into_any()
            }}
            {move || {
                let f = facts();
                let o = f.as_ref().map(|f| f.overview.clone());
                let games: Vec<bool> = f.as_ref().map(|f| f.games.iter().map(|g| g.won).collect()).unwrap_or_default();
                let first_pick = f.as_ref().map(|f| f.draft.first_pick);
                let record = match &o {
                    Some(o) => view! { {o.record.k}<span>"–"</span>{o.record.n - o.record.k} }.into_any(),
                    None => "—".into_any(),
                };
                let n_games = o.as_ref().map_or(0, |o| o.games);
                let range = match &o {
                    Some(o) if o.games > 0 => format!("{} → {}", day(o.first_date.as_deref()), day(o.last_date.as_deref())),
                    _ => "add replays below".to_string(),
                };
                let avg = match &o {
                    Some(o) if o.games > 0 => fmt_len(Some(o.avg_length_s)),
                    _ => "—".to_string(),
                };
                let builds = o.as_ref().map(|o| o.builds.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")).filter(|b| !b.is_empty()).unwrap_or_else(|| "?".to_string());
                let sample = o.as_ref().filter(|o| o.games > 0 && o.games < 5).map(|o| {
                    format!("⚠ Small sample — {} game{}. Treat trends as hints, not certainties.", o.games, if o.games == 1 { "" } else { "s" })
                });
                let roster: Vec<_> = f.as_ref().map(|f| f.players.iter().filter(|p| p.core).cloned().collect()).unwrap_or_default();
                view! {
                    <div class="ds-statline">
                        <div class="ds-stat big"><span class="ds-label">"Record"</span><strong>{record}</strong><Pips games=games /></div>
                        <div class="ds-stat"><span class="ds-label">"Games"</span><strong>{n_games}</strong><small>{range}</small></div>
                        <div class="ds-stat"><span class="ds-label">"Had first pick"</span><strong>{pct_or_dash(first_pick)}</strong><small>{kn(first_pick)}</small></div>
                        <div class="ds-stat"><span class="ds-label">"Avg length"</span><strong>{avg}</strong><small>{format!("builds {builds}")}</small></div>
                        {sample.map(|s| view! { <div class="ds-sample">{s}</div> })}
                    </div>
                    {(!roster.is_empty()).then(|| view! {
                        <div class="ds-roster">
                            {roster.into_iter().map(|p| view! {
                                <div class="ds-rm">
                                    {p.heroes.first().map(|h| view! { <Portrait hero=h.hero.clone() size=58 /> })}
                                    <div>
                                        <strong>{p.name.clone()}</strong>
                                        <span>{p.roles.first().map(|r| r.key.clone()).unwrap_or_default()}</span>
                                        <em>{p.heroes.iter().take(3).map(|h| h.hero.clone()).collect::<Vec<_>>().join(" · ")}</em>
                                    </div>
                                </div>
                            }).collect_view()}
                        </div>
                    })}
                }
            }}
            {children}
        </header>
    }
}

#[component]
fn Banners(r: Signal<Report>) -> impl IntoView {
    move || {
        let rep = r.get();
        let snap = rep.snapshot.clone();
        let games = snap.as_ref().map_or(0, |s| s.facts.overview.games);
        let excluded = snap.as_ref().map_or(0, |s| s.facts.overview.excluded);
        let ambiguous = snap
            .as_ref()
            .is_some_and(|s| s.detection.ambiguous && s.roster_auto)
            && rep.anchors.is_empty()
            && !rep.games.is_empty();
        view! {
            {ambiguous.then(|| view! {
                <div class="ds-banner">
                    "These replays don't tell which team to scout (the same players keep facing each other). Set an "<a href="#roster">"anchor player"</a>" from the team you're scouting."
                </div>
            })}
            {(snap.is_some() && excluded > 0).then(|| view! {
                <div class="ds-banner">
                    {format!("{excluded} replay{} left out: the target team wasn't found (fewer than 3 roster players and no anchor).", if excluded > 1 { "s" } else { "" })}
                    " "<a href="#games">"Pick the side by hand"</a>" or adjust the roster."
                </div>
            })}
            {(rep.status == storm_web_core::scouting::ReportStatus::Stale).then(|| view! {
                <div class="ds-banner">
                    {format!("The draft plan was written on facts v{}; replays or roster changed since (now v{}). Copy the pack again and re-import to refresh it.",
                        rep.analysis_facts_version.map_or("undefined".to_string(), |v| v.to_string()), rep.facts_version)}
                </div>
            })}
            {(rep.analysis.is_none() && snap.is_some() && games > 0).then(|| view! {
                <div class="ds-banner" style="color: var(--accent-2); border-color: rgba(127,119,221,.35); background: rgba(127,119,221,.06)">
                    "No draft plan yet — "<b>"Copy pack"</b>", paste it into your LLM, then "<b>"Import analysis"</b>". The numbers below come straight from the replays."
                </div>
            })}
        }
    }
}

// ── sections ────────────────────────────────────────────────────────────────

#[component]
fn Identity(r: Report, facts: Facts, names: Names, num: u32) -> impl IntoView {
    let d = facts.draft.clone();
    let quote = match r.analysis.as_ref().map(|a| a.summary.as_str()).filter(|s| !s.is_empty()) {
        Some(s) => view! { <blockquote class="ds-quote">{humanize(s, &names)}</blockquote> }.into_any(),
        None => view! { <blockquote class="ds-quote muted">"No draft plan imported yet — the numbers come straight from the replays."</blockquote> }.into_any(),
    };
    let tally = r.analysis_tally.clone().map(|t| {
        view! { <div class="ds-tally">{format!("{} recommendations · {} unsupported · {} unknown fact ids", t.claims, t.unsupported, t.unknown_ids)}</div> }
    });
    view! {
        <section class="ds-sec" id="identity">
            <SecHead num=num title="Draft identity" />
            <div class="ds-identity">
                <div>{quote}{tally}</div>
                <div class="ds-tells">
                    <div class="ds-tell"><span class="ds-label">"They ban first"</span><div class="ds-chips"><Chips list=d.bans_first.clone() tone=Tone::Ban /></div></div>
                    <div class="ds-tell"><span class="ds-label">"They ban mid-draft"</span><div class="ds-chips"><Chips list=d.bans_mid.clone() tone=Tone::Ban /></div></div>
                    <div class="ds-tell"><span class="ds-label">"Their first pick"</span><div class="ds-chips"><Chips list=d.openers.clone() tone=Tone::Them /></div></div>
                    <div class="ds-tell"><span class="ds-label">"Their last pick"</span><div class="ds-chips"><Chips list=d.last_picks.clone() tone=Tone::Them /></div></div>
                    <div class="ds-tell"><span class="ds-label">"Banned against them"</span><div class="ds-chips"><Chips list=d.bans_against.clone() /></div></div>
                    <div class="ds-tell"><span class="ds-label">{format!("Role of their first pick · first pick {}", fmt_rate(Some(d.first_pick)))}</span>
                        <div class="ds-chips"><TextChips list=d.opener_roles.clone() /></div></div>
                </div>
            </div>
        </section>
    }
}

#[component]
fn MapChoice(a: Analysis, names: Names, num: u32) -> impl IntoView {
    let tile = |m: MapCall, kind: &'static str| {
        view! {
            <a class=format!("ds-mtile {kind}") href=format!("#{}", map_anchor(&m.map)) style=map_art(&m.map)>
                <span class="badge">{if kind == "go" { "PICK" } else { "AVOID" }}</span>
                <strong>{m.map.clone()}</strong>
                <p>{humanize(&m.why, &names)}</p>
                <EvLine ev=m.evidence.clone() unsupported=m.unsupported names=names.clone() map=Some(m.map.clone()) />
            </a>
        }
    };
    let c = a.map_choice;
    view! {
        <section class="ds-sec" id="map-choice">
            <SecHead num=num title="Map choice" sub="when we get to pick the map" />
            <div class="ds-mtiles">
                {c.pick.into_iter().map(|m| tile(m, "go")).collect_view()}
                {c.avoid.into_iter().map(|m| tile(m, "no")).collect_view()}
            </div>
        </section>
    }
}

#[component]
fn MapBoard(
    map: String,
    m: Option<MapFacts>,
    plan: Option<MapPlan>,
    facts: Facts,
    names: Names,
    i: usize,
    has_analysis: bool,
) -> impl IntoView {
    let games: Vec<bool> = facts
        .games
        .iter()
        .filter(|x| x.map == map)
        .map(|x| x.won)
        .collect();
    let rec = match &m {
        Some(m) => view! {
            <span class="rec">{format!("{}–{}", m.record.k, m.record.n - m.record.k)}</span><Pips games=games />
        }
        .into_any(),
        None => view! { <span class="rec none">"not in these replays"</span> }.into_any(),
    };
    let ci = m.as_ref().and_then(|m| m.wilson).map(|w| {
        view! { <span class="ds-ci">{format!("95% CI {}–{}%", fixed0(w[0]), fixed0(w[1]))}</span> }
    });
    let meter = plan
        .as_ref()
        .map(|p| view! { <Meter conf=p.confidence.clone() /> });
    let overview = plan.as_ref().filter(|p| !p.overview.is_empty()).map(|p| {
        view! {
            <p class="ds-overview">{humanize(&p.overview, &names)}</p>
            <EvLine ev=p.evidence.clone() unsupported=p.unsupported names=names.clone() map=Some(map.clone()) />
        }
    });
    let body = match &plan {
        Some(p) => {
            let bans = calls(Tone::Ban, &p.bans, &names, Some(&map));
            let picks = calls(Tone::Pick, &p.picks, &names, Some(&map));
            let theirs = calls(Tone::Them, &p.their_picks, &names, Some(&map));
            view! {
                <div class="ds-board">
                    <Lane title="We ban" kind="ban" empty="no ban suggested" count=p.bans.len()>{bans}</Lane>
                    <Lane title="We pick" kind="pick" empty="no pick suggested" count=p.picks.len()>{picks}</Lane>
                    <Lane title="They will likely pick" kind="them" empty="—" count=p.their_picks.len()>{theirs}</Lane>
                </div>
                <Consider points=p.considerations.clone() names=names.clone() map=map.clone() />
            }
            .into_any()
        }
        None => view! {
            <div class="ds-noplan">
                {if has_analysis { view! { "No plan for this map — see "<a href="#general">"Any map"</a>"." }.into_any() } else { "No draft plan imported yet.".into_any() }}
            </div>
        }
        .into_any(),
    };
    let last = m.map(|m| {
        view! {
            <div class="ds-lasttime">
                <div><span class="ds-label">"They played"</span><div class="ds-chips"><PickChips list=m.picks.clone() names=names.clone() /></div></div>
                <div><span class="ds-label">"They banned"</span><div class="ds-chips"><Chips list=m.bans.clone() tone=Tone::Ban /></div></div>
                <div><span class="ds-label">"Banned against them"</span><div class="ds-chips"><Chips list=m.bans_against.clone() /></div></div>
            </div>
        }
    });
    view! {
        <article class="ds-mboard" id=map_anchor(&map)>
            <div class="ds-mboard-hd" style=map_art(&map)>
                <div>
                    <span class="ds-idx">{format!("MAP {:02}", i + 1)}</span>
                    <h3>{map.clone()}</h3>
                    <div class="ds-mboard-rec">{rec}{ci}</div>
                </div>
                {meter}
            </div>
            {overview}
            {body}
            {last}
        </article>
    }
}

#[component]
fn General(a: Analysis, names: Names, num: u32) -> impl IntoView {
    let g = a.general;
    let bans = calls(Tone::Ban, &g.bans, &names, None);
    let picks = calls(Tone::Pick, &g.picks, &names, None);
    view! {
        <section class="ds-sec" id="general">
            <SecHead num=num title="Any map" sub="applies everywhere, including maps missing from the replays" />
            <div class="ds-board two">
                <Lane title="We ban" kind="ban" empty="—" count=g.bans.len()>{bans}</Lane>
                <Lane title="We pick" kind="pick" empty="—" count=g.picks.len()>{picks}</Lane>
            </div>
            <Consider points=g.considerations.clone() names=names.clone() />
        </section>
    }
}

/// One-line chat habits of an account (compact `PlayerChat`): nothing when the account is not in the corpus.
#[component]
fn ChatLine(toon: String) -> impl IntoView {
    let data = fetch::<ChatHabits>(move || format!("/api/players/{}/chat", uri_component(&toon)));
    move || {
        let d = data.get()?.ok()?;
        if !d.enough {
            return None;
        }
        let summary = if d.per_game < 0.5 {
            "quiet".to_string()
        } else {
            format!(
                "{} msg/game · {}% team chat",
                to_fixed1(d.per_game),
                (d.team_share * 100.0).round() as i64
            )
        };
        Some(
            view! { <div class="ds-pstats" title="From your own archived games with this account">"Chat "<b>{summary}</b>" · from your games"</div> },
        )
    }
}

#[component]
fn Players(facts: Facts, num: u32) -> impl IntoView {
    if facts.players.is_empty() {
        return None;
    }
    Some(view! {
        <section class="ds-sec" id="players">
            <SecHead num=num title="Their players" />
            <div class="ds-pcards">
                {facts.players.into_iter().map(|p| {
                    let max = p.heroes.iter().map(|h| h.picks.k).max().unwrap_or(0).max(1);
                    let roles = p.roles.iter().map(|x| x.key.clone()).collect::<Vec<_>>().join(" / ");
                    let s = p.stats.clone();
                    view! {
                        <div class="ds-pcard">
                            <div class="ds-pcard-hd">
                                {p.heroes.first().map(|h| view! { <Portrait hero=h.hero.clone() size=72 /> })}
                                <div><strong>{p.name.clone()}</strong><span>{format!("{roles}{}", if p.core { "" } else { " · substitute" })}</span><em>{format!("{} wins", fmt_rate(Some(p.record)))}</em></div>
                            </div>
                            {p.heroes.iter().map(|h| view! {
                                <div class="ds-pool-row" title=h.id.clone()>
                                    <Portrait hero=h.hero.clone() size=24 /><span>{h.hero.clone()}</span>
                                    <i class="ds-bar"><b style=format!("width: {}%", ((h.picks.k * 100) as f64 / max as f64 + 0.5).floor() as i64)></b></i>
                                    <em>{format!("{}g · {}%", h.picks.k, pct(Some(h.record)).map_or("null".to_string(), |v| v.to_string()))}</em>
                                </div>
                            }).collect_view()}
                            <div class="ds-pstats">
                                "K/D/A "<b>{format!("{}/{}/{}", to_fixed1(s.kills), to_fixed1(s.deaths), to_fixed1(s.assists))}</b>" · KP "<b>{format!("{}%", fixed0(s.kill_participation_pct))}</b>" · dmg "
                                <b>{fixed0(s.hero_damage_pm)}</b>"/min · heal "<b>{fixed0(s.healing_pm)}</b>"/min · dead "<b>{format!("{}%", to_fixed1(s.time_dead_pct))}</b>
                            </div>
                            <ChatLine toon=p.toon.clone() />
                        </div>
                    }
                }).collect_view()}
            </div>
        </section>
    })
}

#[component]
fn Faced(facts: Facts, num: u32) -> impl IntoView {
    let mut list = facts.draft.faced.clone();
    list.sort_by(|x, y| {
        let (px, py) = (
            pct(Some(x.record)).unwrap_or(0),
            pct(Some(y.record)).unwrap_or(0),
        );
        px.cmp(&py).then(y.picks.k.cmp(&x.picks.k))
    });
    if list.is_empty() {
        return None;
    }
    Some(view! {
        <section class="ds-sec" id="faced">
            <SecHead num=num title="What they faced">
                "heroes picked against them · their record in those games · "<b class="lg-hot">"green"</b>" = they never beat it"
            </SecHead>
            <div class="ds-faced">
                {list.into_iter().map(|h| {
                    let p = pct(Some(h.record)).unwrap_or(0);
                    let class = format!("ds-fc {}", if p == 0 { "hot" } else if p == 100 { "cold" } else { "" });
                    view! { <div class=class title=h.id.clone()><Portrait hero=h.hero.clone() size=44 /><span>{h.hero.clone()}</span><em>{format!("{}/{}", h.record.k, h.record.n)}</em></div> }
                }).collect_view()}
            </div>
        </section>
    })
}

#[component]
fn Flow(facts: Facts, num: u32) -> impl IntoView {
    let fl = facts.flow;
    let tile = |label: String, r: Rate, sub: Option<String>| {
        view! {
            <div class="ds-stat"><span class="ds-label">{label}</span><strong>{pct_or_dash(Some(r))}</strong>
                <small>{format!("{}{}", kn(Some(r)), sub.map(|s| format!(" · {s}")).unwrap_or_default())}</small></div>
        }
    };
    let won = |r: Rate| (r.n > 0).then(|| format!("won {} then", kn(Some(r))));
    view! {
        <section class="ds-sec" id="flow">
            <SecHead num=num title="Game flow" sub="how their games tend to go" />
            <div class="ds-flow">
                {tile("First to level 10".into(), fl.first_to_10, won(fl.first_to_10_record))}
                {tile("First fort".into(), fl.first_fort, won(fl.first_fort_record))}
                {tile("First objective".into(), fl.first_objective, won(fl.first_objective_record))}
                {tile("Comebacks".into(), fl.comebacks, Some("won after trailing by 2+ levels".into()))}
                {tile("Throws".into(), fl.throws, Some("lost after leading by 2+ levels".into()))}
                {fl.level_diff.iter().map(|l| view! {
                    <div class="ds-stat"><span class="ds-label">{format!("Level lead at {} min", l.minute)}</span>
                        <strong>{if l.n > 0 { format!("{}{}", if l.avg >= 0.0 { "+" } else { "" }, to_fixed1(l.avg)) } else { "—".to_string() }}</strong>
                        <small>{format!("{} game{}", l.n, if l.n == 1 { "" } else { "s" })}</small></div>
                }).collect_view()}
                {fl.length.iter().filter(|b| b.record.n > 0).map(|b| tile(format!("Games {}", b.label), b.record, None)).collect_view()}
            </div>
        </section>
    }
}

// ── replays, roster, import ─────────────────────────────────────────────────

#[component]
fn GameLine(
    g: ScoutGame,
    row: Option<GameRow>,
    names: Names,
    on_side: Callback<(ScoutGame, Option<i64>)>,
    on_delete: Callback<ScoutGame>,
) -> impl IntoView {
    let t = g.target_team;
    let won = match (t, g.winner) {
        (Some(t), Some(w)) => Some(w == t),
        _ => None,
    };
    let side = |s: i64| {
        let is_target = t == Some(s);
        let team = &g.teams[s as usize];
        // pick order read from the replay when the game is analysed, otherwise the lobby order
        let order = row.as_ref().and_then(|r| {
            if is_target {
                Some(&r.picks)
            } else if t.is_some() {
                Some(&r.opp_picks)
            } else {
                None
            }
        });
        let heroes: Vec<(String, String)> = match order {
            Some(o) => o
                .iter()
                .map(|p| {
                    let who = if is_target {
                        names
                            .get(&p.player)
                            .cloned()
                            .unwrap_or_else(|| p.player.clone())
                    } else {
                        p.player.clone()
                    };
                    (p.hero.clone(), who)
                })
                .collect(),
            None => team
                .iter()
                .map(|p| (p.hero.clone(), p.name.clone()))
                .collect(),
        };
        let bans: Vec<String> = match (&row, t) {
            (Some(r), Some(_)) => {
                if is_target {
                    r.bans.clone()
                } else {
                    r.bans_against.clone()
                }
            }
            _ => vec![],
        };
        let label = if is_target {
            "Scouted team".to_string()
        } else if t.is_none() {
            format!("Team {}", if s == 0 { "A" } else { "B" })
        } else {
            "Opponent".to_string()
        };
        let g2 = g.clone();
        view! {
            <div class=if is_target { "ds-gl-side target" } else { "ds-gl-side" } title="Set as the scouted team" on:click=move |_| on_side.run((g2.clone(), Some(s)))>
                <span class="ds-label"><span>{label}</span></span>
                <div class="ds-gl-picks">
                    {heroes.into_iter().map(|(h, who)| view! { <span><Portrait hero=h size=30 /><em>{who}</em></span> }).collect_view()}
                </div>
                {(!bans.is_empty()).then(|| view! {
                    <div class="ds-gl-bans"><span class="ds-label">"bans"</span>{bans.into_iter().map(|b| view! { <Portrait hero=b size=22 tone=Tone::Ban /> }).collect_view()}</div>
                })}
            </div>
        }
    };
    let first_pick = match row.as_ref().and_then(|r| r.first_pick) {
        Some(true) => " · they had first pick",
        Some(false) => " · opponent had first pick",
        None => "",
    };
    let (g_auto, g_del) = (g.clone(), g.clone());
    let manual = g.target_source.as_deref() == Some("manual");
    view! {
        <div class=format!("ds-gl {}", match won { Some(true) => "w", Some(false) => "l", None => "" })>
            <div class="ds-gl-meta">
                <strong>{match won { None => "?", Some(true) => "WIN", Some(false) => "LOSS" }}</strong>
                <span>{g.map.clone().unwrap_or_else(|| "?".to_string())}</span>
                <em>{format!("{} · {}{first_pick}", day(g.played_at.as_deref()), fmt_len(g.length_s))}</em>
            </div>
            {side(0)}
            {side(1)}
            <div class="ds-gl-manage">
                {if t.is_some() {
                    view! { <span class="ds-src">{format!("by {}", g.target_source.clone().unwrap_or_default())}</span> }.into_any()
                } else {
                    view! { <span class="ds-src missing">"team not found"</span> }.into_any()
                }}
                <span style="display: flex; gap: 6px">
                    {manual.then(|| view! { <span class="ds-btn small" title="Back to automatic detection" on:click=move |_| on_side.run((g_auto.clone(), None))>"auto"</span> })}
                    <span class="ds-btn small danger" title="Remove from the report" on:click=move |_| on_delete.run(g_del.clone())>"remove"</span>
                </span>
            </div>
        </div>
    }
}

#[component]
fn Games(
    r: Signal<Report>,
    names: Signal<Names>,
    num: Signal<u32>,
    on_changed: Callback<()>,
    flash: Callback<String>,
) -> impl IntoView {
    let queue = RwSignal::new(Vec::<(String, String)>::new());
    let over = RwSignal::new(false);
    let input = NodeRef::<html::Input>::new();
    let upload = move |files: Vec<web_sys::File>| {
        let list: Vec<web_sys::File> = files
            .into_iter()
            .filter(|f| f.name().to_lowercase().ends_with(".stormreplay"))
            .collect();
        if list.is_empty() {
            flash.run("✗ drop .StormReplay files".to_string());
            return;
        }
        queue.set(
            list.iter()
                .map(|f| (f.name(), "waiting".to_string()))
                .collect(),
        );
        let id = r.get_untracked().id;
        spawn_local(async move {
            for (i, f) in list.iter().enumerate() {
                let set = move |s: String| queue.update(|q| q[i].1 = s);
                set("uploading…".to_string());
                let state = match net::read_file_bytes(f).await {
                    Err(_) => "network error".to_string(),
                    Ok(bytes) => {
                        let fname: String = js_sys_encode(&f.name());
                        match net::write_raw(
                            "POST",
                            &format!("/api/scouting/{id}/replays"),
                            None,
                            &[("X-Filename", &fname)],
                            Some(bytes),
                            None,
                        )
                        .await
                        {
                            Err(_) => "network error".to_string(),
                            Ok((401, _)) => {
                                "unauthorized — set the admin token in Admin".to_string()
                            }
                            Ok((s, body)) if (200..300).contains(&s) => {
                                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                                upload_label(
                                    v.get("status").and_then(Value::as_str),
                                    v.get("error_class").and_then(Value::as_str),
                                    v.get("target_team").and_then(Value::as_i64),
                                )
                            }
                            Ok((s, _)) => format!("HTTP {s}"),
                        }
                    }
                };
                set(state);
                on_changed.run(());
            }
        });
    };
    let on_files = move |list: Option<web_sys::FileList>| {
        let files: Vec<web_sys::File> = list
            .map(|l| (0..l.length()).filter_map(|i| l.item(i)).collect())
            .unwrap_or_default();
        upload(files);
    };
    let set_side = Callback::new(move |(g, team): (ScoutGame, Option<i64>)| {
        if team.is_some() && g.target_team == team && g.target_source.as_deref() == Some("manual") {
            return;
        }
        let id = r.get_untracked().id;
        spawn_local(async move {
            let body = serde_json::json!({ "target_team": team }).to_string();
            if let Ok((s, _)) = net::write(
                "PATCH",
                &format!("/api/scouting/{id}/replays/{}", g.gid),
                Some(body),
                None,
            )
            .await
            {
                if !(200..300).contains(&s) {
                    flash.run(format!("✗ HTTP {s}"));
                }
            }
            on_changed.run(());
        });
    });
    let delete = Callback::new(move |g: ScoutGame| {
        let what = g.map.clone().unwrap_or_else(|| "this game".to_string());
        if !net::confirm(&format!(
            "Remove {what} ({}) from the report?",
            day(g.played_at.as_deref())
        )) {
            return;
        }
        let id = r.get_untracked().id;
        spawn_local(async move {
            if let Ok((s, _)) = net::write(
                "DELETE",
                &format!("/api/scouting/{id}/replays/{}", g.gid),
                None,
                None,
            )
            .await
            {
                if !(200..300).contains(&s) {
                    flash.run(format!("✗ HTTP {s}"));
                }
            }
            on_changed.run(());
        });
    });
    let cls = |s: &str| {
        if s == "waiting" || s.ends_with('…') {
            "wait"
        } else if s.starts_with("added") && !s.contains("not found") {
            "ok"
        } else {
            "ko"
        }
    };
    view! {
        <section class="ds-sec" id="games">
            {move || view! { <SecHead num=num.get() title="Replays" sub="click a team to set it as the scouted side by hand" /> }}
            <div class=move || if over.get() { "ds-drop over" } else { "ds-drop" }
                on:click=move |_| { if let Some(i) = input.get() { i.click(); } }
                on:dragover=move |e: ev::DragEvent| { e.prevent_default(); over.set(true); }
                on:dragleave=move |_| over.set(false)
                on:drop=move |e: ev::DragEvent| {
                    e.prevent_default();
                    over.set(false);
                    on_files(e.data_transfer().and_then(|d| d.files()));
                }>
                <b>"Drop replays here"</b>
                ".StormReplay files of their games — or click to choose"
                <input node_ref=input type="file" multiple accept=".StormReplay,.stormreplay" style="display: none"
                    on:click=move |e| e.stop_propagation()
                    on:change=move |e| {
                        let el: web_sys::HtmlInputElement = event_target(&e);
                        on_files(el.files());
                        el.set_value("");
                    } />
            </div>
            {move || { let q = queue.get(); (!q.is_empty()).then(|| view! {
                <div class="ds-queue">{q.into_iter().map(|(n, s)| { let c = cls(&s); view! { <div><span>{n}</span><span class=c>{s}</span></div> } }).collect_view()}</div>
            }) }}
            {move || {
                let rep = r.get();
                let rows: HashMap<i64, GameRow> = rep.snapshot.as_ref().map(|s| s.facts.games.iter().map(|g| (g.gid, g.clone())).collect()).unwrap_or_default();
                let names = names.get();
                (!rep.games.is_empty()).then(|| view! {
                    <div class="ds-glog">
                        {rep.games.iter().map(|g| view! { <GameLine g=g.clone() row=rows.get(&g.gid).cloned() names=names.clone() on_side=set_side on_delete=delete /> }).collect_view()}
                    </div>
                })
            }}
        </section>
    }
}

fn js_sys_encode(s: &str) -> String {
    uri_component(s)
}

#[component]
fn Roster(r: Signal<Report>, num: Signal<u32>, on_patch: Callback<Value>) -> impl IntoView {
    // state initialised once per facts version (the page re-creates this component when it changes)
    let rep = r.get_untracked();
    let snap = rep.snapshot.clone();
    let initial_roster: Vec<String> = snap.as_ref().map(|s| s.roster.clone()).unwrap_or_default();
    let initial_anchors = rep.anchors.clone();
    let roster = RwSignal::new(initial_roster.clone());
    let anchors = RwSignal::new(initial_anchors.clone());
    let candidates: Vec<String> = snap
        .as_ref()
        .map(|s| {
            s.detection
                .candidates
                .iter()
                .map(|c| c.toon.clone())
                .collect()
        })
        .unwrap_or_default();
    let roster_auto = snap.as_ref().is_some_and(|s| s.roster_auto);
    // (toon, name, games, hero) of every account seen, candidates first, then most games, then name
    let mut seen: Vec<(String, String, i64, String)> = Vec::new();
    for g in &rep.games {
        for team in &g.teams {
            for p in team {
                if let Some(e) = seen.iter_mut().find(|e| e.0 == p.toon) {
                    e.2 += 1;
                    e.1 = p.name.clone();
                } else {
                    seen.push((p.toon.clone(), p.name.clone(), 1, p.hero.clone()));
                }
            }
        }
    }
    seen.sort_by(|a, b| {
        let (ca, cb) = (candidates.contains(&a.0), candidates.contains(&b.0));
        cb.cmp(&ca)
            .then(b.2.cmp(&a.2))
            .then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let sorted = |v: &Vec<String>| {
        let mut v = v.clone();
        v.sort();
        v
    };
    let dirty = Signal::derive(move || {
        sorted(&roster.get()) != sorted(&initial_roster)
            || sorted(&anchors.get()) != sorted(&initial_anchors)
    });
    let toggle = |sig: RwSignal<Vec<String>>, t: String| {
        sig.update(|l| {
            if let Some(i) = l.iter().position(|x| *x == t) {
                l.remove(i);
            } else {
                l.push(t);
            }
        })
    };
    let empty = seen.is_empty();
    view! {
        <section class="ds-sec" id="roster">
            {move || view! { <SecHead num=num.get() title="Roster" sub=if roster_auto { "auto-detected" } else { "set by hand" } /> }}
            <div class="ds-roster-hd">
                <span>"A game counts when 3+ roster players are on one side, otherwise the anchor's side (⚓)."</span>
                <span class="ds-actions">
                    {(!roster_auto).then(|| view! { <span class="ds-btn" on:click=move |_| on_patch.run(serde_json::json!({ "roster": [] }))>"Back to auto"</span> })}
                    <span class=move || if dirty.get() { "ds-btn primary" } else { "ds-btn" } aria-disabled=move || (!dirty.get()).to_string()
                        on:click=move |_| if dirty.get() { on_patch.run(serde_json::json!({ "roster": roster.get_untracked(), "anchors": anchors.get_untracked() })) }>"Save roster"</span>
                </span>
            </div>
            {empty.then(|| view! { <div class="ds-empty">"Add replays first."</div> })}
            <div class="ds-roster-list">
                {seen.into_iter().map(|(toon, name, games, hero)| {
                    let (t1, t2, t3, t4) = (toon.clone(), toon.clone(), toon.clone(), toon.clone());
                    let cand = candidates.contains(&toon);
                    view! {
                        <label class=move || if roster.get().contains(&t1) { "ds-roster-row in" } else { "ds-roster-row" }>
                            <input type="checkbox" title="In the main roster" prop:checked=move || roster.get().contains(&t2) on:change=move |_| toggle(roster, t3.clone()) />
                            <Portrait hero=hero size=30 />
                            <strong>{name}</strong>
                            {cand.then(|| view! { <span class="ds-cand">"candidate"</span> })}
                            <span class="toon">{toon.clone()}</span>
                            <span class="games">{format!("{games} game{}", if games == 1 { "" } else { "s" })}</span>
                            <span class=move || if anchors.get().contains(&t4) { "ds-btn small on" } else { "ds-btn small" }
                                title="Anchor: this player's side is the scouted team"
                                on:click=move |e: ev::MouseEvent| { e.prevent_default(); toggle(anchors, toon.clone()); }>"⚓ anchor"</span>
                        </label>
                    }
                }).collect_view()}
            </div>
        </section>
    }
}

#[component]
fn ImportModal(r: Signal<Report>, on_close: Callback<()>, on_done: Callback<()>) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let result = RwSignal::new(None::<(bool, Vec<String>)>);
    let busy = RwSignal::new(false);
    let submit = move || {
        let body = text.get_untracked();
        if body.trim().is_empty() {
            return;
        }
        let rep = r.get_untracked();
        if rep.analysis.is_some()
            && !net::confirm(&format!(
                "Replace the draft plan imported on {}?",
                day(rep.analysis_imported_at.as_deref())
            ))
        {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let res = net::write_raw(
                "PUT",
                &format!("/api/scouting/{}/analysis", rep.id),
                Some("text/plain"),
                &[],
                Some(body.into()),
                None,
            )
            .await;
            match res {
                Ok((s, b)) => {
                    let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
                    if (200..300).contains(&s) {
                        let t = v.get("tally");
                        let n = |k: &str| {
                            t.and_then(|t| t.get(k))
                                .and_then(Value::as_i64)
                                .unwrap_or(0)
                        };
                        let mut lines = vec![format!(
                            "✓ imported — {} recommendations, {} unsupported, {} unknown fact ids",
                            n("claims"),
                            n("unsupported"),
                            n("unknown_ids")
                        )];
                        lines.extend(
                            v.get("warnings")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(|w| format!("⚠ {w}")),
                        );
                        result.set(Some((true, lines)));
                        on_done.run(());
                    } else {
                        let why = if s == 401 {
                            "unauthorized — set the admin token in Admin".to_string()
                        } else {
                            v.get("error")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("HTTP {s}"))
                        };
                        result.set(Some((
                            false,
                            vec![format!("✗ {why}"), "Nothing was changed.".to_string()],
                        )));
                    }
                }
                Err(_) => result.set(Some((false, vec!["✗ network error".to_string()]))),
            }
            busy.set(false);
        });
    };
    view! {
        <div class="ds-modal-bg" on:click=move |e| {
            let t: web_sys::Element = event_target(&e);
            if t.class_list().contains("ds-modal-bg") { on_close.run(()); }
        }>
            <div class="ds-modal">
                <div style="display: flex; align-items: center; margin-bottom: 12px">
                    <h2>"Import analysis"</h2>
                    <span class="ds-btn" style="margin-left: auto" on:click=move |_| on_close.run(())>"Close"</span>
                </div>
                <div style="display: flex; flex-direction: column; gap: 10px">
                    <span class="muted" style="font-size: 13px">
                        "Paste the LLM's whole reply (the JSON block can be surrounded by text), or load it from a file."
                        {move || r.get().analysis.is_some().then_some(" The current draft plan will be replaced.")}
                    </span>
                    <textarea class="ds-input" rows="14" prop:value=move || text.get() on:input=move |e| text.set(event_target_value(&e))
                        placeholder=move || format!("```json\n{{ \"format_version\": 2, \"report_id\": {}, ... }}\n```", r.get().id)></textarea>
                    <div style="display: flex; gap: 8px; align-items: center">
                        <input type="file" accept=".json,.txt,.md" style="font-size: 11px; color: var(--muted-2)" on:change=move |e| {
                            let el: web_sys::HtmlInputElement = event_target(&e);
                            if let Some(f) = el.files().and_then(|l| l.item(0)) {
                                spawn_local(async move { if let Ok(t) = net::read_file_text(&f).await { text.set(t); } });
                            }
                        } />
                        <span class="ds-btn primary" style="margin-left: auto" aria-disabled=move || busy.get().to_string()
                            on:click=move |_| if !busy.get_untracked() { submit() }>{move || if busy.get() { "Importing…" } else { "Import" }}</span>
                    </div>
                    {move || result.get().map(|(ok, lines)| lines.into_iter().enumerate().map(|(i, l)| {
                        let color = if ok && i == 0 { "color: var(--win)" } else if ok { "color: var(--muted-2)" } else { "color: var(--loss-soft)" };
                        view! { <div class="res" style=color>{l}</div> }
                    }).collect_view())}
                </div>
            </div>
        </div>
    }
}

// ── page ────────────────────────────────────────────────────────────────────

#[component]
pub fn ScoutingReport() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();
    let bump = RwSignal::new(0u32);
    let data = fetch::<Report>(move || {
        bump.track();
        format!("/api/scouting/{}", id())
    });
    let refresh = Callback::new(move |()| bump.update(|b| *b += 1));
    let msg = RwSignal::new(None::<(String, bool)>);
    let import_open = RwSignal::new(false);
    let nav = StoredValue::new_local(use_navigate());
    let flash = Callback::new(move |text: String| {
        let bad = text.starts_with('✗');
        msg.set(Some((text, bad)));
        net::after_ms(6000, move || msg.set(None));
    });
    // the last loaded report, kept while a refresh is in flight
    let report = Memo::new(move |_| data.get().and_then(Result::ok));
    let r = Signal::derive(move || report.get().unwrap_or_default());
    let patch = Callback::new(move |body: Value| {
        let rid = r.get_untracked().id;
        spawn_local(async move {
            match net::write(
                "PATCH",
                &format!("/api/scouting/{rid}"),
                Some(body.to_string()),
                None,
            )
            .await
            {
                Ok((401, _)) => {
                    flash.run("✗ unauthorized — set the admin token in Admin".to_string())
                }
                Ok((s, _)) if !(200..300).contains(&s) => flash.run(format!("✗ HTTP {s}")),
                _ => {}
            }
            refresh.run(());
        });
    });
    let copy_pack = move || {
        let rid = r.get_untracked().id;
        spawn_local(async move {
            let text = net::write_raw(
                "GET",
                &format!("/api/scouting/{rid}/pack.md"),
                None,
                &[],
                None,
                Some(""),
            )
            .await;
            let done = match text {
                Ok((_, t)) => net::copy_text(&t).await.map(|()| t.encode_utf16().count()),
                Err(e) => Err(e),
            };
            flash.run(match done {
                Ok(n) => format!("✓ pack copied ({} k characters) — paste it into ChatGPT, Claude or your local model", (n as f64 / 1000.0 + 0.5).floor() as i64),
                Err(_) => "✗ could not copy — use Download .md instead".to_string(),
            });
        });
    };
    let remove = move || {
        let rep = r.get_untracked();
        if !net::confirm(&format!(
            "Delete the report “{}” and its replays?",
            rep.title
        )) {
            return;
        }
        spawn_local(async move {
            match net::write("DELETE", &format!("/api/scouting/{}", rep.id), None, None).await {
                Ok((s, _)) if (200..300).contains(&s) => {
                    refresh.run(());
                    nav.with_value(|n| n("/scouting", Default::default()));
                }
                Ok((s, _)) => flash.run(format!("✗ HTTP {s}")),
                Err(e) => flash.run(format!("✗ {e}")),
            }
        });
    };
    let facts = move || r.get().snapshot.map(|s| s.facts);
    let names = Signal::derive(move || -> Names { Arc::new(pid_names(facts().as_ref())) });
    let has_games = move || facts().is_some_and(|f| f.overview.games > 0);
    let has_choice = move || {
        r.get()
            .analysis
            .is_some_and(|a| a.map_choice.pick.len() + a.map_choice.avoid.len() > 0)
    };
    let has_general = move || {
        r.get().analysis.is_some_and(|a| {
            a.general.bans.len() + a.general.picks.len() + a.general.considerations.len() > 0
        })
    };
    let rows = Memo::new(move |_| {
        let rep = r.get();
        let plans = rep.analysis.map(|a| a.maps).unwrap_or_default();
        maps_with_plans(rep.snapshot.as_ref().map(|s| &s.facts), &plans)
    });
    // numbers of the sections after the analysis ones
    let base = move || {
        if has_games() {
            1 + u32::from(has_choice()) + 1 + u32::from(has_general()) + 3
        } else {
            0
        }
    };
    let games_num = Signal::derive(move || base() + 1);
    let roster_num = Signal::derive(move || base() + 2);
    let facts_version = Memo::new(move |_| r.get().facts_version);
    let export_html = move || {
        flash.run(
            "✗ Export HTML is not available in the Rust front-end yet — use the React app"
                .to_string(),
        )
    };

    view! {
        {move || match (report.get(), data.get()) {
            (None, None) => view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"loading…"</div></div> }.into_any(),
            (None, Some(_)) => view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"Report not found. "<Link to="/scouting">"Back to reports"</Link></div></div> }.into_any(),
            (Some(_), _) => view! {
                <div class="ds-page">
                    <div class="ds-crumb"><Link to="/scouting">"Scouting"</Link>{move || format!(" / report #{}", r.get().id)}</div>
                    <Cover r=r on_patch=patch>
                        <div class="ds-cover-foot">
                            <span class=move || format!("ds-status {}", r.get().status.class())>{move || r.get().status.label()}</span>
                            <span class="ds-meta">
                                {move || { let rep = r.get(); format!("created {} · updated {} · facts v{}{}", day(Some(&rep.created_at)), day(Some(&rep.updated_at)), rep.facts_version,
                                    rep.analysis_imported_at.as_deref().map(|d| format!(" · plan imported {}{}", day(Some(d)), rep.analysis_model.as_deref().map(|m| format!(" ({m})")).unwrap_or_default())).unwrap_or_default()) }}
                            </span>
                            <span class="ds-actions">
                                <span class="ds-btn" title="Copy the LLM pack (Markdown) to the clipboard" on:click=move |_| copy_pack()>"Copy pack"</span>
                                <a class="ds-btn" href=move || format!("/api/scouting/{}/pack.md", r.get().id) download>".md"</a>
                                <a class="ds-btn" href=move || format!("/api/scouting/{}/pack.xlsx", r.get().id) download>".xlsx"</a>
                                <span class="ds-btn" on:click=move |_| import_open.set(true)>"Import analysis"</span>
                                <span class="ds-btn primary" aria-disabled="false" title="Save a self-contained, shareable HTML report for your teammates" on:click=move |_| export_html()>"Export HTML"</span>
                                <span class="ds-btn danger" title="Delete this report" on:click=move |_| remove()>"Delete"</span>
                            </span>
                        </div>
                    </Cover>

                    <nav class="ds-nav">
                        {move || has_games().then(|| view! { <a href="#identity">"Identity"</a> })}
                        {move || has_choice().then(|| view! { <a href="#map-choice">"Map choice"</a> })}
                        {move || rows.get().into_iter().map(|x| view! { <a class="map" href=format!("#{}", map_anchor(&x.map))>{x.map.clone()}</a> }).collect_view()}
                        {move || has_general().then(|| view! { <a href="#general">"Any map"</a> })}
                        {move || has_games().then(|| view! { <a href="#players">"Players"</a> })}
                        {move || has_games().then(|| view! { <a href="#faced">"Faced"</a> })}
                        {move || has_games().then(|| view! { <a href="#flow">"Game flow"</a> })}
                        <a href="#games">"Replays"</a>
                        <a href="#roster">"Roster"</a>
                    </nav>
                    {move || msg.get().map(|(t, bad)| view! { <div class=if bad { "ds-toast bad" } else { "ds-toast" }>{t}</div> })}
                    <Banners r=r />

                    {move || {
                        let f = facts().filter(|f| f.overview.games > 0)?;
                        let rep = r.get();
                        let names = names.get();
                        let mut n = 0u32;
                        let mut next = || { n += 1; n };
                        let identity = view! { <Identity r=rep.clone() facts=f.clone() names=names.clone() num=next() /> };
                        let choice = if has_choice() { rep.analysis.clone().map(|a| view! { <MapChoice a=a names=names.clone() num=next() /> }) } else { None };
                        let has_analysis = rep.analysis.is_some();
                        let maps_num = next();
                        let boards = rows.get().into_iter().enumerate().map(|(i, x)| view! {
                            <MapBoard map=x.map.clone() m=x.facts.clone() plan=x.plan.clone() facts=f.clone() names=names.clone() i=i has_analysis=has_analysis />
                        }).collect_view();
                        let general = if has_general() { rep.analysis.clone().map(|a| view! { <General a=a names=names.clone() num=next() /> }) } else { None };
                        let players = view! { <Players facts=f.clone() num=next() /> };
                        let faced = view! { <Faced facts=f.clone() num=next() /> };
                        let flow = view! { <Flow facts=f.clone() num=next() /> };
                        Some(view! {
                            {identity}
                            {choice}
                            <section class="ds-sec" id="maps">
                                <SecHead num=maps_num title="Map by map" sub="bans · picks · what they will play" />
                                {boards}
                            </section>
                            {general}
                            {players}
                            {faced}
                            {flow}
                        })
                    }}
                    <Games r=r names=names num=games_num on_changed=refresh flash=flash />
                    {move || { facts_version.track(); view! { <Roster r=r num=roster_num on_patch=patch /> } }}
                    {move || import_open.get().then(|| view! { <ImportModal r=r on_close=Callback::new(move |()| import_open.set(false)) on_done=refresh /> })}
                </div>
            }.into_any(),
        }}
    }
}
