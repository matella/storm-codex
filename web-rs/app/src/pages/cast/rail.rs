//! Left rail of the control room: Series (Bo, format, pool, "New series"), Teams & lineups (`TeamEditor` per side:
//! name, logo, record, 5 players + captain; each player: typed name + `PlayerPicker`) and the batch replay import.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use leptos::{ev, prelude::*, task::spawn_local, web_sys};
use serde_json::{json, Value};
use storm_web_core::{
    castctl::{
        next_hint, propose_imports, reconcile_team, replay_error_text, replay_import_order,
        replay_note, resync_team, setup_steps, toggle_pool, MAPS,
    },
    logic::{series_score, startable_maps},
    model::{CastSeries, Side, Team},
    net,
    players::{PlayerHit, PlayerSearch},
};

use super::api::{self, Runner};

const BOS: [u32; 4] = [1, 3, 5, 7];
const FORMATS: [&str; 3] = ["fearless", "standard", "normal"];

fn first_file(e: &ev::Event) -> Option<web_sys::File> {
    let input = event_target::<web_sys::HtmlInputElement>(e);
    let f = input.files().and_then(|f| f.get(0));
    input.set_value("");
    f
}

// ── player picker ──────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Found {
    Nothing,
    Loading,
    Failed,
    Hits(String, Vec<PlayerHit>),
}

