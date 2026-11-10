//! Cast overlay (`/cast/overlay`), Dioxus 0.7 web (CSR). All scenes: `ingame`, `series` (map strip +
//! post-game slides), `lineups`, `draft`; transparent body, 300 ms cross-fade keeping the outgoing scene
//! mounted, slideshow driven by the server anchor, `/ws` live updates with reconnection.

use dioxus::prelude::*;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use spike_common::{
    logic::*,
    model::*,
    net,
    profile::{Dim, DimHero},
    scenes::*,
};
use std::collections::HashMap;

const FADE_MS: u32 = 300;
const CSS: Asset = asset!("/assets/cast-overlay.css");
const POSTGAME_CSS: Asset = asset!("/assets/postgame.css");
const DRAFT_CSS: Asset = asset!("/assets/draft-overlay.css");

fn main() {
    net::set_body_transparent();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let series = use_signal(|| None::<CastSeries>);
    let draft = use_signal(|| None::<DraftState>);
    let mut layers = use_signal(Layers::default);
    let mut now = use_signal(net::now_ms);
    let mut dim = use_context_provider(|| Signal::new(Dim::default()));
    use_context_provider(|| now);
    use_context_provider(|| draft);

    use_hook(move || {
        let fetch_cast = move || {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(s) = net::fetch_cast().await {
                    let mut series = series;
                    series.set(Some(s));
                }
            })
        };
        let fetch_draft = move || {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(d) = net::fetch_draft().await {
                    let mut draft = draft;
                    draft.set(Some(d));
                }
            })
        };
        fetch_cast();
        // one WebSocket for both: `draft.updated` only matters (and is only fetched) while the draft scene is up
        net::start_live_events(move |ev| {
            let draft_up = series.peek().as_ref().is_some_and(|s| s.scene == Scene::Draft);
            match ev {
                "draft.updated" if draft_up => fetch_draft(),
                "draft.updated" => {}
                _ => {
                    fetch_cast();
                    if draft_up {
                        fetch_draft();
                    }
                }
            }
        });
        // clock for the slideshow and the replay timer, 250 ms like the React hook
        Interval::new(250, move || now.set(net::now_ms())).forget();
        // reference data for the portraits (`useDimHeroes`)
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(m) = net::get_json::<HashMap<String, DimHero>>("/api/dim/heroes").await {
                dim.set(Dim::new(m));
            }
        });
    });

    // the draft scene appears: read its state
    let draft_up = series.read().as_ref().is_some_and(|s| s.scene == Scene::Draft);
    use_effect(use_reactive!(|draft_up| {
        if draft_up {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(d) = net::fetch_draft().await {
                    let mut draft = draft;
                    draft.set(Some(d));
                }
            });
        }
    }));

    use_effect(move || {
        let s = series.read().clone();
        let cur = layers.peek().clone();
        let next = scene_transition(&cur, s.as_ref());
        if next != cur {
            layers.set(next);
        }
    });
    // drop the outgoing layer once its fade is done
    use_effect(move || {
        let leaving = layers.read().leaving.as_ref().map(|l| l.id);
        if let Some(id) = leaving {
            spawn(async move {
                TimeoutFuture::new(FADE_MS).await;
                layers.with_mut(|l| {
                    if l.leaving.as_ref().is_some_and(|x| x.id == id) {
                        l.leaving = None;
                    }
                });
            });
        }
    });

    let skin = spike_common::scenes::skin(&net::search());
    let ids: Vec<u64> = {
        let l = layers.read();
        l.leaving.iter().chain(l.cur.iter()).map(|x| x.id).collect()
    };
    rsx! {
        document::Link { rel: "stylesheet", href: CSS }
        document::Link { rel: "stylesheet", href: POSTGAME_CSS }
        document::Link { rel: "stylesheet", href: DRAFT_CSS }
        div { class: "cast-overlay", "data-skin": skin,
            for id in ids {
                Layer { key: "{id}", id, layers, skin }
            }
        }
    }
}

