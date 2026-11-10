//! Cast overlay (`/cast/overlay`), Dioxus 0.7 web (CSR). Same scope as the Leptos spike: scenes `ingame`
//! and `series` (map strip), transparent body, 300 ms cross-fade keeping the outgoing scene mounted,
//! slideshow driven by the server anchor, `/ws` live updates with reconnection.

use dioxus::prelude::*;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use spike_common::{logic::*, model::*, net};

const FADE_MS: u32 = 300;
const CSS: Asset = asset!("/assets/cast-overlay.css");

fn main() {
    net::set_body_transparent();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let series = use_signal(|| None::<CastSeries>);
    let mut layers = use_signal(Layers::default);
    let mut now = use_signal(net::now_ms);

    use_hook(move || {
        let refresh = move || {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(s) = net::fetch_cast().await {
                    let mut series = series;
                    series.set(Some(s));
                }
            })
        };
        refresh();
        net::start_live(refresh);
        // clock for the slideshow, 250 ms like the React hook
        Interval::new(250, move || now.set(net::now_ms())).forget();
    });

    use_context_provider(|| now);

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

    let skin = net::skin_from_search(&net::search());
    let ids: Vec<u64> = {
        let l = layers.read();
        l.leaving.iter().chain(l.cur.iter()).map(|x| x.id).collect()
    };
    rsx! {
        document::Link { rel: "stylesheet", href: CSS }
        div { class: "cast-overlay", "data-skin": skin,
            for id in ids {
                Layer { key: "{id}", id, layers }
            }
        }
    }
}

/// One DOM layer per id: the node stays when the layer goes from current to leaving, only `out` flips.
#[component]
fn Layer(id: u64, layers: Signal<Layers>) -> Element {
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
                _ => rsx! {},
            }
        }
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

#[component]
fn SeriesScene(s: CastSeries) -> Element {
    let now = use_context::<Signal<u64>>();
    let idx = slide_index(&s, slide_tick(&s, now()));
    // keyed by the slide index: the frame is rebuilt (and its fade replayed) only when the slide changes
    rsx! {
        div { class: "sr-frame", key: "{idx}",
            if idx > 0 {
                div { class: "sr-ttl", "Post-game slide (not ported in the spike)" }
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
