//! Player profile (`/player/:toon`), Dioxus 0.7 web + dioxus-router. Same scope as the Leptos spike: API
//! loading, Stats / Identity tabs driven by `?tab=identity`, tiles, hero pool bars, recent games, loading
//! and "not found" states. Chat section left out; portraits use `/api/dim/heroes` with an initials fallback.

use dioxus::prelude::*;
use spike_common::{logic::map_slug, net, profile::*};
use std::collections::HashMap;

const THEME: Asset = asset!("/assets/theme.css");
const DS: Asset = asset!("/assets/ds.css");

fn main() {
    dioxus::launch(App);
}

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[route("/player/:toon?:tab")]
    Player { toon: String, tab: Option<String> },
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: THEME }
        document::Link { rel: "stylesheet", href: DS }
        Router::<Route> {}
    }
}

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    rsx! { div { class: "ds-page", div { class: "ds-empty", "Not found" } } }
}

#[component]
fn Portrait(hero: String, size: u32, #[props(into, default)] class: String, #[props(into, default = "plain".to_string())] tone: String) -> Element {
    // subscribed to the reference data: re-renders when it arrives, like `useDimHeroes` in React
    let dim = use_context::<Signal<Dim>>();
    let mut broken = use_signal(|| false); // image failed to load: fall back to initials
    let ring = dim.read().ring(Some(&hero));
    let icon = dim.read().icon(&hero).map(str::to_string).filter(|_| !broken());
    let cls = format!("ds-pt ds-pt-{tone} {class}").trim().to_string();
    rsx! {
        span { class: cls, style: "--s:{size}px;--ring:{ring}", title: "{hero}",
            if let Some(src) = icon {
                img { src, alt: "{hero}", loading: "lazy", onerror: move |_| broken.set(true) }
            } else {
                span { class: "ds-ini", "{initials(&hero)}" }
            }
        }
    }
}

