//! Draft scene (Timeline view) shared by the cast overlay (`draft` scene) and the stand-alone `/draft/overlay`
//! page. Port of `web/src/cast/DraftScene.tsx` and `web/src/pages/DraftOverlay.tsx`.

use leptos::{prelude::*, task::spawn_local};
use std::time::Duration;
use storm_web_core::{heroes::Dim, model::*, net, scenes::*};

/// Hero portrait that fills its container (splash), initials when there is no image.
#[component]
pub fn Splash(hero: String) -> impl IntoView {
    let dim = expect_context::<RwSignal<Dim>>();
    move || match dim.with(|d| d.icon(&hero).map(str::to_string)) {
        Some(src) => view! { <div class="por" style=format!("background-image:url({src})")></div> }
            .into_any(),
        None => view! { <div class="por none">{draft_initials(&hero)}</div> }.into_any(),
    }
}

/// Draft scene: its own background (`.stage`), skin from `?skin=`.
#[component]
pub fn DraftScene() -> impl IntoView {
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
    // invariant: `DraftScene` mounts the timeline only once the draft is loaded
    #[allow(clippy::expect_used)]
    let d = Signal::derive(move || {
        draft
            .get()
            .expect("draft is loaded while the timeline is mounted")
    });
    // local 45 s countdown, restarted when the phase changes
    let phase = Memo::new(move |_| d.with(phase_start));
    let (secs, set_secs) = signal(PHASE_SECONDS);
    Effect::new(move |_| {
        phase.track();
        set_secs.set(PHASE_SECONDS);
    });
    let handle = set_interval_with_handle(
        move || {
            if phase.get_untracked().is_some() {
                set_secs.update(|s| *s = (*s - 1.0).max(0.0))
            }
        },
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
        let tr = if d.with(in_replay) {
            ";transition:width .25s linear"
        } else {
            ""
        };
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

/// Stand-alone draft overlay (`/draft/overlay`): the Timeline view, live state over the WebSocket, skin from
/// `?skin=`.
pub fn mount() {
    mount_to_body(DraftPage);
}

#[component]
fn DraftPage() -> impl IntoView {
    let draft = RwSignal::new(None::<DraftState>);
    let fetch = move || {
        spawn_local(async move {
            if let Ok(d) = net::fetch_draft().await {
                draft.set(Some(d));
            }
        })
    };
    fetch();
    // `draft.updated` carries the change; a (re)connection and the 10 s poll catch up on missed events
    net::start_live_events(move |ev| {
        if matches!(ev, "draft.updated" | "open" | "poll") {
            fetch()
        }
    });
    provide_context(draft);
    crate::dim::provide_dim();
    let (now, set_now) = signal(net::now_ms());
    set_interval(
        move || set_now.set(net::now_ms()),
        Duration::from_millis(250),
    );
    provide_context(now);
    let skin = skin(&net::search());
    provide_context(skin.to_string());
    view! { <DraftScene /> }
}
