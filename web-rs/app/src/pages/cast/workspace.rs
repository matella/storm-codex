//! Workspaces of the Series and Lineups scenes. Port of `SeriesWorkspace.tsx` and `LineupsWorkspace.tsx`.

use leptos::{prelude::*, task::spawn_local};
use serde_json::{json, Value};
use storm_web_core::{
    castctl::{
        interval_seconds, mode_a_hint, replay_error_text, replay_note, slide_label, INTERVAL_MAX,
        INTERVAL_MIN, SLIDE_NAMES,
    },
    logic::{display_name, shown_game, slide_count, slide_index, slide_tick},
    model::{CastLineup, CastSeries, HeroCount, LineupMode, Side},
    net,
    scenes::{effective_lineup_mode, lineup_query, series_player_heroes, Mode},
};

use super::{
    api::{self, Runner},
    HeroPortrait,
};
use crate::ui::use_interval;

#[component]
fn GameRow(n: u32, s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let busy = RwSignal::new(false);
    let swap = RwSignal::new(false);
    let note = RwSignal::new(None::<&'static str>);
    let game = move || s.with(|x| x.games.iter().find(|g| g.number == n).cloned());
    let load = move |file: leptos::web_sys::File| {
        busy.set(true);
        note.set(None);
        let sw = swap.get_untracked();
        spawn_local(async move {
            let res = run
                .exec(async {
                    api::game_replay(n, &file, sw)
                        .await
                        .map_err(|e| replay_error_text(&e))
                })
                .await;
            note.set(res.and_then(|v| {
                replay_note(v.get("forced_against_evidence").and_then(Value::as_bool) == Some(true))
            }));
            busy.set(false);
            swap.set(false); // "Swap sides" is for ONE upload: unticked after each attempt
        });
    };
    let line = move || {
        let Some(g) = game() else {
            return String::new();
        };
        match (&g.summary, g.winner) {
            (Some(sm), _) => format!(
                "replay loaded · {} won · {}–{} · {}",
                if sm.winner == Side::Blue {
                    "blue"
                } else {
                    "red"
                },
                sm.takedowns.0,
                sm.takedowns.1,
                storm_web_core::scenes::fmt_clock(sm.duration_secs)
            ),
            (None, Some(w)) => format!(
                "{} won · no replay",
                if w == Side::Blue { "blue" } else { "red" }
            ),
            (None, None) => "no replay".to_string(),
        }
    };
    let has_summary = move || game().is_some_and(|g| g.summary.is_some());
    view! {
        <div class="cr-game">
            <b>{format!("Game {n}")}</b>" "<span>{move || game().map(|g| g.map).unwrap_or_default()}</span>" "
            <span class="cr-note">{line}</span>
            <label class="cr-note" title="The series' Blue team played as the replay's second team">
                <input type="checkbox" prop:checked=move || swap.get() aria-label=format!("Game {n} swap sides")
                    on:change=move |e| swap.set(event_target_checked(&e)) />" Swap sides"
            </label>
            <label class="ds-btn small">
                {move || if busy.get() { "Decoding…" } else if has_summary() { "Replace replay" } else { "Load replay" }}
                <input type="file" accept=".StormReplay" hidden disabled=move || busy.get() aria-label=format!("Game {n} replay file")
                    on:change=move |e| {
                        let input = event_target::<leptos::web_sys::HtmlInputElement>(&e);
                        let f = input.files().and_then(|l| l.get(0));
                        input.set_value("");
                        if let Some(f) = f { load(f) }
                    } />
            </label>
            {move || note.get().map(|t| view! { <div class="cr-note cr-warn" role="status">{t}</div> })}
        </div>
    }
}

/// Series scene: game whose data is shown, current slide (‹ k / n ›), auto/manual scrolling, replays.
#[component]
pub fn SeriesWorkspace(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    // tick of the auto rotation, computed from the server anchor: OBS and the preview share the same slide
    let now = RwSignal::new(net::now_ms());
    use_interval(250, move || now.set(net::now_ms()));
    let tick = Memo::new(move |_| slide_tick(&s.get(), now.get()));
    let idx = move || slide_index(&s.get(), tick.get());
    let n = move || slide_count(&s.get());
    let label = move || slide_label(&s.get(), tick.get());
    let game_nums =
        Memo::new(move |_| s.with(|x| x.games.iter().map(|g| g.number).collect::<Vec<_>>()));
    let sl_game = move || s.with(|x| x.slide.game);
    let interval_s = move || (s.with(|x| x.slide.interval_ms) as f64 / 1000.0).round() as i64;
    view! {
        <section class="cr-box">
            <div class="cr-k">"Series · slides"</div>
            <div class="cr-steppers">
                <label class="cr-note">"Game shown"{" "}
                    <select class="ds-input" aria-label="Game shown" disabled=move || s.with(|x| x.games.is_empty())
                        on:change=move |e| { let v: u32 = event_target_value(&e).parse().unwrap_or(0); run.run(api::slide(json!({ "game": v }))) }>
                        <option value="0" selected=move || sl_game().is_none()>
                            {move || format!("Latest (auto){}", match (sl_game(), s.with(|x| shown_game(x).map(|g| g.number))) { (None, Some(n)) => format!(" · game {n}"), _ => String::new() })}
                        </option>
                        {move || s.with(|x| x.games.clone()).into_iter().map(|g| {
                            let num = g.number;
                            view! { <option value=num.to_string() selected=move || sl_game() == Some(num)>{format!("Game {num} · {}", g.map)}</option> }
                        }).collect_view()}
                    </select>
                </label>
                <div class="cr-stepper">
                    <button type="button" class="ds-btn small" aria-label="Previous slide" disabled=move || n() <= 1
                        on:click=move |_| run.run(api::slide(json!({ "index": (idx() + n() - 1) % n() })))>"‹"</button>
                    <b title=move || label().1>{move || label().0}</b>
                    <button type="button" class="ds-btn small" aria-label="Next slide" disabled=move || n() <= 1
                        on:click=move |_| run.run(api::slide(json!({ "index": (idx() + 1) % n() })))>"›"</button>
                </div>
                <span class="cr-note" title=move || label().1>
                    {move || if s.with(|x| x.slide.auto) && n() > 1 { format!("rotating {} slides · same clock as the preview and OBS", n()) } else { SLIDE_NAMES.get(idx()).copied().unwrap_or("").to_string() }}
                </span>
                <button type="button" class=move || format!("ds-btn small{}", if s.with(|x| x.slide.auto) { " on" } else { "" })
                    aria-pressed=move || s.with(|x| x.slide.auto).to_string()
                    on:click=move |_| { let auto = !s.get_untracked().slide.auto; run.run(api::slide(json!({ "auto": auto, "index": idx() }))) }>
                    {move || if s.with(|x| x.slide.auto) { "Auto" } else { "Manual" }}
                </button>
                <label class="cr-note">"Every"{" "}
                    <input class="ds-input" type="number" min=INTERVAL_MIN.to_string() max=INTERVAL_MAX.to_string() step="1"
                        aria-label="Slide interval (seconds)" style="width: 70px" prop:value=move || interval_s().to_string()
                        on:change=move |e| {
                            match interval_seconds(&event_target_value(&e)) {
                                None => {
                                    // not a number: back to the server value
                                    let input = event_target::<leptos::web_sys::HtmlInputElement>(&e);
                                    input.set_value(&interval_s().to_string());
                                }
                                Some(v) if v * 1000 != s.get_untracked().slide.interval_ms as i64 => {
                                    run.run(api::slide(json!({ "interval_ms": v * 1000, "index": idx() })))
                                }
                                Some(_) => {}
                            }
                        } />" s"
                </label>
            </div>
            <div class="cr-note">
                {move || if n() > 1 { "Slide 1 = map strip, 2–5 = post-game breakdown of the shown game." } else { "Only the map strip: load the shown game's Custom replay to add the post-game breakdown." }}
            </div>
            {move || (!game_nums.with(Vec::is_empty)).then(|| view! {
                <div class="cr-games">
                    <For each=move || game_nums.get() key=|n| *n children=move |num| view! { <GameRow n=num s=s run=run /> } />
                </div>
            })}
        </section>
    }
}

const MODES: [(&str, &str, &str); 3] = [
    (
        "auto",
        "Auto",
        "Before the first game winner: recent heroes; after that: heroes played in this series",
    ),
    (
        "a",
        "Before series",
        "Top 3 recent heroes per linked player (before any game)",
    ),
    (
        "b",
        "Between games",
        "Heroes played so far in this series, Fearless-locked dimmed",
    ),
];

fn mode_id(m: LineupMode) -> &'static str {
    match m {
        LineupMode::Auto => "auto",
        LineupMode::A => "a",
        LineupMode::B => "b",
    }
}