/// One DOM layer per id: the node stays when the layer goes from current to leaving, only `out` flips.
#[component]
fn Layer(id: u64, layers: Signal<Layers>, skin: &'static str) -> Element {
    let found = {
        let l = layers.read();
        l.cur
            .iter()
            .find(|x| x.id == id)
            .map(|x| (x.clone(), false))
            .or_else(|| l.leaving.iter().find(|x| x.id == id).map(|x| (x.clone(), true)))
    };
    let Some((layer, out)) = found else { return rsx! {} };
    rsx! {
        div { class: if out { "scene out" } else { "scene" }, "aria-hidden": out.then_some("true"),
            match layer.key {
                Scene::Ingame => rsx! { InGame { s: layer.s } },
                Scene::Series => rsx! {
                    div { class: "scene-backdrop", "data-backdrop": true, div { class: "stars" } }
                    SeriesScene { s: layer.s }
                },
                Scene::Lineups => rsx! {
                    div { class: "scene-backdrop", "data-backdrop": true, div { class: "stars" } }
                    Lineups { s: layer.s }
                },
                // paints its own background (`.stage`): no backdrop
                Scene::Draft => rsx! { DraftScene { skin } },
                _ => rsx! {},
            }
        }
    }
}

/// Hero portrait, initials when the reference data does not know the hero (`portrait.tsx`).
#[component]
fn HeroPortrait(hero: String, #[props(into, default)] class: String, #[props(into, default)] title: String) -> Element {
    let dim = use_context::<Signal<Dim>>();
    let title = if title.is_empty() { hero.clone() } else { title };
    let icon = dim.read().icon(&hero).map(str::to_string);
    match icon {
        Some(src) => rsx! { img { class: "hp-i {class}", src, alt: "{hero}", title: "{title}" } },
        None => rsx! { i { class: "hp-i ph {class}", title: "{title}", "{team_initials(&hero)}" } },
    }
}

#[component]
fn InGame(s: CastSeries) -> Element {
    let (sb, sr) = series_score(&s);
    rsx! {
        div { class: "tab b", style: tab_box(Side::Blue),
            span { class: "n", "{display_name(&s.blue.name, Side::Blue)}" }
            span { class: "s", "{sb}" }
        }
        div { class: "tab r", style: tab_box(Side::Red),
            span { class: "s", "{sr}" }
            span { class: "n", "{display_name(&s.red.name, Side::Red)}" }
        }
        div { class: "badge",
            div { class: "bo", "BO{s.bo}", small { "GAME {badge_game_number(&s)}" } }
            div { class: "fl", "{s.format.to_uppercase()}" }
        }
    }
}

// ── series: map strip + post-game slides ───────────────────────────────────────

#[component]
fn SeriesScene(s: CastSeries) -> Element {
    let now = use_context::<Signal<u64>>();
    let idx = slide_index(&s, slide_tick(&s, now()));
    // keyed by the slide index: the frame is rebuilt (and its fade replayed) only when the slide changes
    rsx! {
        div { class: "sr-frame", key: "{idx}",
            if idx > 0 {
                PostGame { s, tab: idx - 1 }
            } else {
                MapStrip { s }
            }
        }
    }
}

fn card_class(v: &MapCardView) -> String {
    format!("mc {}{}", v.tone.class(), if v.struck { " struck" } else { "" })
}

#[component]
fn MapStrip(s: CastSeries) -> Element {
    let (b, r) = (display_name(&s.blue.name, Side::Blue), display_name(&s.red.name, Side::Red));
    let (sb, sr) = series_score(&s);
    let (cols, rows) = strip_layout(s.pool.len());
    let class = match rows {
        1 => "sr-strip",
        2 => "sr-strip two-rows",
        _ => "sr-strip three-rows",
    };
    let next = startable_maps(&s).first().copied();
    let cards: Vec<_> = s
        .pool
        .iter()
        .map(|m| (m.name.clone(), map_card_view(m, &s.games, next, (&b, &r))))
        .collect();
    rsx! {
        div { class: "sr-head",
            div { class: "sr-ttl", "BEST OF {s.bo} · GAME {badge_game_number(&s)}" }
            div { class: "sr-teams",
                span { class: "b", "{b}" }
                span { class: "s", "{sb} – {sr}" }
                span { class: "r", "{r}" }
            }
        }
        div { class, style: "--n:{cols}", "data-rows": "{rows}",
            for (name, v) in cards {
                article { key: "{name}", class: card_class(&v),
                    div { class: "art", style: "background-image:url({map_image(&name)})" }
                    div { class: "tx",
                        b { "{name}" }
                        if !v.label.is_empty() {
                            i { class: "lb", "{v.label}" }
                        }
                    }
                }
            }
        }
    }
}