/// Player selector: typed name → `GET /api/players/search` → list (name, games, last seen) → the operator chooses. No
/// result = "unlinked" player (the typed name stays). The first result is NEVER taken automatically.
#[component]
fn PlayerPicker(team: RwSignal<Team>, i: usize, label: String) -> impl IntoView {
    let found = RwSignal::new(Found::Nothing);
    let seq = Arc::new(AtomicU32::new(0));
    let name = move || team.with(|t| t.players.get(i).map(|p| p.name.clone()).unwrap_or_default());
    let toon = move || team.with(|t| t.players.get(i).and_then(|p| p.toon.clone()));
    let can_search =
        move || name().trim().chars().count() >= 2 && !matches!(found.get(), Found::Loading);
    let search = {
        let seq = seq.clone();
        Callback::new(move |()| {
            let q = name().trim().to_string();
            if q.chars().count() < 2 || matches!(found.get_untracked(), Found::Loading) {
                return;
            }
            let id = seq.fetch_add(1, Ordering::SeqCst) + 1;
            found.set(Found::Loading);
            let seq = seq.clone();
            spawn_local(async move {
                let url = format!(
                    "/api/players/search?q={}",
                    storm_web_core::route::uri_component(&q)
                );
                let r = net::get_json::<PlayerSearch>(&url).await;
                if seq.load(Ordering::SeqCst) == id {
                    found.set(match r {
                        Ok(s) => Found::Hits(q, s.hits),
                        Err(_) => Found::Failed,
                    });
                }
            });
        })
    };
    let edit = move |f: &dyn Fn(&mut storm_web_core::model::CastPlayer)| {
        team.update(|t| {
            if let Some(p) = t.players.get_mut(i) {
                f(p)
            }
        })
    };
    let (l_name, l_find, l_unlink, l_results) = (
        format!("{label} name"),
        format!("{label} find player"),
        format!("{label} unlink"),
        format!("{label} results"),
    );
    let seq2 = seq.clone();
    view! {
        <div class="cr-picker">
            <div class="cr-pk-row">
                <input class="ds-input" placeholder="pseudo" aria-label=l_name prop:value=name
                    on:input=move |e| {
                        // retyping a name breaks the link (never a stale toon) and invalidates any search in flight
                        seq2.fetch_add(1, Ordering::SeqCst);
                        found.set(Found::Nothing);
                        let v = event_target_value(&e);
                        edit(&|p| { p.name = v.clone(); p.toon = None; });
                    }
                    on:keydown=move |e| if e.key() == "Enter" { search.run(()) } />
                <button type="button" class="ds-btn small" disabled=move || !can_search() aria-label=l_find
                    on:click=move |_| search.run(())>"Find"</button>
            </div>
            <div class=move || format!("cr-pk-st{}", if toon().is_some() { " on" } else { "" })>
                {move || if toon().is_some() { "linked" } else { "unlinked" }}
                {
                    let l_unlink = l_unlink.clone();
                    move || toon().is_some().then(|| view! {
                        <button type="button" class="ds-btn small" aria-label=l_unlink.clone()
                            on:click=move |_| { edit(&|p| p.toon = None); found.set(Found::Nothing); }>"unlink"</button>
                    })
                }
            </div>
            {move || match found.get() {
                Found::Nothing => ().into_any(),
                Found::Loading => view! { <div class="cr-note">"searching…"</div> }.into_any(),
                Found::Failed => view! { <div class="cr-note">{format!("search failed — the player stays {}", if toon().is_some() { "linked" } else { "unlinked" })}</div> }.into_any(),
                Found::Hits(q, hits) if hits.is_empty() => view! { <div class="cr-note">{format!("No player found for \"{q}\" — stays unlinked (typed name kept).")}</div> }.into_any(),
                Found::Hits(_, hits) => view! {
                    <ul class="cr-pk-list" aria-label=l_results.clone()>
                        {hits.into_iter().map(|h| {
                            let shown = h.name.clone().unwrap_or_else(|| h.matched_name.clone());
                            let pick = (h.toon.clone(), shown.clone());
                            view! {
                                <li>
                                    <button type="button" class="cr-pk-hit"
                                        on:click=move |_| { let (t, n) = pick.clone(); edit(&|p| { p.toon = Some(t.clone()); p.name = n.clone(); }); found.set(Found::Nothing); }>
                                        <b>{shown}</b>
                                        {h.matched_old.then(|| view! { <i>{format!(" was {}", h.matched_name)}</i> })}
                                        <span>{format!("{} games · {}", h.games, net::fmt_time(h.last_seen.as_deref()))}</span>
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </div>
    }
}

// ── team editor ────────────────────────────────────────────────────────────────

/// Editor of one team in the rail: name, logo (file or URL), record, 5 players (name + picker, tagline, captain).
#[component]
fn TeamEditor(
    side: Side,
    team: RwSignal<Team>,
    on_upload: Callback<web_sys::File>,
    on_logo_typed: Callback<()>,
) -> impl IntoView {
    let sid = if side == Side::Blue { "blue" } else { "red" };
    // URL of a logo that could not be shown (another URL retries by itself)
    let bad_logo = RwSignal::new(None::<String>);
    let n = Memo::new(move |_| team.with(|t| t.players.len()));
    let logo = move || team.with(|t| t.logo.clone());
    view! {
        <fieldset class=format!("cr-team tone-{sid}")>
            <legend>{if side == Side::Blue { "Blue" } else { "Red" }}</legend>
            <input class="ds-input" placeholder=if side == Side::Blue { "Team Blue" } else { "Team Red" } aria-label=format!("{sid} team name")
                prop:value=move || team.with(|t| t.name.clone())
                on:input=move |e| { let v = event_target_value(&e); team.update(|t| t.name = v); } />
            <div class="cr-logo">
                {move || match logo() {
                    Some(l) if bad_logo.get().as_deref() != Some(l.as_str()) => {
                        let l2 = l.clone();
                        view! { <img src=l alt="" on:error=move |_| bad_logo.set(Some(l2.clone())) /> }.into_any()
                    }
                    _ => view! { <span class="cr-nologo">"no logo"</span> }.into_any(),
                }}
                <input type="file" accept="image/png,image/jpeg,image/webp" aria-label=format!("{sid} logo file")
                    on:change=move |e| if let Some(f) = first_file(&e) { on_upload.run(f) } />
            </div>
            <input class="ds-input" placeholder="logo URL (optional)" aria-label=format!("{sid} logo URL")
                prop:value=move || logo().unwrap_or_default()
                on:input=move |e| {
                    on_logo_typed.run(());
                    let v = event_target_value(&e);
                    team.update(|t| t.logo = if v.trim().is_empty() { None } else { Some(v) });
                } />
            <input class="ds-input" placeholder="record (e.g. 4-1)" aria-label=format!("{sid} record")
                prop:value=move || team.with(|t| t.record.clone())
                on:input=move |e| { let v = event_target_value(&e); team.update(|t| t.record = v); } />
            {move || (0..n.get()).map(|i| {
                let label = format!("{sid} player {}", i + 1);
                let tagline = move || team.with(|t| t.players.get(i).map(|p| p.tagline.clone()).unwrap_or_default());
                let captain = move || team.with(|t| t.captain == Some(i));
                view! {
                    <div class="cr-player">
                        <PlayerPicker team=team i=i label=label.clone() />
                        <div class="cr-pl2">
                            <input class="ds-input" placeholder="tagline" aria-label=format!("{label} tagline") prop:value=tagline
                                on:input=move |e| { let v = event_target_value(&e); team.update(|t| if let Some(p) = t.players.get_mut(i) { p.tagline = v }); } />
                            <button type="button" class=move || format!("ds-btn small{}", if captain() { " on" } else { "" }) title="Captain"
                                aria-pressed=move || captain().to_string()
                                on:click=move |_| team.update(|t| t.captain = if t.captain == Some(i) { None } else { Some(i) })>"C"</button>
                        </div>
                    </div>
                }
            }).collect_view()}
        </fieldset>
    }
}

// ── batch replay import ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum St {
    Ready,
    Importing,
    Done,
    Failed,
}

#[derive(Clone)]
struct Item {
    key: u32,
    file: web_sys::File,
    summary: Option<Value>,
    error: Option<String>,
    map: Option<String>,
    swap: bool,
    state: St,
    note: Option<String>,
}

fn sum_str(it: &Item, k: &str) -> Option<String> {
    it.summary.as_ref()?.get(k)?.as_str().map(str::to_string)
}

fn when(iso: Option<String>) -> String {
    match iso {
        Some(s) => format!(
            "{} UTC",
            s.replace('T', " ").chars().take(16).collect::<String>()
        ),
        None => "date unknown".to_string(),
    }
}

/// Batch import of a series' replays: every file is decoded without writing anything, the batch proposes a startable
/// pool map per replay, the operator corrects (and may swap sides), then the import creates the games in date order.
/// One failure does not stop the others. The batch is tied to the series it was read for.
#[component]
fn ReplayImport(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let items = RwSignal::new_local(Vec::<Item>::new());
    let batch_series = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    // new (or restarted) series: the batch read for the old one no longer applies
    Effect::new(move |_| {
        let id = s.with(|x| x.series_id.clone());
        if batch_series.with_untracked(|b| b.as_ref().is_some_and(|b| *b != id)) {
            items.set(Vec::new());
            batch_series.set(None);
        }
    });

    let candidates = Memo::new(move |_| {
        s.with(|x| {
            startable_maps(x)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
    });

    let add_files = move |files: Vec<web_sys::File>| {
        let series_id = s.get_untracked().series_id;
        busy.set(true);
        spawn_local(async move {
            let mut decoded = Vec::new();
            for file in files {
                match api::replay_inspect(&file).await {
                    Ok(v) => decoded.push((file, Some(v), None)),
                    Err(e) => decoded.push((file, None, Some(replay_error_text(&e)))),
                }
            }
            busy.set(false);
            batch_series.set(Some(series_id));
            items.update(|prev| {
                let mut key = prev.iter().map(|i| i.key).max().unwrap_or(0);
                let mut added: Vec<Item> = decoded
                    .into_iter()
                    .map(|(file, summary, error)| {
                        key += 1;
                        Item {
                            key,
                            file,
                            summary,
                            error,
                            map: None,
                            swap: false,
                            state: St::Ready,
                            note: None,
                        }
                    })
                    .collect();
                // proposals for the new ones only: choices already made stay, their maps are removed
                let taken: Vec<String> = prev
                    .iter()
                    .filter(|i| i.state != St::Failed)
                    .filter_map(|i| i.map.clone())
                    .collect();
                let free: Vec<String> = candidates
                    .get_untracked()
                    .into_iter()
                    .filter(|m| !taken.contains(m))
                    .collect();
                let props = propose_imports(
                    &added
                        .iter()
                        .map(|i| (sum_str(i, "map"), sum_str(i, "played_at")))
                        .collect::<Vec<_>>(),
                    &free,
                );
                for (it, p) in added.iter_mut().zip(props) {
                    it.map = p;
                }
                prev.extend(added);
            });
        });
    };

    // changing the map or ticking Swap on a failed replay makes it ready again
    let patch = move |key: u32, f: &dyn Fn(&mut Item)| {
        items.update(|v| {
            if let Some(it) = v.iter_mut().find(|i| i.key == key) {
                f(it);
            }
        })
    };
    let reready = |it: &mut Item| {
        if it.state == St::Failed {
            it.state = St::Ready;
        }
        it.error = None;
    };

    let import_all = move |_| {
        run.run(async move {
            let Some(series_id) = batch_series.get_untracked() else { return Ok(()) };
            let snapshot = items.get_untracked();
            let order = replay_import_order(&snapshot.iter().map(|i| sum_str(i, "played_at")).collect::<Vec<_>>());
            busy.set(true);
            for i in order {
                let it = snapshot[i].clone();
                let Some(map) = it.map.clone() else { continue };
                if it.state != St::Ready {
                    continue;
                }
                patch(it.key, &|x| x.state = St::Importing);
                match api::replay_import(&map, &it.file, it.swap, &series_id).await {
                    Ok(res) => {
                        let note = if res.get("orientation").and_then(Value::as_str) == Some("assumed") {
                            Some("no linked player to tell the sides apart: check Blue/Red in the Series tab".to_string())
                        } else {
                            replay_note(res.get("forced_against_evidence").and_then(Value::as_bool) == Some(true)).map(str::to_string)
                        };
                        patch(it.key, &|x| { x.state = St::Done; x.note = note.clone(); });
                    }
                    Err(e) => {
                        let error = replay_error_text(&e);
                        patch(it.key, &|x| { x.state = St::Failed; x.error = Some(error.clone()); });
                    }
                }
            }
            busy.set(false);
            Ok(())
        })
    };

    let importable = move || {
        items.with(|v| {
            v.iter()
                .filter(|i| i.map.is_some() && i.state == St::Ready)
                .count()
        })
    };

    view! {
        <section class="cr-box" aria-label="Batch replay import">
            <div class="cr-k">"Replays · batch import"</div>
            {move || candidates.with(Vec::is_empty).then(|| view! {
                <div class="cr-note">"Mark the maps of this series as picked or decider in the pool first: a replay becomes a game of a picked map."</div>
            })}
            <label class="ds-btn small">
                {move || if busy.get() { "Reading…" } else { "Add .StormReplay files" }}
                <input type="file" accept=".StormReplay" multiple hidden disabled=move || busy.get()
                    on:change=move |e| {
                        let input = event_target::<web_sys::HtmlInputElement>(&e);
                        let files: Vec<web_sys::File> = input.files().map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect()).unwrap_or_default();
                        input.set_value("");
                        if !files.is_empty() { add_files(files) }
                    } />
            </label>
            {move || {
                let list = items.get();
                if list.is_empty() { return ().into_any(); }
                let rows = list.iter().map(|it| {
                    let name = it.file.name();
                    let key = it.key;
                    let options: Vec<String> = {
                        let taken: Vec<String> = list.iter().filter(|o| o.key != key && o.state != St::Failed).filter_map(|o| o.map.clone()).collect();
                        candidates.get().into_iter().filter(|m| Some(m) == it.map.as_ref() || !taken.contains(m)).collect()
                    };
                    let sel = it.map.clone();
                    let summary_line = it.summary.as_ref().map(|sm| {
                        let td = sm.get("takedowns").and_then(Value::as_array);
                        let g = |i: usize| td.and_then(|a| a.get(i)).and_then(Value::as_i64).unwrap_or(0);
                        format!("{} · {} · {} won · {}–{}", when(sum_str(it, "played_at")),
                            sum_str(it, "map").unwrap_or_else(|| "unknown map".to_string()),
                            sm.get("winner").and_then(Value::as_str).unwrap_or(""), g(0), g(1))
                    });
                    let (n1, n2) = (name.clone(), name.clone());
                    view! {
                        <li class="cr-game">
                            <b>{name.clone()}</b>{" "}
                            {summary_line.map(|l| view! { <span class="cr-note">{l}</span> })}
                            {it.error.clone().map(|e| view! { <span class="cr-note cr-warn">{e}</span> })}
                            {(it.summary.is_some() && matches!(it.state, St::Ready | St::Failed)).then(|| view! {
                                <label class="cr-note">{" "}"Game"{" "}
                                    <select class="ds-input" aria-label=format!("Game for {n1}") disabled=move || busy.get()
                                        on:change=move |e| { let v = event_target_value(&e); patch(key, &|x| { x.map = if v.is_empty() { None } else { Some(v.clone()) }; reready(x); }); }>
                                        <option value="" selected=sel.is_none()>"skip"</option>
                                        {options.iter().map(|m| view! { <option value=m.clone() selected=sel.as_deref() == Some(m.as_str())>{m.clone()}</option> }).collect_view()}
                                    </select>
                                </label>
                                <label class="cr-note" title="The series' Blue team played as the replay's second team">
                                    <input type="checkbox" prop:checked=it.swap disabled=move || busy.get() aria-label=format!("Swap sides for {n2}")
                                        on:change=move |e| { let v = event_target_checked(&e); patch(key, &|x| { x.swap = v; reready(x); }); } />" Swap sides"
                                </label>
                            })}
                            {(it.state == St::Importing).then(|| view! { <span class="cr-note">"importing…"</span> })}
                            {(it.state == St::Done).then(|| view! { <span class="cr-note" role="status">{format!("imported{}", it.note.as_ref().map(|n| format!(" · {n}")).unwrap_or_default())}</span> })}
                            {(it.state == St::Failed).then(|| view! { <span class="cr-note cr-warn" role="alert">{format!("failed · {}", it.error.clone().unwrap_or_default())}</span> })}
                        </li>
                    }
                }).collect_view();
                view! { <ol class="cr-games" aria-label="Replays to import">{rows}</ol> }.into_any()
            }}
            {move || (!items.with(Vec::is_empty)).then(|| view! {
                <button type="button" class="ds-btn primary cr-wide" disabled=move || busy.get() || importable() == 0 on:click=import_all>
                    {move || { let n = importable(); format!("Import {n} replay{} as game{}", if n == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" }) }}
                </button>
            })}
        </section>
    }
}

// ── the rail ───────────────────────────────────────────────────────────────────

#[component]
pub fn SeriesRail(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let init = s.get_untracked();
    let blue = RwSignal::new(init.blue.clone());
    let red = RwSignal::new(init.red.clone());
    let bo = RwSignal::new(if BOS.contains(&init.bo) { init.bo } else { 5 });
    let format = RwSignal::new(init.format.clone());
    let pool = RwSignal::new(init.pool.iter().map(|m| m.name.clone()).collect::<Vec<_>>());
    let batch = RwSignal::new(false);

    // last server version seen: an input equal to `base` (nothing edited, or the edit undone) adopts the new
    // server version (another tab, WS); an input that diverged is kept
    let base = StoredValue::new((init.blue.clone(), init.red.clone()));
    // a logo sent as a file comes back from the server: it is adopted even if the input was edited
    let logo_touched = (RwSignal::new(false), RwSignal::new(false));
    Effect::new(move |_| {
        let sv = s.get();
        let old = base.get_value();
        if sv.blue == old.0 && sv.red == old.1 {
            return;
        }
        base.set_value((sv.blue.clone(), sv.red.clone()));
        blue.update(|t| {
            *t = reconcile_team(
                &resync_team(t, &old.0, &sv.blue),
                &sv.blue,
                logo_touched.0.get_untracked(),
            )
        });
        red.update(|t| {
            *t = reconcile_team(
                &resync_team(t, &old.1, &sv.red),
                &sv.red,
                logo_touched.1.get_untracked(),
            )
        });
    });

    let current = move || {
        let sv = s.get_untracked();
        (
            reconcile_team(
                &blue.get_untracked(),
                &sv.blue,
                logo_touched.0.get_untracked(),
            ),
            reconcile_team(
                &red.get_untracked(),
                &sv.red,
                logo_touched.1.get_untracked(),
            ),
        )
    };
    let clean = move || {
        logo_touched.0.set(false);
        logo_touched.1.set(false);
    };
    let save_teams = move |_| {
        let (b, r) = current();
        run.run(async move {
            let v = api::teams(&b, &r).await?;
            clean();
            Ok(v)
        })
    };
    let new_series = move |_| {
        if s.get_untracked().active
            && !net::confirm("Replace the running series? Games and score are reset.")
        {
            return;
        }
        let (b, r) = current();
        let body = json!({ "blue": b, "red": r, "bo": bo.get_untracked(), "format": format.get_untracked(), "pool": pool.get_untracked() });
        // one call: complete teams + series (no intermediate state on air)
        run.run(async move {
            let v = api::series(body).await?;
            clean();
            Ok(v)
        })
    };
    let upload = move |side: Side, file: web_sys::File| {
        if side == Side::Blue {
            logo_touched.0.set(false)
        } else {
            logo_touched.1.set(false)
        }
        run.run(async move { api::logo_upload(side, &file).await })
    };

    let unsaved =
        move || blue.get() != s.with(|x| x.blue.clone()) || red.get() != s.with(|x| x.red.clone());
    let score = move || series_score(&s.get());

    view! {
        <aside class="cr-rail">
            <div class="cr-k">"Series"</div>
            <ol class="cr-steps" aria-label="Setup steps">
                {move || setup_steps(&s.get()).into_iter().enumerate().map(|(i, st)| view! {
                    <li class=if st.done { "done" } else { "" }>{format!("{}. {}{}", i + 1, st.label, if st.done { " ✓" } else { "" })}</li>
                }).collect_view()}
            </ol>
            {move || next_hint(&s.get()).map(|h| view! { <div class="cr-note cr-hint">{h}</div> })}
            <div class="cr-score">
                <span class="tone-blue">{move || { let n = blue.with(|t| t.name.trim().to_string()); if n.is_empty() { "Blue".to_string() } else { n } }}</span><b>{move || score().0}</b>
                <b>{move || score().1}</b><span class="tone-red">{move || { let n = red.with(|t| t.name.trim().to_string()); if n.is_empty() { "Red".to_string() } else { n } }}</span>
            </div>
            <div class="cr-row">
                <label>"Best of"
                    <select class="ds-input" on:change=move |e| bo.set(event_target_value(&e).parse().unwrap_or(5))>
                        {BOS.iter().map(|n| { let n = *n; view! { <option value=n.to_string() selected=move || bo.get() == n>{format!("Bo{n}")}</option> } }).collect_view()}
                    </select>
                </label>
                <label>"Format"
                    <select class="ds-input" on:change=move |e| format.set(event_target_value(&e))>
                        {FORMATS.iter().map(|f| { let f = *f; view! { <option selected=move || format.get() == f>{f}</option> } }).collect_view()}
                    </select>
                </label>
            </div>
            <div class="cr-poolpick" role="group" aria-label="Map pool">
                {MAPS.iter().map(|m| {
                    let m = *m;
                    let on = move || pool.with(|p| p.iter().any(|n| n == m));
                    view! {
                        <label class=move || if on() { "on" } else { "" }>
                            <input type="checkbox" prop:checked=on on:change=move |_| pool.update(|p| *p = toggle_pool(p, m)) />{m}
                        </label>
                    }
                }).collect_view()}
            </div>
            <button type="button" class="ds-btn primary cr-wide" disabled=move || pool.with(Vec::is_empty) on:click=new_series>
                {move || format!("New series ({} maps)", pool.with(Vec::len))}
            </button>
            {move || pool.with(Vec::is_empty).then(|| view! { <div class="cr-note">"Tick at least one map to create a series."</div> })}
            {move || unsaved().then(|| view! { <div class="cr-note">"Unsaved team changes will be sent with the new series."</div> })}
            <label class="cr-note">"Replays"{" "}
                <select class="ds-input" aria-label="Replay loading" on:change=move |e| batch.set(event_target_value(&e) == "batch")>
                    <option value="manual" selected=move || !batch.get()>"Per game (one file each)"</option>
                    <option value="batch" selected=move || batch.get()>"Batch import (all the series' files)"</option>
                </select>
            </label>
            {move || batch.get().then(|| if s.with(|x| x.active) {
                view! { <ReplayImport s=s run=run /> }.into_any()
            } else {
                view! { <div class="cr-note">"Create the series first, then add its replays here."</div> }.into_any()
            })}

            <div class="cr-k">"Teams & lineups"</div>
            <TeamEditor side=Side::Blue team=blue on_upload=Callback::new(move |f| upload(Side::Blue, f)) on_logo_typed=Callback::new(move |()| logo_touched.0.set(true)) />
            <TeamEditor side=Side::Red team=red on_upload=Callback::new(move |f| upload(Side::Red, f)) on_logo_typed=Callback::new(move |()| logo_touched.1.set(true)) />
            <button type="button" class="ds-btn cr-wide" disabled=move || !unsaved() on:click=save_teams>
                {move || if unsaved() { "Save teams" } else { "Teams saved" }}
            </button>
        </aside>
    }
}