#[component]
fn SideRows(
    s: Memo<CastSeries>,
    side: Side,
    lineup: Memo<Option<Result<CastLineup, String>>>,
    linked_any: Memo<bool>,
) -> impl IntoView {
    let sid = if side == Side::Blue { "blue" } else { "red" };
    let team = move || {
        s.with(|x| {
            if side == Side::Blue {
                x.blue.clone()
            } else {
                x.red.clone()
            }
        })
    };
    view! {
        <div class="cr-lu-side">
            <div class=format!("cr-lu-h tone-{sid}")>{move || display_name(&team().name, side)}</div>
            {move || {
                let x = s.get();
                let t = team();
                let mode = effective_lineup_mode(&x);
                let played = series_player_heroes(&x, side);
                let heroes: Vec<Vec<HeroCount>> = match lineup.get() {
                    Some(Ok(l)) => if side == Side::Blue { l.blue } else { l.red },
                    _ => Vec::new(),
                };
                let (loading, error) = (linked_any.get() && lineup.get().is_none(), matches!(lineup.get(), Some(Err(_))));
                t.players.iter().enumerate().map(|(i, p)| {
                    let hs = heroes.get(i).cloned().unwrap_or_default();
                    let pl = played.get(i).cloned().unwrap_or_default();
                    let linked = p.toon.is_some();
                    let hint = mode_a_hint(linked, hs.len(), loading, error);
                    let inner = match mode {
                        Mode::A => match hint {
                            None => hs.into_iter().map(|h| view! { <HeroPortrait hero=h.hero.clone() title=format!("{} · {} games", h.hero, h.games) /> }).collect_view().into_any(),
                            Some(h) => view! { <em>{h}</em> }.into_any(),
                        },
                        Mode::B if !pl.is_empty() => pl.into_iter().map(|h| view! { <HeroPortrait hero=h.hero.clone() title=format!("G{} {}", h.game, h.hero) /> }).collect_view().into_any(),
                        Mode::B => view! { <em>"awaiting game data"</em> }.into_any(),
                    };
                    view! {
                        <div class="cr-lu-row">
                            <span class="cr-lu-n">{if p.name.trim().is_empty() { format!("Player {}", i + 1) } else { p.name.clone() }}</span>
                            <span class=format!("cr-pk-st{}", if linked { " on" } else { "" })>{if linked { "linked" } else { "unlinked" }}</span>
                            <span class="cr-lu-h3">{inner}</span>
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

/// Lineups scene: Auto / A / B mode (server override) + preview of what the effective mode shows for each player
/// (recent heroes in A, heroes played in the series in B).
#[component]
pub fn LineupsWorkspace(s: Memo<CastSeries>, run: Runner) -> impl IntoView {
    let query = Memo::new(move |_| s.with(lineup_query));
    let linked_any = Memo::new(move |_| query.with(Option::is_some));
    let res = LocalResource::new(move || {
        let q = query.get();
        async move {
            match q {
                Some(q) => net::fetch_lineup(&q).await.map(Some),
                None => Ok(None),
            }
        }
    });
    // None = loading; the disabled query (nobody linked) is "not loading" with no data
    let lineup = Memo::new(move |_| res.get().map(|r| r.map(Option::unwrap_or_default)));
    let eff = move || {
        if effective_lineup_mode(&s.get()) == Mode::A {
            "A"
        } else {
            "B"
        }
    };
    view! {
        <section class="cr-box">
            <div class="cr-k">"Lineups · mode"</div>
            <div class="cr-steppers" role="group" aria-label="Lineup mode">
                {MODES.iter().map(|(id, label, hint)| {
                    let (id, label, hint) = (*id, *label, *hint);
                    let on = move || s.with(|x| mode_id(x.lineup_mode)) == id;
                    view! {
                        <button type="button" class=move || format!("ds-btn small{}", if on() { " on" } else { "" })
                            aria-pressed=move || on().to_string() title=hint on:click=move |_| run.run(api::lineup_mode(id))>{label}</button>
                    }
                }).collect_view()}
            </div>
            <div class="cr-note">
                "Showing mode "<b>{eff}</b>{move || if s.with(|x| x.lineup_mode) == LineupMode::Auto { " (auto)" } else { " (forced)" }}" — "
                {move || { let id = if eff() == "A" { "a" } else { "b" }; MODES.iter().find(|m| m.0 == id).map(|m| m.2).unwrap_or("") }}
                ". Link players in the rail (Find) to get their recent heroes."
            </div>
            <div class="cr-lu-prev" aria-label="Lineup preview">
                <SideRows s=s side=Side::Blue lineup=lineup linked_any=linked_any />
                <SideRows s=s side=Side::Red lineup=lineup linked_any=linked_any />
            </div>
        </section>
    }
}