/// Team logo or initials.
fn logo(t: &Team, name: &str) -> Element {
    match &t.logo {
        Some(l) => rsx! { img { src: "{l}", alt: "" } },
        None => rsx! { "{team_initials(name)}" },
    }
}

fn pg_side(s: &CastSeries, side: Side, won: bool, score: u32) -> Element {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let name = display_name(&t.name, side);
    let class = format!("pg-side {}", if side == Side::Blue { "b" } else { "r" });
    let tag = format!("pg-tag {}", if won { "w" } else { "l" });
    rsx! {
        div { class,
            div { class: "pg-logo", {logo(t, &name)} }
            div { class: "pg-id",
                div { class: "pg-nm", "{name}" }
                span { class: tag, if won { "WIN" } else { "LOSS" } }
            }
            b { class: "pg-sc", "{score}" }
        }
    }
}

fn pg_stat(label: &'static str, a: String, c: String) -> Element {
    rsx! { div { class: "pg-st", b { class: "a", "{a}" } span { "{label}" } b { class: "c", "{c}" } } }
}

fn pg_pair(label: &'static str, v: Option<(u32, u32)>) -> Element {
    match v {
        Some((a, c)) => pg_stat(label, a.to_string(), c.to_string()),
        None => rsx! {},
    }
}

/// Post-game breakdown (`PostGameSlide.tsx`): every value comes from the replay summary; an absent value
/// removes its row, never an invented 0.
#[component]
fn PostGame(s: CastSeries, tab: usize) -> Element {
    let Some((g, sm)) = shown_game(&s).and_then(|g| g.summary.as_ref().map(|sm| (g, sm))) else { return rsx! {} };
    let (sb, sr) = series_score(&s);
    let sum = |side, f: fn(&PlayerSummary) -> f64| side_sum(sm, side, f);
    let kda = |side| format!("{} / {} / {}", sum(side, |p| p.kills as f64), sum(side, |p| p.deaths as f64), sum(side, |p| p.assists as f64));
    let has_players = !sm.players.is_empty();
    let has_bans = sm.bans.0.len() + sm.bans.1.len() > 0;
    let (blue_rows, red_rows) = (bars_side(sm, Side::Blue, tab), bars_side(sm, Side::Red, tab));
    let fmt = |p: &PlayerSummary| if tab == 3 { format!("{}", tab_value(p, tab) as i64) } else { fmt_bar(tab_value(p, tab)) };
    let width = |p: &PlayerSummary| format!("width:{}%", bar_width(sm, p, tab));
    let rows = blue_rows.len().max(red_rows.len());
    let xp = (sm.xp_lead.len() > 1).then(|| xp_paths(&sm.xp_lead, sm.duration_secs));
    let blue_name = display_name(&s.blue.name, Side::Blue);
    rsx! {
        div { class: "pg", "data-tab": TAB_KEYS[tab],
            div { class: "pg-hd",
                {pg_side(&s, Side::Blue, sm.winner == Side::Blue, sm.takedowns.0)}
                div { class: "pg-ctr",
                    div { class: "t", "{fmt_clock(sm.duration_secs)}" }
                    div { class: "m", "GAME {g.number} · {g.map.to_uppercase()}" }
                }
                {pg_side(&s, Side::Red, sm.winner == Side::Red, sm.takedowns.1)}
            }
            div { class: "pg-bar1", "BEST OF {s.bo} · SERIES {sb} – {sr} · TAKEDOWNS" }
            div { class: "pg-left",
                if has_players { {pg_stat("K / D / A", kda(Side::Blue), kda(Side::Red))} }
                {pg_pair("TEAM LEVEL", sm.team_level)}
                {pg_pair("FORTS · KEEPS", sm.structures)}
                {pg_pair("MERC CAMPS", sm.merc_camps)}
                {pg_pair("MAP OBJECTIVES", sm.objectives)}
                if has_players { {pg_stat("SIEGE DAMAGE", fmt_stat(sum(Side::Blue, |p| p.siege_damage)), fmt_stat(sum(Side::Red, |p| p.siege_damage)))} }
                if has_players { {pg_stat("HERO DAMAGE", fmt_stat(sum(Side::Blue, |p| p.hero_damage)), fmt_stat(sum(Side::Red, |p| p.hero_damage)))} }
                if has_bans {
                    div { class: "pg-bans",
                        div { for h in sm.bans.0.iter() { HeroPortrait { key: "{h}", hero: h.clone(), class: "bn" } } }
                        span { "BANS" }
                        div { class: "c", for h in sm.bans.1.iter() { HeroPortrait { key: "{h}", hero: h.clone(), class: "bn" } } }
                    }
                }
            }
            div { class: "pg-right",
                div { class: "pg-panel",
                    div { class: "pg-tabs",
                        for (k, l) in TAB_LABELS.iter().enumerate() {
                            span { key: "{k}", class: if k == tab { "on" }, "{l}" }
                        }
                    }
                    for i in 0..rows {
                        div { key: "{i}", class: "pg-dm",
                            div { class: "bl",
                                if let Some(b) = blue_rows.get(i) {
                                    em { "{fmt(b)}" }
                                    span { class: "tr", i { style: width(b) } }
                                    HeroPortrait { hero: b.hero.clone(), title: format!("{} · {}", b.name, b.hero) }
                                }
                            }
                            div { class: "rd",
                                if let Some(r) = red_rows.get(i) {
                                    HeroPortrait { hero: r.hero.clone(), title: format!("{} · {}", r.name, r.hero) }
                                    span { class: "tr", i { style: width(r) } }
                                    em { "{fmt(r)}" }
                                }
                            }
                        }
                    }
                }
                if let Some((line, area)) = xp {
                    div { class: "pg-panel pg-gr",
                        h4 { "EXPERIENCE ADVANTAGE OVER TIME" }
                        div { class: "pg-svg",
                            svg { view_box: "0 0 400 110", preserve_aspect_ratio: "none", role: "img", "aria-label": "Experience advantage over time",
                                defs {
                                    clipPath { id: "pg-up", rect { x: "0", y: "0", width: "400", height: "55" } }
                                    clipPath { id: "pg-dn", rect { x: "0", y: "55", width: "400", height: "55" } }
                                }
                                line { x1: "0", y1: "55", x2: "400", y2: "55", class: "pg-zero" }
                                path { d: "{area}", class: "pg-area b", clip_path: "url(#pg-up)" }
                                path { d: "{area}", class: "pg-area r", clip_path: "url(#pg-dn)" }
                                path { d: "{line}", class: "pg-line" }
                            }
                        }
                        div { class: "pg-cap", "above the line = {blue_name} ahead · 0:00 → {fmt_clock(sm.duration_secs)}" }
                    }
                }
            }
        }
    }
}

