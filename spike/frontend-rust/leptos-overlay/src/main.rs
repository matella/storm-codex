//! Cast overlay (`/cast/overlay`), Leptos 0.8 CSR. All scenes: `ingame`, `series` (map strip + post-game
//! slides), `lineups`, `draft`; transparent body, 300 ms cross-fade that keeps the outgoing scene mounted,
//! slideshow driven by the server anchor, `/ws` live updates with reconnection.
//! Port of `web/src/pages/CastOverlay.tsx` and `web/src/cast/*Scene.tsx`.

use leptos::{prelude::*, task::spawn_local};
use spike_common::{
    logic::*,
    model::*,
    net,
    profile::{Dim, DimHero},
    scenes::*,
};
use std::time::Duration;

const FADE_MS: u64 = 300;

fn main() {
    net::set_body_transparent();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (series, set_series) = signal(None::<CastSeries>);
    let draft = RwSignal::new(None::<DraftState>);
    let fetch_cast = move || {
        spawn_local(async move {
            if let Ok(s) = net::fetch_cast().await {
                set_series.set(Some(s));
            }
        })
    };
    let fetch_draft = move || {
        spawn_local(async move {
            if let Ok(d) = net::fetch_draft().await {
                draft.set(Some(d));
            }
        })
    };
    fetch_cast();
    // one WebSocket for both: `draft.updated` only matters (and is only fetched) while the draft scene is up
    net::start_live_events(move |ev| {
        let draft_up = series.get_untracked().is_some_and(|s| s.scene == Scene::Draft);
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
    provide_context(draft);
    // the draft scene appears: read its state
    Effect::new(move |prev: Option<bool>| {
        let up = series.with(|s| s.as_ref().is_some_and(|s| s.scene == Scene::Draft));
        if up && prev != Some(true) {
            fetch_draft();
        }
        up
    });

    // reference data for the portraits (`useDimHeroes`)
    let dim = RwSignal::new(Dim::default());
    provide_context(dim);
    spawn_local(async move {
        if let Ok(m) = net::get_json::<std::collections::HashMap<String, DimHero>>("/api/dim/heroes").await {
            dim.set(Dim::new(m));
        }
    });

    // clock for the slideshow and the replay timer, 250 ms like the React hook
    let (now, set_now) = signal(net::now_ms());
    set_interval(move || set_now.set(net::now_ms()), Duration::from_millis(250));
    provide_context(now);

    let (layers, set_layers) = signal(Layers::default());
    Effect::new(move |_| {
        let s = series.get();
        let cur = layers.get_untracked();
        let next = scene_transition(&cur, s.as_ref());
        if next != cur {
            set_layers.set(next);
        }
    });
    // drop the outgoing layer once its fade is done
    Effect::new(move |_| {
        if let Some(id) = layers.with(|l| l.leaving.as_ref().map(|x| x.id)) {
            set_timeout(
                move || set_layers.update(|l| if l.leaving.as_ref().is_some_and(|x| x.id == id) { l.leaving = None }),
                Duration::from_millis(FADE_MS),
            );
        }
    });

    let search = net::search();
    let skin_name = spike_common::scenes::skin(&search);
    provide_context(skin_name.to_string());
    let ids = move || {
        layers.with(|l| l.leaving.iter().chain(l.cur.iter()).map(|x| x.id).collect::<Vec<_>>())
    };
    view! {
        <div class="cast-overlay" data-skin=skin_name>
            <For each=ids key=|id| *id children=move |id| view! { <Layer id layers /> } />
        </div>
    }
}

/// One DOM layer per id: the node stays when the layer goes from current to leaving, only `out` flips.
#[component]
fn Layer(id: u64, layers: ReadSignal<Layers>) -> impl IntoView {
    let find = move |l: &Layers| {
        l.cur.iter().find(|x| x.id == id).map(|x| (x.clone(), false))
            .or_else(|| l.leaving.iter().find(|x| x.id == id).map(|x| (x.clone(), true)))
    };
    let found = Memo::new(move |prev: Option<&Option<(SceneLayer, bool)>>| {
        layers.with(find).or_else(|| prev.cloned().flatten())
    });
    let key = found.get_untracked().map(|(l, _)| l.key);
    let s = Signal::derive(move || found.get().map(|(l, _)| l.s).expect("layer exists while mounted"));
    let out = move || found.get().is_some_and(|(_, out)| out);
    let backdrop = || view! { <div class="scene-backdrop" data-backdrop><div class="stars"></div></div> };
    view! {
        <div class="scene" class:out=out aria-hidden=move || out().then_some("true")>
            {match key {
                Some(Scene::Ingame) => view! { <InGame s /> }.into_any(),
                Some(Scene::Series) => view! { {backdrop()} <SeriesScene s /> }.into_any(),
                Some(Scene::Lineups) => view! { {backdrop()} <Lineups s /> }.into_any(),
                // paints its own background (`.stage`): no backdrop
                Some(Scene::Draft) => view! { <DraftScene /> }.into_any(),
                _ => ().into_any(),
            }}
        </div>
    }
}

/// Hero portrait, initials when the reference data does not know the hero (`portrait.tsx`).
#[component]
fn HeroPortrait(hero: String, #[prop(optional)] class: &'static str, #[prop(optional, into)] title: Option<String>) -> impl IntoView {
    let dim = expect_context::<RwSignal<Dim>>();
    let title = title.unwrap_or_else(|| hero.clone());
    move || match dim.with(|d| d.icon(&hero).map(str::to_string)) {
        Some(src) => view! { <img class=format!("hp-i {class}") src=src alt=hero.clone() title=title.clone() /> }.into_any(),
        None => view! { <i class=format!("hp-i ph {class}") title=title.clone()>{team_initials(&hero)}</i> }.into_any(),
    }
}

#[component]
fn InGame(s: Signal<CastSeries>) -> impl IntoView {
    let blue = move || display_name(&s.with(|s| s.blue.name.clone()), Side::Blue);
    let red = move || display_name(&s.with(|s| s.red.name.clone()), Side::Red);
    let score = move || s.with(series_score);
    view! {
        <div class="tab b" style=tab_box(Side::Blue)>
            <span class="n">{blue}</span><span class="s">{move || score().0}</span>
        </div>
        <div class="tab r" style=tab_box(Side::Red)>
            <span class="s">{move || score().1}</span><span class="n">{red}</span>
        </div>
        <div class="badge">
            <div class="bo">{move || format!("BO{}", s.with(|s| s.bo))}
                <small>{move || format!("GAME {}", s.with(badge_game_number))}</small></div>
            <div class="fl">{move || s.with(|s| s.format.to_uppercase())}</div>
        </div>
    }
}

// ── series: map strip + post-game slides ───────────────────────────────────────

#[component]
fn SeriesScene(s: Signal<CastSeries>) -> impl IntoView {
    let now = expect_context::<ReadSignal<u64>>();
    let idx = Memo::new(move |_| s.with(|s| slide_index(s, slide_tick(s, now.get()))));
    // the frame is rebuilt (and its fade replayed) only when the slide index changes
    move || {
        let i = idx.get();
        // a slide > 0 exists only when the shown game has a summary
        let body = if i > 0 {
            view! { <PostGame s tab=i - 1 /> }.into_any()
        } else {
            view! { <MapStrip s /> }.into_any()
        };
        view! { <div class="sr-frame">{body}</div> }
    }
}

#[component]
fn MapStrip(s: Signal<CastSeries>) -> impl IntoView {
    let names = move || s.with(|s| (display_name(&s.blue.name, Side::Blue), display_name(&s.red.name, Side::Red)));
    let layout = move || strip_layout(s.with(|s| s.pool.len()));
    let rows_class = move || match layout().1 {
        1 => "sr-strip",
        2 => "sr-strip two-rows",
        _ => "sr-strip three-rows",
    };
    let cards = move || {
        let n = names();
        s.with(|s| {
            let next = startable_maps(s).first().copied();
            s.pool.iter().map(|m| (m.name.clone(), map_card_view(m, &s.games, next, (&n.0, &n.1)))).collect::<Vec<_>>()
        })
    };
    view! {
        <div class="sr-head">
            <div class="sr-ttl">{move || s.with(|s| format!("BEST OF {} · GAME {}", s.bo, badge_game_number(s)))}</div>
            <div class="sr-teams">
                <span class="b">{move || names().0}</span>
                <span class="s">{move || { let (b, r) = s.with(series_score); format!("{b} – {r}") }}</span>
                <span class="r">{move || names().1}</span>
            </div>
        </div>
        <div class=rows_class style=move || format!("--n:{}", layout().0) data-rows=move || layout().1>
            <For each=cards key=|(name, v)| (name.clone(), v.clone()) children=|(name, v)| {
                let art = format!("background-image:url({})", map_image(&name));
                let class = format!("mc {}{}", v.tone.class(), if v.struck { " struck" } else { "" });
                view! {
                    <article class=class>
                        <div class="art" style=art></div>
                        <div class="tx"><b>{name}</b>{(!v.label.is_empty()).then(|| view! { <i class="lb">{v.label}</i> })}</div>
                    </article>
                }
            } />
        </div>
    }
}

/// Team block of the post-game header.
fn pg_side(s: &CastSeries, side: Side, won: bool, score: u32) -> impl IntoView {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let name = display_name(&t.name, side);
    let logo = match &t.logo {
        Some(l) => view! { <img src=l.clone() alt="" /> }.into_any(),
        None => team_initials(&name).into_any(),
    };
    view! {
        <div class=format!("pg-side {}", if side == Side::Blue { "b" } else { "r" })>
            <div class="pg-logo">{logo}</div>
            <div class="pg-id">
                <div class="pg-nm">{name}</div>
                <span class=format!("pg-tag {}", if won { "w" } else { "l" })>{if won { "WIN" } else { "LOSS" }}</span>
            </div>
            <b class="pg-sc">{score}</b>
        </div>
    }
}

fn pg_stat(label: &'static str, a: String, c: String) -> impl IntoView {
    view! { <div class="pg-st"><b class="a">{a}</b><span>{label}</span><b class="c">{c}</b></div> }
}

fn pg_pair(label: &'static str, v: Option<(u32, u32)>) -> Option<impl IntoView> {
    v.map(|(a, c)| pg_stat(label, a.to_string(), c.to_string()))
}

/// Post-game breakdown (`PostGameSlide.tsx`): every value comes from the replay summary; an absent value
/// removes its row, never an invented 0.
#[component]
fn PostGame(s: Signal<CastSeries>, tab: usize) -> impl IntoView {
    move || {
        s.with(|s| {
            let g = shown_game(s)?;
            let sm = g.summary.as_ref()?;
            let (sb, sr) = series_score(s);
            let sum = |side, f: fn(&PlayerSummary) -> f64| side_sum(sm, side, f);
            let kda = |side| format!("{} / {} / {}", sum(side, |p| p.kills as f64), sum(side, |p| p.deaths as f64), sum(side, |p| p.assists as f64));
            let has_players = !sm.players.is_empty();
            let has_bans = sm.bans.0.len() + sm.bans.1.len() > 0;
            let bans = |v: &Vec<String>| v.iter().map(|h| view! { <HeroPortrait hero=h.clone() class="bn" /> }).collect_view();
            let bars = (0..sm.players.iter().filter(|p| p.side == Some(Side::Blue)).count().max(sm.players.iter().filter(|p| p.side == Some(Side::Red)).count()))
                .map(|i| {
                    let (b, r) = (bars_side(sm, Side::Blue, tab).get(i).copied(), bars_side(sm, Side::Red, tab).get(i).copied());
                    let fmt = |p: &PlayerSummary| if tab == 3 { format!("{}", tab_value(p, tab) as i64) } else { fmt_bar(tab_value(p, tab)) };
                    let w = |p: &PlayerSummary| format!("width:{}%", bar_width(sm, p, tab));
                    view! {
                        <div class="pg-dm">
                            <div class="bl">{b.map(|b| view! {
                                <em>{fmt(b)}</em><span class="tr"><i style=w(b)></i></span>
                                <HeroPortrait hero=b.hero.clone() title=format!("{} · {}", b.name, b.hero) />
                            })}</div>
                            <div class="rd">{r.map(|r| view! {
                                <HeroPortrait hero=r.hero.clone() title=format!("{} · {}", r.name, r.hero) />
                                <span class="tr"><i style=w(r)></i></span><em>{fmt(r)}</em>
                            })}</div>
                        </div>
                    }
                })
                .collect_view();
            let xp = (sm.xp_lead.len() > 1).then(|| {
                let (line, area) = xp_paths(&sm.xp_lead, sm.duration_secs);
                view! {
                    <div class="pg-panel pg-gr">
                        <h4>"EXPERIENCE ADVANTAGE OVER TIME"</h4>
                        <div class="pg-svg">
                            <svg viewBox="0 0 400 110" preserveAspectRatio="none" role="img" aria-label="Experience advantage over time">
                                <defs>
                                    <clipPath id="pg-up"><rect x="0" y="0" width="400" height="55" /></clipPath>
                                    <clipPath id="pg-dn"><rect x="0" y="55" width="400" height="55" /></clipPath>
                                </defs>
                                <line x1="0" y1="55" x2="400" y2="55" class="pg-zero" />
                                <path d=area.clone() class="pg-area b" clip-path="url(#pg-up)" />
                                <path d=area class="pg-area r" clip-path="url(#pg-dn)" />
                                <path d=line class="pg-line" />
                            </svg>
                        </div>
                        <div class="pg-cap">{format!("above the line = {} ahead · 0:00 → {}", display_name(&s.blue.name, Side::Blue), fmt_clock(sm.duration_secs))}</div>
                    </div>
                }
            });
            Some(view! {
                <div class="pg" data-tab=TAB_KEYS[tab]>
                    <div class="pg-hd">
                        {pg_side(s, Side::Blue, sm.winner == Side::Blue, sm.takedowns.0)}
                        <div class="pg-ctr">
                            <div class="t">{fmt_clock(sm.duration_secs)}</div>
                            <div class="m">{format!("GAME {} · {}", g.number, g.map.to_uppercase())}</div>
                        </div>
                        {pg_side(s, Side::Red, sm.winner == Side::Red, sm.takedowns.1)}
                    </div>
                    <div class="pg-bar1">{format!("BEST OF {} · SERIES {sb} – {sr} · TAKEDOWNS", s.bo)}</div>
                    <div class="pg-left">
                        {has_players.then(|| pg_stat("K / D / A", kda(Side::Blue), kda(Side::Red)))}
                        {pg_pair("TEAM LEVEL", sm.team_level)}
                        {pg_pair("FORTS · KEEPS", sm.structures)}
                        {pg_pair("MERC CAMPS", sm.merc_camps)}
                        {pg_pair("MAP OBJECTIVES", sm.objectives)}
                        {has_players.then(|| pg_stat("SIEGE DAMAGE", fmt_stat(sum(Side::Blue, |p| p.siege_damage)), fmt_stat(sum(Side::Red, |p| p.siege_damage))))}
                        {has_players.then(|| pg_stat("HERO DAMAGE", fmt_stat(sum(Side::Blue, |p| p.hero_damage)), fmt_stat(sum(Side::Red, |p| p.hero_damage))))}
                        {has_bans.then(|| view! {
                            <div class="pg-bans"><div>{bans(&sm.bans.0)}</div><span>"BANS"</span><div class="c">{bans(&sm.bans.1)}</div></div>
                        })}
                    </div>
                    <div class="pg-right">
                        <div class="pg-panel">
                            <div class="pg-tabs">
                                {TAB_LABELS.iter().enumerate().map(|(k, l)| view! { <span class=(k == tab).then_some("on")>{*l}</span> }).collect_view()}
                            </div>
                            {bars}
                        </div>
                        {xp}
                    </div>
                </div>
            })
        })
    }
}

// ── lineups ────────────────────────────────────────────────────────────────────

/// Team header of a lineup card.
fn lu_top(t: &Team, side: Side, sub: String) -> impl IntoView {
    let name = display_name(&t.name, side);
    let logo = match &t.logo {
        Some(l) => view! { <img src=l.clone() alt="" /> }.into_any(),
        None => team_initials(&name).into_any(),
    };
    view! {
        <div class="lu-top">
            <div class="lu-lg">{logo}</div>
            <div class="lu-id">
                <div class="lu-nm">{name}</div>
                {(!sub.is_empty()).then(|| view! { <div class="lu-sub">{sub}</div> })}
            </div>
        </div>
    }
}

#[component]
fn Lineups(s: Signal<CastSeries>) -> impl IntoView {
    // recent heroes of the linked players: fetched again only when the toons change
    let query = Memo::new(move |_| s.with(lineup_query));
    let heroes = LocalResource::new(move || {
        let q = query.get();
        async move {
            match q {
                Some(q) => net::fetch_lineup(&q).await.ok(),
                None => None,
            }
        }
    });
    move || {
        s.with(|s| {
            let mode = effective_lineup_mode(s);
            let (sb, sr) = series_score(s);
            let done = s.games.iter().filter(|g| g.winner.is_some()).count();
            let title = match mode {
                Mode::A => format!("BEST OF {} · {} · LINEUPS", s.bo, s.format.to_uppercase()),
                Mode::B => format!(
                    "AFTER GAME {done} · SERIES {sb} – {sr}{}",
                    if s.format == "fearless" { " · FEARLESS HEROES LOCKED" } else { "" }
                ),
            };
            let (cls, mc) = if mode == Mode::A { ("lu lu-a", "A") } else { ("lu lu-b", "B") };
            let cards = [Side::Blue, Side::Red].map(|side| match mode {
                Mode::A => mode_a(s, side, heroes.get().flatten()).into_any(),
                Mode::B => mode_b(s, side).into_any(),
            });
            view! {
                <div class=cls data-mode=mc>
                    <div class="lu-title">{title}</div>
                    <div class="lu-vs">{if mode == Mode::A { "VS".to_string() } else { format!("{sb} – {sr}") }}</div>
                    {cards}
                </div>
            }
        })
    }
}

fn mode_a(s: &CastSeries, side: Side, lineup: Option<CastLineup>) -> impl IntoView {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let recent = lineup.map(|l| if side == Side::Blue { l.blue } else { l.red }).unwrap_or_default();
    let players = t.players.iter().enumerate().map(|(i, p)| {
        // unlinked player (or < 5 recent games: empty list server side) -> no hero row
        let hs: Vec<HeroCount> = if p.toon.is_some() { recent.get(i).cloned().unwrap_or_default() } else { vec![] };
        view! {
            <div class="lu-pl">
                <div class="lu-pn">{player_name(&p.name, i)}
                    {(!p.tagline.trim().is_empty()).then(|| view! { <span class="lu-st">{p.tagline.clone()}</span> })}
                </div>
                {(!hs.is_empty()).then(|| view! {
                    <div class="lu-hp">{hs.iter().map(|h| view! { <HeroPortrait hero=h.hero.clone() title=format!("{} · {}", h.hero, h.games) /> }).collect_view()}</div>
                })}
                {(t.captain == Some(i)).then(|| view! { <span class="lu-cap">"CAPTAIN"</span> })}
            </div>
        }
    });
    view! {
        <section class=format!("lu-card {}", if side == Side::Blue { "b" } else { "r" })>
            {lu_top(t, side, t.record.trim().to_uppercase())}
            {players.collect_view()}
        </section>
    }
}

fn mode_b(s: &CastSeries, side: Side) -> impl IntoView {
    let t = if side == Side::Blue { &s.blue } else { &s.red };
    let played = series_player_heroes(s, side);
    let last_with = played.iter().flatten().map(|h| h.game).max().unwrap_or(0);
    let results = s
        .games
        .iter()
        .filter_map(|g| g.winner.map(|w| format!("G{} {}", g.number, if w == side { "WON" } else { "LOST" })))
        .collect::<Vec<_>>()
        .join(" · ");
    let players = t.players.iter().enumerate().map(|(i, p)| {
        let hs = played.get(i).cloned().unwrap_or_default();
        let sub = if hs.is_empty() { "awaiting game data".to_string() } else { hs.iter().map(|h| format!("G{} {}", h.game, h.hero)).collect::<Vec<_>>().join(" · ") };
        let fearless = s.format == "fearless";
        view! {
            <div class="lu-pl">
                <div class="lu-pn">{player_name(&p.name, i)}<span class="lu-st">{sub}</span></div>
                {(!hs.is_empty()).then(|| view! {
                    <div class="lu-hp">{hs.iter().map(|h| {
                        // Fearless: a hero from an earlier game is locked (dimmed); the last game's stays highlighted
                        let locked = fearless && h.game < last_with;
                        view! { <HeroPortrait hero=h.hero.clone() class=if locked { "lk" } else { "cur" }
                            title=format!("G{} {}{}", h.game, h.hero, if locked { " (locked)" } else { "" }) /> }
                    }).collect_view()}</div>
                })}
                {(t.captain == Some(i)).then(|| view! { <span class="lu-cap">"CAPTAIN"</span> })}
            </div>
        }
    });
    view! {
        <section class=format!("lu-card {}", if side == Side::Blue { "b" } else { "r" })>
            {lu_top(t, side, results)}
            {players.collect_view()}
        </section>
    }
}

// ── draft ──────────────────────────────────────────────────────────────────────

/// Hero portrait that fills its container (splash), initials when there is no image.
#[component]
fn Splash(hero: String) -> impl IntoView {
    let dim = expect_context::<RwSignal<Dim>>();
    move || match dim.with(|d| d.icon(&hero).map(str::to_string)) {
        Some(src) => view! { <div class="por" style=format!("background-image:url({src})")></div> }.into_any(),
        None => view! { <div class="por none">{draft_initials(&hero)}</div> }.into_any(),
    }
}

/// Draft scene: its own background (`.stage`), skin from `?skin=`.
#[component]
fn DraftScene() -> impl IntoView {
    let draft = expect_context::<RwSignal<Option<DraftState>>>();
    let skin = expect_context::<String>();
    view! {
        <div class="draft-overlay" data-skin=skin>
            <div class="stage"><div class="stars"></div>
                {move || draft.get().map(|_| view! { <Timeline draft /> })}
            </div>
        </div>
    }
}

#[component]
fn Timeline(draft: RwSignal<Option<DraftState>>) -> impl IntoView {
    let now = expect_context::<ReadSignal<u64>>();
    let d = Signal::derive(move || draft.get().expect("draft is loaded while the timeline is mounted"));
    // local 45 s countdown, restarted when the phase changes
    let phase = Memo::new(move |_| d.with(phase_start));
    let (secs, set_secs) = signal(PHASE_SECONDS);
    Effect::new(move |_| {
        phase.track();
        set_secs.set(PHASE_SECONDS);
    });
    let handle = set_interval_with_handle(
        move || if phase.get_untracked().is_some() { set_secs.update(|s| *s = (*s - 1.0).max(0.0)) },
        Duration::from_secs(1),
    );
    on_cleanup(move || {
        if let Ok(h) = handle {
            h.clear()
        }
    });

    let active_side = move || d.with(|d| d.steps.get(d.cursor).map(|st| side_of_step(d, st)));
    let timer = move || {
        let w = d.with(|d| timer_fraction(d, now.get() as f64, secs.get(), PHASE_SECONDS)) * 100.0;
        let tr = if d.with(in_replay) { ";transition:width .25s linear" } else { "" };
        format!("width:{w}%{tr}")
    };
    let row = move |side: Side| {
        d.with(|d| {
            pick_slots(d)
                .into_iter()
                .map(|m| {
                    if m.side != side {
                        return view! { <div class="pk ghost"></div> }.into_any();
                    }
                    let hero = d.assignments.get(m.index).cloned().flatten();
                    let team = if side == Side::Blue { &d.blue } else { &d.red };
                    let player = team.players.get(m.ordinal).cloned().unwrap_or_default();
                    let active = m.index == d.cursor;
                    let label = hero.clone().unwrap_or_else(|| if active { "…".into() } else { "—".into() });
                    let cls = format!("pk {} {}", if hero.is_some() { "" } else { "empty" }, if active { "active" } else { "" });
                    view! {
                        <div class=cls>
                            {hero.map(|h| view! { <Splash hero=h /> })}
                            <span class="nm"><b>{label}</b>{(!player.is_empty()).then(|| view! { <i>{player}</i> })}</span>
                        </div>
                    }
                    .into_any()
                })
                .collect_view()
        })
    };
    let bans = move || {
        d.with(|d| {
            ban_slots(d)
                .into_iter()
                .flat_map(|(i, side, sep)| {
                    let hero = d.assignments.get(i).cloned().flatten();
                    let cls = format!(
                        "bn {} {} {}",
                        if side == Side::Blue { "blue" } else { "red" },
                        if hero.is_some() { "" } else { "empty" },
                        if i == d.cursor { "active" } else { "" }
                    );
                    [
                        sep.then(|| view! { <span class="sep"></span> }.into_any()),
                        Some(view! { <span class=cls>{hero.map(|h| view! { <Splash hero=h /> })}</span> }.into_any()),
                    ]
                })
                .flatten()
                .collect_view()
        })
    };
    view! {
        <div class="tl-head">
            <div class="map">{move || d.with(|d| format!("{}{}", d.map, if d.bo > 1 { format!(" — Bo{}", d.bo) } else { String::new() }))}</div>
            <div class="teams">
                <span class="b">{move || (active_side() == Some(Side::Blue)).then(|| view! { <span class="dot"></span> })}{move || d.with(|d| d.blue.name.clone())}</span>
                <span class="s">{move || d.with(|d| format!("{} — {}", d.score.0, d.score.1))}</span>
                <span class="r">{move || d.with(|d| d.red.name.clone())}{move || (active_side() == Some(Side::Red)).then(|| view! { <span class="dot"></span> })}</span>
            </div>
            {move || d.with(is_replay_ready).then(|| view! { <div class="ready">"Draft ready — waiting"</div> })}
            <div class="tl-timer"><span style=timer></span></div>
        </div>
        {move || d.with(|d| d.format == "fearless" && !d.series_bans.is_empty()).then(|| view! {
            <div class="tl-series">
                <div class="lab">{move || format!("Series bans · {}", d.with(|d| d.series_bans.len()))}</div>
                <div class="pool">{move || d.with(|d| d.series_bans.iter().map(|h| view! { <span class="si"><Splash hero=h.clone() /></span> }).collect_view())}</div>
            </div>
        })}
        <div class="tl-stack">
            <div class="tl-picks b">{move || row(Side::Blue)}</div>
            <div class="tl-bans">{bans}</div>
            <div class="tl-picks r">{move || row(Side::Red)}</div>
        </div>
    }
}
