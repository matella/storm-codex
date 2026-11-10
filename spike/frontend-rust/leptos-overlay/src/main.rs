//! Cast overlay (`/cast/overlay`), Leptos 0.8 CSR. Scenes `ingame` and `series` (map strip),
//! transparent body, 300 ms cross-fade that keeps the outgoing scene mounted, slideshow driven by the
//! server anchor, `/ws` live updates with reconnection. Port of `web/src/pages/CastOverlay.tsx`.

use leptos::{prelude::*, task::spawn_local};
use spike_common::{logic::*, model::*, net};
use std::time::Duration;

const FADE_MS: u64 = 300;

fn main() {
    net::set_body_transparent();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (series, set_series) = signal(None::<CastSeries>);
    let refresh = move || {
        spawn_local(async move {
            if let Ok(s) = net::fetch_cast().await {
                set_series.set(Some(s));
            }
        })
    };
    refresh();
    net::start_live(refresh);

    // clock for the slideshow, 250 ms like the React hook
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

    let skin = net::skin_from_search(&net::search());
    let ids = move || {
        layers.with(|l| l.leaving.iter().chain(l.cur.iter()).map(|x| x.id).collect::<Vec<_>>())
    };
    view! {
        <div class="cast-overlay" data-skin=skin>
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
    view! {
        <div class="scene" class:out=out aria-hidden=move || out().then_some("true")>
            {match key {
                Some(Scene::Ingame) => view! { <InGame s /> }.into_any(),
                Some(Scene::Series) => view! {
                    <div class="scene-backdrop" data-backdrop><div class="stars"></div></div>
                    <SeriesScene s />
                }.into_any(),
                _ => ().into_any(),
            }}
        </div>
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

#[component]
fn SeriesScene(s: Signal<CastSeries>) -> impl IntoView {
    let now = expect_context::<ReadSignal<u64>>();
    let idx = Memo::new(move |_| s.with(|s| slide_index(s, slide_tick(s, now.get()))));
    // the frame is rebuilt (and its fade replayed) only when the slide index changes
    move || {
        let i = idx.get();
        let body = if i > 0 {
            view! { <div class="sr-ttl">"Post-game slide (not ported in the spike)"</div> }.into_any()
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
