//! Draft workspace of the control room: Live (operator console of the draft simulator) and Replay (remote control of
//! the draft from a replay). Port of `DraftWorkspace.tsx` and `DraftReplayPanel.tsx`. `series.active` locks the name,
//! player names, score and Bo (the series owns them: edited in the rail).

use leptos::{ev, prelude::*, task::spawn_local, web_sys};
use serde::Deserialize;
use serde_json::{json, Value};
use storm_web_core::{
    castctl::{
        current_phase, load_error_message, phase_label, phases_of, seconds_left, side_id, MAPS,
    },
    model::{CastSeries, DraftAction, DraftState, Side},
    net,
    scenes::side_of_step,
};

use super::api;
use crate::{
    layout::LiveBus,
    ui::{fetch, use_dim, use_interval, Portrait, Tone},
};

const FORMATS: [(&str, &str); 3] = [
    ("standard", "Standard"),
    ("normal", "Normal (no bans)"),
    ("fearless", "Fearless"),
];
const ALL: &str = "All";

/// Draft state, read again on every `draft.updated`; the last good value stays on screen.
fn use_draft(bus: LiveBus) -> (Memo<bool>, Memo<DraftState>) {
    let data = fetch::<DraftState>(move || {
        bus.draft.get();
        "/api/draft".to_string()
    });
    let latest = RwSignal::new(None::<DraftState>);
    Effect::new(move |_| {
        if let Some(Ok(d)) = data.get() {
            if latest.with_untracked(|l| l.as_ref() != Some(&d)) {
                latest.set(Some(d));
            }
        }
    });
    (
        Memo::new(move |_| latest.with(Option::is_some)),
        Memo::new(move |_| latest.get().unwrap_or_default()),
    )
}

/// Fire-and-forget draft write, then read the state again (the WS `draft.updated` does the same for other tabs).
fn post(bus: LiveBus, path: &'static str, body: Option<Value>) {
    spawn_local(async move {
        let _ = api::draft_post(path, body).await;
        bus.draft.update(|n| *n += 1);
    });
}

#[component]
pub fn DraftWorkspace(s: Memo<CastSeries>) -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let (ready, d) = use_draft(bus);
    let replay_mode = RwSignal::new(false);
    let locked = Memo::new(move |_| s.with(|x| x.active));
    let series_format = Memo::new(move |_| s.with(|x| x.format.clone()));
    view! {
        <div class="cr-draft">
            <div class="cr-subtabs" role="group" aria-label="Draft mode">
                <button type="button" aria-pressed=move || (!replay_mode.get()).to_string() class=move || if replay_mode.get() { "" } else { "on" } on:click=move |_| replay_mode.set(false)>"Live"</button>
                <button type="button" aria-pressed=move || replay_mode.get().to_string() class=move || if replay_mode.get() { "on" } else { "" } on:click=move |_| replay_mode.set(true)>"Replay"</button>
            </div>
            {move || if replay_mode.get() {
                view! { <ReplayPanel ready=ready d=d /> }.into_any()
            } else {
                view! { <Console ready=ready d=d locked=locked series_format=series_format on_replay=Callback::new(move |()| replay_mode.set(true)) /> }.into_any()
            }}
        </div>
    }
}

// ── live console ───────────────────────────────────────────────────────────────

fn cfg(d: &DraftState) -> Value {
    json!({
        "format": d.format, "map": d.map, "first_pick": side_id(d.first_pick),
        "blue": { "name": d.blue.name, "players": d.blue.players },
        "red": { "name": d.red.name, "players": d.red.players },
        "bo": d.bo,
    })
}

#[component]
fn Stepper(v: Signal<u32>, side: &'static str, on: Callback<u32>) -> impl IntoView {
    view! {
        <div class="score">
            <span class=format!("v {side}")>{move || v.get()}</span>
            <div class="stp">
                <button on:click=move |_| on.run(v.get_untracked() + 1) aria-label="increase">"▲"</button>
                <button on:click=move |_| on.run(v.get_untracked().saturating_sub(1)) aria-label="decrease">"▼"</button>
            </div>
        </div>
    }
}