// ── lineups ────────────────────────────────────────────────────────────────────

/// Team header of a lineup card.
fn lu_top(t: &Team, side: Side, sub: String) -> Element {
    let name = display_name(&t.name, side);
    rsx! {
        div { class: "lu-top",
            div { class: "lu-lg", {logo(t, &name)} }
            div { class: "lu-id",
                div { class: "lu-nm", "{name}" }
                if !sub.is_empty() { div { class: "lu-sub", "{sub}" } }
            }
        }
    }
}

#[component]
fn Lineups(s: CastSeries) -> Element {
    // recent heroes of the linked players: fetched again only when the toons change
    let query = lineup_query(&s);
    let heroes = use_resource(use_reactive!(|query| async move {
        match query {
            Some(q) => net::fetch_lineup(&q).await.ok(),
            None => None,
        }
    }));
    let recent: Option<CastLineup> = heroes.read().clone().flatten();
    let mode = effective_lineup_mode(&s);
    let (sb, sr) = series_score(&s);
    let done = s.games.iter().filter(|g| g.winner.is_some()).count();
    let title = match mode {
        Mode::A => format!("BEST OF {} · {} · LINEUPS", s.bo, s.format.to_uppercase()),
        Mode::B => format!("AFTER GAME {done} · SERIES {sb} – {sr}{}", if s.format == "fearless" { " · FEARLESS HEROES LOCKED" } else { "" }),
    };
    let (class, mc) = if mode == Mode::A { ("lu lu-a", "A") } else { ("lu lu-b", "B") };
    let vs = if mode == Mode::A { "VS".to_string() } else { format!("{sb} – {sr}") };
    rsx! {
        div { class, "data-mode": mc,
            div { class: "lu-title", "{title}" }
            div { class: "lu-vs", "{vs}" }
            for side in [Side::Blue, Side::Red] {
                if mode == Mode::A { {mode_a(&s, side, &recent)} } else { {mode_b(&s, side)} }
            }
        }
    }
}