#[component]
fn Player(toon: ReadSignal<String>, tab: ReadSignal<Option<String>>) -> Element {
    let mut dim = use_context_provider(|| Signal::new(Dim::default()));
    use_hook(move || {
        spawn(async move {
            if let Ok(m) = net::get_json::<HashMap<String, DimHero>>("/api/dim/heroes").await {
                dim.set(Dim::new(m));
            }
        });
    });
    let player = use_resource(move || async move { net::get_json::<PlayerSummary>(&format!("/api/players/{}", toon())).await });
    // loaded apart from the summary (slower), shared by the header and the Identity tab
    let identity = use_resource(move || async move { net::get_json::<PlayerIdentity>(&format!("/api/players/{}/identity", toon())).await.ok() });
    let mut all_heroes = use_signal(|| false);
    let tab_now = Tab::from_query(tab().as_deref());

    let d = match &*player.read() {
        None => return rsx! { div { class: "ds-page", div { class: "ds-empty", style: "margin-top: 24px", "loading…" } } },
        Some(Err(_)) => {
            return rsx! { div { class: "ds-page", div { class: "ds-empty", style: "margin-top: 24px", "Player not found. ", a { href: "/matches", "Back to matches" } } } }
        }
        Some(Ok(d)) => d.clone(),
    };
    let main = d.heroes.first().map(|h| h.hero.clone());
    let ring = dim.read().ring(main.as_deref());
    let ratio = kda_ratio(d.avg_takedowns, d.avg_deaths);
    let max_g = d.heroes.first().map(|h| h.games).unwrap_or(1).max(1);
    let others: Vec<String> = d.names.iter().filter(|n| Some(*n) != d.name.as_ref()).cloned().collect();
    let alias = identity.read().clone().flatten().and_then(|i| likely_alias(&i));
    let t = toon();
    let main_label = main.as_ref().map(|m| format!("main: {m}")).unwrap_or_default();
    let n_heroes = d.heroes.len();
    let shown = if all_heroes() { n_heroes } else { 18.min(n_heroes) };

    rsx! {
        div { class: "ds-page",
            header { class: "ds-hhead", style: "--ring:{ring}",
                if let Some(m) = main.clone() { Portrait { hero: m, size: 160, class: "xl" } }
                div { class: "ds-hhead-txt",
                    div { class: "ds-kicker", "Player · {d.toon}" }
                    h1 { class: "ds-title", style: "cursor: default", "{d.name.clone().unwrap_or(d.toon.clone())}" }
                    if !others.is_empty() {
                        div { class: "ds-subtitle", style: "cursor: default",
                            "also seen as {others.iter().take(3).cloned().collect::<Vec<_>>().join(\", \")}"
                            if others.len() > 3 {
                                " "
                                Link { to: Route::Player { toon: t.clone(), tab: Some("identity".into()) }, "+{others.len() - 3} more" }
                            }
                        }
                    }
                    if let Some((a_toon, a_name)) = alias {
                        div { class: "ds-subtitle", style: "cursor: default",
                            Link { to: Route::Player { toon: t.clone(), tab: Some("identity".into()) }, "Also plays as {a_name.unwrap_or(a_toon)} →" }
                        }
                    }
                    div { class: "ds-statline", style: "margin-top: 16px",
                        div { class: "ds-stat", span { class: "ds-label", "Win rate" }, strong { "{pct(d.wins, d.matches)}", span { "%" } }, small { "{d.wins}–{d.matches - d.wins} · {d.matches} games" } }
                        div { class: "ds-stat", span { class: "ds-label", "Avg K / D / T" }, strong { "{num(d.avg_kills)}", span { "/" }, "{num(d.avg_deaths)}", span { "/" }, "{num(d.avg_takedowns)}" }, small { "KDA ratio {ratio}" } }
                        div { class: "ds-stat", span { class: "ds-label", "Avg APM" }, strong { "{num(d.avg_apm)}" }, small { if d.apm_games > 0 { "{d.apm_games} games measured" } else { "re-process pending" } } }
                        div { class: "ds-stat", span { class: "ds-label", "Heroes" }, strong { "{n_heroes}" }, small { "{main_label}" } }
                    }
                }
            }

            nav { class: "ds-nav", "aria-label": "Player sections",
                Link { to: Route::Player { toon: t.clone(), tab: None }, class: if tab_now == Tab::Stats { "map" } else { "" }, "Stats" }
                Link { to: Route::Player { toon: t.clone(), tab: Some("identity".into()) }, class: if tab_now == Tab::Identity { "map" } else { "" }, "Identity" }
            }

            if tab_now == Tab::Identity {
                section { class: "ds-sec",
                    match &*identity.read() {
                        Some(Some(i)) => rsx! {
                            div { class: "ds-panel",
                                ul { for n in i.names.iter() { li { key: "{n.name}", "{n.name} · {n.games} games" } } }
                                p { "{i.confirmed.len()} confirmed link(s), {i.suggestions.len()} suggestion(s)" }
                            }
                        },
                        _ => rsx! { div { class: "ds-empty", "loading…" } },
                    }
                }
            } else {
                section { class: "ds-sec",
                    div { class: "ds-sec-hd",
                        span { class: "num", "01" }
                        h2 { "Hero pool" }
                        small {
                            if n_heroes > 18 {
                                span { class: "ds-pill", onclick: move |_| all_heroes.toggle(), if all_heroes() { "top 18" } else { "show all {n_heroes}" } }
                            } else {
                                "games · win rate · avg K/D/T"
                            }
                        }
                    }
                    div { class: "ds-panel",
                        div { class: "ds-pool",
                            for h in d.heroes.iter().take(shown) {
                                {
                                    let p = pct(h.wins, h.games);
                                    let wr = format!("wr {}", if p >= 55 { "hot" } else if p < 45 { "cold" } else { "" });
                                    rsx! {
                                        a { key: "{h.hero}", class: "ds-poolrow", href: "/hero/{h.hero}",
                                            Portrait { hero: h.hero.clone(), size: 34 }
                                            span { "{h.hero}" }
                                            i { class: "ds-bar", b { style: "width:{(h.games * 100 + max_g / 2) / max_g}%" } }
                                            span { class: wr, "{p}% · {h.games}g" }
                                            em { "{num(h.avg_kills)}/{num(h.avg_deaths)}/{num(h.avg_takedowns)}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section { class: "ds-sec",
                    div { class: "ds-sec-hd", span { class: "num", "02" }, h2 { "Recent games" } }
                    div { class: "ds-mlist",
                        for g in d.recent.iter() { RecentRow { key: "{g.match_id}", g: g.clone() } }
                    }
                }
            }
        }
    }
}

#[component]
fn RecentRow(g: PlayerGame) -> Element {
    let win = g.win == Some(true);
    let aw = award_label(g.award.as_deref());
    let a = (g.takedowns.unwrap_or(0.0) - g.kills.unwrap_or(0.0)).max(0.0);
    let (short, long, fam) = mode_tag(g.mode);
    let art = if has_map_art(g.map.as_deref()) { g.map.as_deref().map(|m| format!("--img:url('/images/minimaps/{}.jpg')", map_slug(m))) } else { None };
    let id = g.match_id;
    let nav = navigator();
    let row_class = format!("ds-mrow {}", if win { "w" } else { "l" });
    let tag_class = format!("ds-tag {}", if win { "win" } else { "loss" });
    let map_name = g.map.clone().unwrap_or("—".into());
    rsx! {
        div { class: row_class, style: art, role: "link", tabindex: "0",
            onclick: move |_| { nav.push(format!("/match/{id}")); },
            onkeydown: move |e| if e.key() == Key::Enter { nav.push(format!("/match/{id}")); },
            span { class: "when", "{net::fmt_time(g.played_at.as_deref())}" }
            span { span { class: "ds-mode {fam}", title: "{long}", "{short}" } }
            if let Some(h) = g.hero.clone() { Portrait { hero: h, size: 40, tone: if win { "pick" } else { "loss" } } } else { span {} }
            span { class: "what",
                span { class: "map", "{map_name}" }
                span { class: "hero", "{g.hero.clone().unwrap_or_default()} · ", span { class: "ds-kda", "{num(g.kills)}", i { "/" }, "{num(g.deaths)}", i { "/" }, "{num(Some(a))}" } }
            }
            span { style: "display: flex; gap: 6px",
                if let Some(apm) = g.apm { span { class: "ds-tag", title: "actions per minute", "{num(Some(apm))} APM" } }
                span { class: tag_class, if win { "WIN" } else { "LOSS" } }
                if aw.is_some_and(|a| a.1) { span { class: "ds-tag mvp", "👑 MVP" } }
            }
            span { class: "len", "›" }
        }
    }
}