#[component]
fn Field(label: &'static str, children: Children) -> impl IntoView {
    view! { <div class="fld"><label>{label}</label>{children()}</div> }
}

#[component]
fn TeamColumn(d: Memo<DraftState>, side: Side, locked: Memo<bool>) -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let sid = side_id(side);
    let team = move || {
        d.with(|x| {
            if side == Side::Blue {
                x.blue.clone()
            } else {
                x.red.clone()
            }
        })
    };
    let name = RwSignal::new(team().name);
    let players = RwSignal::new(team().players);
    // the server's version wins whenever it changes
    Effect::new(move |_| {
        let t = team();
        if name.get_untracked() != t.name {
            name.set(t.name)
        }
        if players.get_untracked() != t.players {
            players.set(t.players)
        }
    });
    let push = move || {
        let info = json!({ "name": name.get_untracked(), "players": players.get_untracked() });
        let other = d.with_untracked(|x| {
            let o = if side == Side::Blue { &x.red } else { &x.blue };
            json!({ "name": o.name, "players": o.players })
        });
        let body = if side == Side::Blue {
            json!({ "blue": info, "red": other })
        } else {
            json!({ "blue": other, "red": info })
        };
        post(bus, "/teams", Some(body));
    };
    // step indices of this side
    let idx = Memo::new(move |_| {
        d.with(|x| {
            x.steps
                .iter()
                .enumerate()
                .filter(|(_, s)| side_of_step(x, s) == side)
                .map(|(i, s)| (i, s.action))
                .collect::<Vec<_>>()
        })
    });
    let bans = move || {
        idx.get()
            .into_iter()
            .filter(|(_, a)| *a == DraftAction::Ban)
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let picks = move || {
        idx.get()
            .into_iter()
            .filter(|(_, a)| *a == DraftAction::Pick)
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let hero_at = move |i: usize| d.with(|x| x.assignments.get(i).cloned().flatten());
    let cur = move |i: usize| d.with(|x| x.cursor == i);
    let default_name = if side == Side::Blue { "Blue" } else { "Red" };
    view! {
        <div class=format!("team {sid}")>
            <div class="ds-label">{if side == Side::Blue { "Blue side" } else { "Red side" }}</div>
            {move || if locked.get() {
                let n = team().name;
                view! { <div class="tn ro" title="Edit in the series rail">{if n.is_empty() { default_name.to_string() } else { n }}</div> }.into_any()
            } else {
                view! { <input class="tn" placeholder="Team name" title="Team name (editable)" prop:value=move || name.get()
                    on:input=move |e| name.set(event_target_value(&e)) on:blur=move |_| push() /> }.into_any()
            }}
            {move || { let b = bans(); (!b.is_empty()).then(|| view! {
                <div class="bans">
                    {b.into_iter().map(|i| view! {
                        <span class=move || format!("bn {} {}", if hero_at(i).is_some() { "filled" } else { "" }, if cur(i) { "cur" } else { "" })
                            title=move || hero_at(i)>
                            {move || match hero_at(i) { Some(h) => view! { <Portrait hero=h size=36 tone=Tone::Ban /> }.into_any(), None => "ban".into_any() }}
                        </span>
                    }).collect_view()}
                </div>
            }) }}
            {move || picks().into_iter().enumerate().map(|(k, i)| view! {
                <div class=move || format!("slot {}", if cur(i) { "cur" } else { "" })>
                    {move || match hero_at(i) {
                        Some(h) => view! { <Portrait hero=h size=46 tone=if side == Side::Blue { Tone::Blue } else { Tone::Red } /> }.into_any(),
                        None => view! { <span class="av none" /> }.into_any(),
                    }}
                    <div class="info">
                        <div class=move || format!("hero {}", if hero_at(i).is_some() { "" } else { "none" })>
                            {move || hero_at(i).unwrap_or_else(|| if cur(i) { "picking…".to_string() } else { "—".to_string() })}
                        </div>
                        {move || if locked.get() {
                            let n = team().players.get(k).cloned().unwrap_or_default();
                            view! { <div class="pl ro" title="Edit in the series rail">{if n.is_empty() { format!("player {}", k + 1) } else { n }}</div> }.into_any()
                        } else {
                            view! { <input class="pl" placeholder=format!("player {}", k + 1)
                                prop:value=move || players.with(|p| p.get(k).cloned().unwrap_or_default())
                                on:input=move |e| { let v = event_target_value(&e); players.update(|p| { while p.len() <= k { p.push(String::new()) } p[k] = v }); }
                                on:blur=move |_| push() /> }.into_any()
                        }}
                    </div>
                </div>
            }).collect_view()}
        </div>
    }
}

/// Operator console of the draft simulator (design "dossier"). Drives the server state (REST); the overlay
/// `/draft/overlay` (OBS) reflects everything live over the WS.
#[component]
fn Console(
    ready: Memo<bool>,
    d: Memo<DraftState>,
    locked: Memo<bool>,
    series_format: Memo<String>,
    on_replay: Callback<()>,
) -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let dim = use_dim();
    let role = RwSignal::new(ALL.to_string());
    let search = RwSignal::new(String::new());
    let pre_ban = RwSignal::new(false); // Pick (assign to the current step) vs Pre-ban (manual availability toggle)

    let heroes = Memo::new(move |_| {
        dim.with(|x| {
            x.names()
                .into_iter()
                .map(|n| {
                    let r = x
                        .get(&n)
                        .and_then(|h| h.role.clone())
                        .filter(|r| !r.is_empty());
                    (n, r)
                })
                .collect::<Vec<_>>()
        })
    });
    // roles in the order the server lists the heroes (the tab order of React)
    let ordered = fetch::<Value>(|| "/api/dim/heroes".to_string());
    let roles = Memo::new(move |_| {
        let mut all = vec![ALL.to_string()];
        let from_server: Vec<String> = match ordered.get() {
            Some(Ok(Value::Object(m))) => m
                .values()
                .filter_map(|h| h.get("role").and_then(Value::as_str))
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .collect(),
            _ => heroes.get().into_iter().filter_map(|(_, r)| r).collect(),
        };
        for r in from_server {
            if !all.contains(&r) {
                all.push(r)
            }
        }
        all
    });
    let used = move |name: &str| {
        d.with(|x| {
            x.assignments.iter().flatten().any(|h| h == name)
                || x.manual_unavailable.iter().any(|h| h == name)
                || x.series_bans.iter().any(|h| h == name)
        })
    };
    let filtered = Memo::new(move |_| {
        let (r, q) = (role.get(), search.get().to_lowercase());
        let mut v: Vec<String> = heroes
            .get()
            .into_iter()
            .filter(|(_, hr)| r == ALL || hr.as_deref() == Some(r.as_str()))
            .filter(|(n, _)| n.to_lowercase().contains(&q))
            .map(|(n, _)| n)
            .collect();
        v.sort_by_key(|n| n.to_lowercase());
        v
    });
    let toggle = move |name: String| {
        let on = !d.with_untracked(|x| x.manual_unavailable.contains(&name));
        post(
            bus,
            "/unavailable",
            Some(json!({ "hero": name, "value": on })),
        );
    };
    let on_hero = {
        move |name: String| {
            if pre_ban.get_untracked() {
                toggle(name);
                return;
            }
            if !used(&name) && d.with_untracked(|x| x.cursor < x.steps.len()) {
                post(bus, "/action", Some(json!({ "hero": name })));
            }
        }
    };
    let config = move |f: &dyn Fn(&mut Value)| {
        let mut c = d.with_untracked(cfg);
        f(&mut c);
        post(bus, "/config", Some(c));
    };
    let cur_side = move || d.with(|x| x.steps.get(x.cursor).map(|s| side_of_step(x, s)));
    let phase_class = move || cur_side().map_or("done", side_id);
    let team_name = move |side: Side, fallback: &'static str| {
        d.with(|x| {
            let n = if side == Side::Blue {
                &x.blue.name
            } else {
                &x.red.name
            };
            if n.is_empty() {
                fallback.to_string()
            } else {
                n.clone()
            }
        })
    };
    let first_pick = move || d.with(|x| x.first_pick);

    move || {
        if !ready.get() {
            return view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"loading…"</div></div> }.into_any();
        }
        view! {
            <div class="draftc ds-page">
                <header class="ds-cover dcover">
                    <div class="ds-kicker">"Draft · live"</div>
                    <h1 class="ds-title" style="cursor: default; font-size: clamp(40px, 6vw, 76px)">
                        <span class="tb">{move || team_name(Side::Blue, "Blue")}</span>" "<i>"vs"</i>" "<span class="tr">{move || team_name(Side::Red, "Red")}</span>
                    </h1>
                    <div class="ds-subtitle" style="cursor: default">
                        {move || d.with(|x| format!("{} · {} · Bo{} · series {}–{}", x.map, FORMATS.iter().find(|f| f.0 == x.format).map_or("", |f| f.1), x.bo, x.score.0, x.score.1))}
                    </div>
                    <div class="ds-cover-foot">
                        <button type="button" class="olink linkbtn" on:click=move |_| on_replay.run(())>"Draft from a replay →"</button>
                    </div>
                </header>

                <div class="bar">
                    <Field label="Format">
                        <select disabled=move || locked.get() title=move || locked.get().then_some("Fixed by the active series")
                            on:change=move |e| { let v = event_target_value(&e); config(&|c| c["format"] = json!(v)) }>
                            {FORMATS.iter().map(|(v, label)| { let v = *v; view! { <option value=v selected=move || (if locked.get() { series_format.get() } else { d.with(|x| x.format.clone()) }) == v>{*label}</option> } }).collect_view()}
                        </select>
                    </Field>
                    <Field label="Map">
                        <select on:change=move |e| { let v = event_target_value(&e); config(&|c| c["map"] = json!(v)) }>
                            {MAPS.iter().map(|m| { let m = *m; view! { <option selected=move || d.with(|x| x.map == m)>{m}</option> } }).collect_view()}
                        </select>
                    </Field>
                    <Field label="First pick">
                        <div class="seg">
                            {[Side::Blue, Side::Red].into_iter().map(|sd| view! {
                                <button class=move || format!("{} {}", if first_pick() == sd { "on" } else { "" }, side_id(sd))
                                    on:click=move |_| config(&|c| c["first_pick"] = json!(side_id(sd)))>{if sd == Side::Blue { "Blue" } else { "Red" }}</button>
                            }).collect_view()}
                        </div>
                    </Field>
                    <Field label="Series">
                        {move || if locked.get() {
                            view! { <span class="ro" title="Edit in the series rail">{format!("Bo{}", d.with(|x| x.bo))}</span> }.into_any()
                        } else {
                            view! {
                                <select on:change=move |e| { let v: u32 = event_target_value(&e).parse().unwrap_or(1); config(&|c| c["bo"] = json!(v)) }>
                                    {[1u32, 3, 5, 7].into_iter().map(|b| view! { <option value=b.to_string() selected=move || d.with(|x| x.bo == b)>{format!("Bo{b}")}</option> }).collect_view()}
                                </select>
                            }.into_any()
                        }}
                    </Field>
                    <Field label="Score">
                        {move || if locked.get() {
                            view! {
                                <div class="score" title="Edit in the series rail">
                                    <span class="v blue">{move || d.with(|x| x.score.0)}</span><span class="dash">"–"</span><span class="v red">{move || d.with(|x| x.score.1)}</span>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="score">
                                    <Stepper v=Signal::derive(move || d.with(|x| x.score.0)) side="blue" on=Callback::new(move |n| { let r = d.with_untracked(|x| x.score.1); post(bus, "/score", Some(json!({ "blue": n, "red": r }))) }) />
                                    <span class="dash">"–"</span>
                                    <Stepper v=Signal::derive(move || d.with(|x| x.score.1)) side="red" on=Callback::new(move |n| { let b = d.with_untracked(|x| x.score.0); post(bus, "/score", Some(json!({ "blue": b, "red": n }))) }) />
                                </div>
                            }.into_any()
                        }}
                    </Field>
                    <div class="spacer"></div>
                    <button class="ds-btn" on:click=move |_| post(bus, "/series/next", None)>"Next game ›"</button>
                    <button class="ds-btn primary" disabled=move || locked.get()
                        title=move || locked.get().then_some("The active series is managed from the series rail (New series there resets the draft too)")
                        on:click=move |_| { if net::confirm("Start a new series? (clears the fearless history)") { post(bus, "/series/new", None) } }>"New series"</button>
                </div>

                <div class=move || format!("phase {}", phase_class())>
                    {move || {
                        let x = d.get();
                        match x.steps.get(x.cursor).map(|st| (side_of_step(&x, st), st.action)) {
                            Some((sd, action)) => view! {
                                <span class=format!("dot {}", side_id(sd))></span>
                                <span class="who">{if sd == Side::Blue { x.blue.name.clone() } else { x.red.name.clone() }}</span>
                                <span class=format!("ds-tag {}", if action == DraftAction::Ban { "loss" } else { "win" })>{if action == DraftAction::Ban { "Ban" } else { "Pick" }}</span>
                                <span class="step">{format!("step {} / {}", x.cursor + 1, x.steps.len())}</span>
                                <span class="prog"><span style=format!("width: {}%", x.cursor as f64 / (x.steps.len().max(1)) as f64 * 100.0)></span></span>
                            }.into_any(),
                            None => view! {
                                <span class="dot done"></span><span class="who">"Draft complete"</span><span class="step">{format!("{} steps", x.steps.len())}</span>
                            }.into_any(),
                        }
                    }}
                    <span class="acts">
                        <button class="ds-btn" on:click=move |_| post(bus, "/undo", None)>"↶ Undo"</button>
                        <button class="ds-btn" on:click=move |_| post(bus, "/reset", None)>"⟲ Reset"</button>
                    </span>
                </div>

                <div class="series">
                    <div class="lab">
                        {move || format!("Manual pre-bans (competition / missed game) · {}", d.with(|x| x.manual_unavailable.len()))}
                        <span>" — switch the picker to "<b>"Pre-ban"</b>" and click heroes, or click a portrait here to remove it"</span>
                    </div>
                    <div class="pool">
                        {move || d.with(|x| x.manual_unavailable.is_empty()).then(|| view! { <span class="none">"none"</span> })}
                        {move || d.with(|x| x.manual_unavailable.clone()).into_iter().map(|h| {
                            let (h2, title) = (h.clone(), format!("remove {h}"));
                            view! { <button class="si rm" title=title on:click=move |_| post(bus, "/unavailable", Some(json!({ "hero": h2, "value": false })))><Portrait hero=h size=30 tone=Tone::Ban /></button> }
                        }).collect_view()}
                    </div>
                </div>

                {move || (d.with(|x| x.format == "fearless" && !x.series_bans.is_empty())).then(|| view! {
                    <div class="series">
                        <div class="lab">{move || format!("Series bans — fearless (automatic, previous games) · {}", d.with(|x| x.series_bans.len()))}</div>
                        <div class="pool">{move || d.with(|x| x.series_bans.clone()).into_iter().map(|h| { let t = h.clone(); view! { <span class="si" title=t><Portrait hero=h size=30 tone=Tone::Ban /></span> } }).collect_view()}</div>
                    </div>
                })}

                <div class="body">
                    <TeamColumn d=d side=Side::Blue locked=locked />
                    <div class="picker">
                        <div class="ptop">
                            <input type="text" placeholder="Search a hero…" prop:value=move || search.get() on:input=move |e| search.set(event_target_value(&e)) />
                            <div class="seg" title="Pick = assign to the current step · Pre-ban = manually (un)ban">
                                <button class=move || if pre_ban.get() { "" } else { "on" } on:click=move |_| pre_ban.set(false)>"Pick"</button>
                                <button class=move || if pre_ban.get() { "on red" } else { "" } on:click=move |_| pre_ban.set(true)>"Pre-ban"</button>
                            </div>
                        </div>
                        <div class="tabs">
                            {move || roles.get().into_iter().map(|r| { let (r1, r2, r3) = (r.clone(), r.clone(), r.clone()); view! { <button class=move || if role.get() == r1 { "on" } else { "" } on:click=move |_| role.set(r2.clone())>{r3}</button> } }).collect_view()}
                        </div>
                        <div class="grid">
                            <For each=move || filtered.get() key=|n| n.clone() children=move |name| {
                                let n1 = name.clone();
                                let pre_banned = {
                                    let name = name.clone();
                                    move || pre_ban.get() && d.with(|x| x.manual_unavailable.contains(&name))
                                };
                                let is_used = { let name = name.clone(); move || used(&name) };
                                let (c1, c2, p1) = (is_used.clone(), is_used.clone(), pre_banned.clone());
                                                                let (n2, n3) = (name.clone(), name.clone());
                                view! {
                                    <button class=move || format!("hx {} {}", if c1() { "out" } else { "" }, if p1() { "banned" } else { "" })
                                        disabled=move || !pre_ban.get() && c2() title=n1.clone()
                                        on:click=move |_| on_hero(n2.clone())
                                        on:contextmenu=move |e: ev::MouseEvent| { e.prevent_default(); toggle(n3.clone()) }>
                                        {let name = name.clone(); move || view! { <Portrait hero=name.clone() size=46 tone=if pre_banned() { Tone::Ban } else { Tone::Plain } /> }}
                                        <span class="nm">{name.clone()}</span>
                                    </button>
                                }
                            } />
                        </div>
                        <p class="hint"><b>"Pick"</b>" mode: click = assign to the current step. "<b>"Pre-ban"</b>" mode: click = manually (un)ban. Right-click = quick toggle in both modes."</p>
                    </div>
                    <TeamColumn d=d side=Side::Red locked=locked />
                </div>
            </div>
        }.into_any()
    }
}