fn captain(t: &Team, i: usize) -> Element {
    if t.captain == Some(i) {
        rsx! { span { class: "lu-cap", "CAPTAIN" } }
    } else {
        rsx! {}
    }
}

fn mode_a(s: &CastSeries, side: Side, lineup: &Option<CastLineup>) -> Element {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let recent: Vec<Vec<HeroCount>> = lineup.as_ref().map(|l| if side == Side::Blue { l.blue.clone() } else { l.red.clone() }).unwrap_or_default();
    let class = format!("lu-card {}", if side == Side::Blue { "b" } else { "r" });
    rsx! {
        section { class,
            {lu_top(t, side, t.record.trim().to_uppercase())}
            for (i, p) in t.players.iter().enumerate() {
                {
                    // unlinked player (or < 5 recent games: empty list server side) -> no hero row
                    let hs: Vec<HeroCount> = if p.toon.is_some() { recent.get(i).cloned().unwrap_or_default() } else { vec![] };
                    rsx! {
                        div { key: "{i}", class: "lu-pl",
                            div { class: "lu-pn", "{player_name(&p.name, i)}"
                                if !p.tagline.trim().is_empty() { span { class: "lu-st", "{p.tagline}" } }
                            }
                            if !hs.is_empty() {
                                div { class: "lu-hp",
                                    for h in hs { HeroPortrait { key: "{h.hero}", hero: h.hero.clone(), title: format!("{} · {}", h.hero, h.games) } }
                                }
                            }
                            {captain(t, i)}
                        }
                    }
                }
            }
        }
    }
}

fn mode_b(s: &CastSeries, side: Side) -> Element {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let played = series_player_heroes(s, side);
    let last_with = played.iter().flatten().map(|h| h.game).max().unwrap_or(0);
    let results = s
        .games
        .iter()
        .filter_map(|g| g.winner.map(|w| format!("G{} {}", g.number, if w == side { "WON" } else { "LOST" })))
        .collect::<Vec<_>>()
        .join(" · ");
    let fearless = s.format == "fearless";
    let class = format!("lu-card {}", if side == Side::Blue { "b" } else { "r" });
    rsx! {
        section { class,
            {lu_top(t, side, results)}
            for (i, p) in t.players.iter().enumerate() {
                {
                    let hs = played.get(i).cloned().unwrap_or_default();
                    let sub = if hs.is_empty() { "awaiting game data".to_string() } else { hs.iter().map(|h| format!("G{} {}", h.game, h.hero)).collect::<Vec<_>>().join(" · ") };
                    rsx! {
                        div { key: "{i}", class: "lu-pl",
                            div { class: "lu-pn", "{player_name(&p.name, i)}", span { class: "lu-st", "{sub}" } }
                            if !hs.is_empty() {
                                div { class: "lu-hp",
                                    for h in hs.iter() {
                                        {
                                            // Fearless: a hero from an earlier game is locked (dimmed); the last game's stays highlighted
                                            let locked = fearless && h.game < last_with;
                                            let title = format!("G{} {}{}", h.game, h.hero, if locked { " (locked)" } else { "" });
                                            rsx! { HeroPortrait { key: "{h.game}", hero: h.hero.clone(), class: if locked { "lk" } else { "cur" }, title } }
                                        }
                                    }
                                }
                            }
                            {captain(t, i)}
                        }
                    }
                }
            }
        }
    }
}

// ── draft ──────────────────────────────────────────────────────────────────────

/// Hero portrait that fills its container (splash), initials when there is no image.
#[component]
fn Splash(hero: String) -> Element {
    let dim = use_context::<Signal<Dim>>();
    let icon = dim.read().icon(&hero).map(str::to_string);
    match icon {
        Some(src) => rsx! { div { class: "por", style: "background-image:url({src})" } },
        None => rsx! { div { class: "por none", "{draft_initials(&hero)}" } },
    }
}