// ── replay mode ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
struct LoadedGame {
    source: String,
    map: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
struct ReplayState {
    queue: Vec<LoadedGame>,
    current: Option<usize>,
    fearless: bool,
    playing: bool,
    interval_ms: f64,
    next_at_ms: Option<f64>,
}

fn progress(d: &DraftState) -> String {
    let phases = phases_of(&d.steps);
    match current_phase(&phases, d.cursor) {
        None => format!("{} / {}", phases.len(), phases.len()),
        Some(c) => format!("{c} / {}", phases.len()),
    }
}

/// Remote control of the draft from a replay: loads Custom replays, prepares their hidden draft, then reveals it phase
/// by phase (manual or auto). The public sees the OBS overlay `/draft/overlay`.
#[component]
fn ReplayPanel(ready: Memo<bool>, d: Memo<DraftState>) -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let data = fetch::<ReplayState>(move || {
        bus.draft.get();
        "/api/draft/replay".to_string()
    });
    let latest = RwSignal::new(None::<ReplayState>);
    Effect::new(move |_| {
        if let Some(Ok(r)) = data.get() {
            if latest.with_untracked(|l| l.as_ref() != Some(&r)) {
                latest.set(Some(r));
            }
        }
    });
    let rs = Memo::new(move |_| latest.get().unwrap_or_default());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let secs = RwSignal::new(30i64);
    let now = RwSignal::new(net::now_ms() as f64);
    use_interval(500, move || now.set(net::now_ms() as f64));
    Effect::new(move |prev: Option<f64>| {
        let ms = rs.with(|r| r.interval_ms);
        if prev != Some(ms) && ms > 0.0 {
            secs.set((ms / 1000.0).round() as i64)
        }
        ms
    });