/// Draft scene: its own background (`.stage`), skin from `?skin=`.
#[component]
fn DraftScene(skin: &'static str) -> Element {
    let draft = use_context::<Signal<Option<DraftState>>>();
    let d = draft.read().clone();
    rsx! {
        div { class: "draft-overlay", "data-skin": skin,
            div { class: "stage", div { class: "stars" }
                if let Some(d) = d { Timeline { d } }
            }
        }
    }
}

#[component]
fn Timeline(d: DraftState) -> Element {
    let now = use_context::<Signal<u64>>();
    // local 45 s countdown, restarted when the phase changes
    let phase = phase_start(&d);
    let mut secs = use_signal(|| PHASE_SECONDS);
    let mut over = use_signal(|| phase.is_none());
    use_effect(use_reactive!(|phase| {
        over.set(phase.is_none());
        secs.set(PHASE_SECONDS);
    }));
    // dropped (and so stopped) when the timeline unmounts
    use_hook(move || {
        std::rc::Rc::new(Interval::new(1000, move || {
            if !over() {
                secs.set((secs() - 1.0).max(0.0));
            }
        }))
    });

    let active_side = d.steps.get(d.cursor).map(|st| side_of_step(&d, st));
    let w = timer_fraction(&d, now() as f64, secs(), PHASE_SECONDS) * 100.0;
    let timer = format!("width:{w}%;transition:{}", if in_replay(&d) { "width .25s linear" } else { "none" });
    let picks = pick_slots(&d);
    let bans = ban_slots(&d);
    let map = format!("{}{}", d.map, if d.bo > 1 { format!(" — Bo{}", d.bo) } else { String::new() });
    let fearless = d.format == "fearless" && !d.series_bans.is_empty();
    let row = |side: Side| -> Element {
        rsx! {
            for m in picks.iter() {
                if m.side != side {
                    div { key: "{m.index}", class: "pk ghost" }
                } else {
                    {
                        let hero = d.assignments.get(m.index).cloned().flatten();
                        let team = if side == Side::Blue { &d.blue } else { &d.red };
                        let player = team.players.get(m.ordinal).cloned().unwrap_or_default();
                        let active = m.index == d.cursor;
                        let label = hero.clone().unwrap_or_else(|| if active { "…".into() } else { "—".into() });
                        let class = format!("pk {} {}", if hero.is_some() { "" } else { "empty" }, if active { "active" } else { "" });
                        rsx! {
                            div { key: "{m.index}", class,
                                if let Some(h) = hero { Splash { hero: h } }
                                span { class: "nm", b { "{label}" } if !player.is_empty() { i { "{player}" } } }
                            }
                        }
                    }
                }
            }
        }
    };
    rsx! {
        div { class: "tl-head",
            div { class: "map", "{map}" }
            div { class: "teams",
                span { class: "b", if active_side == Some(Side::Blue) { span { class: "dot" } } "{d.blue.name}" }
                span { class: "s", "{d.score.0} — {d.score.1}" }
                span { class: "r", "{d.red.name}" if active_side == Some(Side::Red) { span { class: "dot" } } }
            }
            if is_replay_ready(&d) { div { class: "ready", "Draft ready — waiting" } }
            div { class: "tl-timer", span { style: timer } }
        }
        if fearless {
            div { class: "tl-series",
                div { class: "lab", "Series bans · {d.series_bans.len()}" }
                div { class: "pool", for h in d.series_bans.iter() { span { key: "{h}", class: "si", Splash { hero: h.clone() } } } }
            }
        }
        div { class: "tl-stack",
            div { class: "tl-picks b", {row(Side::Blue)} }
            div { class: "tl-bans",
                for (i, side, sep) in bans {
                    if sep { span { key: "sep{i}", class: "sep" } }
                    {
                        let hero = d.assignments.get(i).cloned().flatten();
                        let class = format!("bn {} {} {}", if side == Side::Blue { "blue" } else { "red" }, if hero.is_some() { "" } else { "empty" }, if i == d.cursor { "active" } else { "" });
                        rsx! { span { key: "{i}", class, if let Some(h) = hero { Splash { hero: h } } } }
                    }
                }
            }
            div { class: "tl-picks r", {row(Side::Red)} }
        }
    }
}