    let run =
        move |f: std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>>>>| {
            err.set(None);
            spawn_local(async move {
                if let Err(e) = f.await {
                    err.set(Some(e))
                }
                bus.draft.update(|n| *n += 1);
            });
        };
    let call = move |path: &'static str, body: Option<Value>| {
        run(Box::pin(async move {
            api::draft_post(&format!("/replay{path}"), body).await
        }))
    };

    let upload = move |files: Vec<web_sys::File>| {
        busy.set(true);
        err.set(None);
        spawn_local(async move {
            for f in files {
                if let Err(e) = api::draft_replay_load(&f).await {
                    err.set(Some(format!(
                        "{} — {}",
                        f.name(),
                        load_error_message(Some(&e))
                    )));
                }
            }
            busy.set(false);
            bus.draft.update(|n| *n += 1);
        });
    };
    let files_of = |l: Option<web_sys::FileList>| -> Vec<web_sys::File> {
        l.map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect())
            .unwrap_or_default()
    };

    let cur = move || rs.with(|r| r.current);
    let is_ready = move || ready.get() && cur().is_some();
    let complete = move || ready.get() && d.with(|x| x.cursor >= x.steps.len());
    let playing = move || rs.with(|r| r.playing);

    // shortcuts: Space = Next, Backspace = Back (outside input fields)
    let _ = window_event_listener(ev::keydown, move |e: ev::KeyboardEvent| {
        use leptos::wasm_bindgen::JsCast;
        if let Some(t) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
        {
            if matches!(t.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA") {
                return;
            }
        }
        if !is_ready() {
            return;
        }
        match e.code().as_str() {
            "Space" => {
                e.prevent_default();
                if !complete() {
                    call("/next", None)
                }
            }
            "Backspace" => {
                e.prevent_default();
                call("/back", None)
            }
            _ => {}
        }
    });

    let file_input = NodeRef::<leptos::html::Input>::new();
    view! {
        <div class="drp ds-page">
            <header class="ds-cover">
                <div class="ds-kicker">"Draft simulator · replay mode"</div>
                <h1 class="ds-title" style="cursor: default; font-size: clamp(34px, 5vw, 60px)">"Draft from a replay"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    "Load custom-game replays, then reveal the draft step by step on the OBS overlay"{" "}
                    <a href="/draft/overlay" target="_blank" class="olink">"/draft/overlay ↗"</a>
                </div>
            </header>

            {move || err.get().map(|e| view! { <div class="err">{e}</div> })}

            <div class="grid2">
                <section class="card">
                    <div class="lab">"Replays · custom games only"</div>
                    <div class="drop" on:dragover=move |e: ev::DragEvent| e.prevent_default()
                        on:drop=move |e: ev::DragEvent| { e.prevent_default(); let f = files_of(e.data_transfer().and_then(|t| t.files())); if !f.is_empty() { upload(f) } }
                        on:click=move |_| if let Some(i) = file_input.get() { i.click() }>
                        <b>{move || if busy.get() { "reading…" } else { "Drop one or more .StormReplay files" }}</b>
                        <small>"added to the queue in order · nothing is saved to the match database"</small>
                        <input node_ref=file_input type="file" accept=".StormReplay" multiple hidden
                            on:change=move |e| { let i = event_target::<web_sys::HtmlInputElement>(&e); let f = files_of(i.files()); i.set_value(""); if !f.is_empty() { upload(f) } } />
                    </div>
                </section>

                <section class="card">
                    <div class="lab">{move || format!("Queue · series ({})", rs.with(|r| r.queue.len()))}</div>
                    {move || rs.with(|r| r.queue.is_empty()).then(|| view! { <div class="hint">"no replay loaded"</div> })}
                    {move || {
                        let r = rs.get();
                        let n = r.queue.len();
                        r.queue.into_iter().enumerate().map(|(i, g)| {
                            let is_cur = r.current == Some(i);
                            let prog = if is_cur { format!(" · phase {}", progress(&d.get())) } else { String::new() };
                            view! {
                                <div class=format!("q {}", if is_cur { "cur" } else { "" })>
                                    <button class="n" title="prepare this game" on:click=move |_| call("/select", Some(json!({ "index": i })))>{i + 1}</button>
                                    <div class="t"><b>{if g.map.is_empty() { "—".to_string() } else { g.map.clone() }}</b><span>{format!("{}{prog}", g.source)}</span></div>
                                    <button class="x" disabled=i == 0 title="move up" on:click=move |_| call("/queue/move", Some(json!({ "from": i, "to": i.saturating_sub(1) })))>"↑"</button>
                                    <button class="x" disabled=i + 1 == n title="move down" on:click=move |_| call("/queue/move", Some(json!({ "from": i, "to": i + 1 })))>"↓"</button>
                                    <button class="x" title="remove" on:click=move |_| run(Box::pin(api::draft_replay_remove(i)))>"✕"</button>
                                </div>
                            }
                        }).collect_view()
                    }}
                    <label class="tog">
                        <input type="checkbox" prop:checked=move || rs.with(|r| r.fearless) on:change=move |e| call("/fearless", Some(json!({ "value": event_target_checked(&e) }))) />
                        "Fearless — picks of earlier games are unavailable"
                        {move || { let n = d.with(|x| x.series_bans.len()); (ready.get() && n > 0).then(|| view! { <span class="hint">{format!(" ({n} heroes)")}</span> }) }}
                    </label>
                </section>
            </div>

            {move || is_ready().then(|| {
                let x = d.get();
                let phases = phases_of(&x.steps);
                let cur_ph = current_phase(&phases, x.cursor);
                let cols = format!("grid-template-columns: repeat({}, minmax(0, 1fr))", phases.len());
                let title = format!("Draft — {} phases · unrevealed heroes stay hidden", phases.len());
                view! {
                    <section class="card">
                        <div class="lab">{title}</div>
                        <div class="rail" style=cols>
                            {phases.iter().enumerate().map(|(i, p)| {
                                let side = x.steps.get(p.start).map(|st| side_of_step(&x, st)).unwrap_or_default();
                                let done = x.cursor >= p.start + p.len;
                                let heroes: Vec<Option<String>> = x.assignments.iter().skip(p.start).take(p.len).cloned().collect();
                                view! {
                                    <div class=format!("ph {} {} {}", side_id(side), if done { "done" } else { "" }, if cur_ph == Some(i) { "cur" } else { "" })>
                                        <b>{format!("{} · {}", i + 1, phase_label(p))}</b>
                                        {if done {
                                            heroes.into_iter().map(|h| view! { <span class="hh">{h.unwrap_or_else(|| "no ban".to_string())}</span> }).collect_view().into_any()
                                        } else {
                                            view! { <span class="hh hid">"••••"</span> }.into_any()
                                        }}
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    </section>
                }
            })}

            <section class="card ctl">
                <button class="ds-btn" disabled=move || !is_ready() || d.with(|x| x.cursor == 0) on:click=move |_| call("/back", None)>"↶ Back "<kbd>"⌫"</kbd></button>
                <button class="ds-btn primary big" disabled=move || !is_ready() || complete() on:click=move |_| call("/next", None)>"Next "<kbd>"Space"</kbd></button>
                <button class="ds-btn" disabled=move || !is_ready() on:click=move |_| call("/reset", None)>"⟲ Reset"</button>
                <span class="sep"></span>
                {move || if playing() {
                    view! { <button class="ds-btn big" on:click=move |_| call("/play", Some(json!({ "playing": false })))>"❚❚ Pause"</button> }.into_any()
                } else {
                    view! { <button class="ds-btn big go" disabled=move || !is_ready() || complete()
                        on:click=move |_| call("/play", Some(json!({ "playing": true, "interval_ms": secs.get_untracked().max(1) * 1000 })))>"▶ Auto"</button> }.into_any()
                }}
                <span class="hint">"every"</span>
                <input class="dur" type="number" min="1" max="3600" prop:value=move || secs.get().to_string() disabled=playing
                    on:input=move |e| secs.set(event_target_value(&e).parse().unwrap_or(30)) />
                <span class="hint">"s per phase"</span>
                {move || playing().then(|| view! { <span class="cd">{format!("next phase in {}s", seconds_left(rs.with(|r| r.next_at_ms), now.get()))}</span> })}
                {move || (is_ready() && complete()).then(|| view! { <span class="cd done">"draft complete"</span> })}
            </section>
        </div>
    }
}
