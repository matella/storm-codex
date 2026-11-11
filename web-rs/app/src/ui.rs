//! Shared pieces of the app: reference data and settings loaded once, and the `ds` components used by several
//! pages (`components/ds/index.tsx`).

use leptos::{prelude::*, task::spawn_local};
use serde::de::DeserializeOwned;
use storm_web_core::{
    aggregates::{AggFilter, MODES},
    heroes::{Dim, DimHero},
    net,
    route::app_href,
};

/// Loads the hero reference data once and shares it (`useDimHeroes` in the React layout).
pub fn provide_dim() {
    let dim = RwSignal::new(Dim::default());
    provide_context(dim);
    spawn_local(async move {
        if let Ok(m) =
            net::get_json::<std::collections::HashMap<String, DimHero>>("/api/dim/heroes").await
        {
            dim.set(Dim::new(m));
        }
    });
}

pub fn use_dim() -> RwSignal<Dim> {
    expect_context::<RwSignal<Dim>>()
}

/// Operator names (`app_settings.operator_names`), shared by every page; empty until loaded.
#[derive(Clone, Copy)]
pub struct Operator(pub RwSignal<Vec<String>>);

pub fn provide_operator() {
    let names = RwSignal::new(Vec::<String>::new());
    provide_context(Operator(names));
    spawn_local(async move {
        if let Ok(s) = net::fetch_settings().await {
            names.set(s.operator_names);
        }
    });
}

pub fn use_operator() -> RwSignal<Vec<String>> {
    expect_context::<Operator>().0
}

/// Portrait tone: the ring and the slash of a ban (`Tone` in ds/index.tsx).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ban,
    Pick,
    Plain,
    Loss,
    Red,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Tone::Ban => "ban",
            Tone::Pick => "pick",
            Tone::Loss => "loss",
            Tone::Red => "red",
            Tone::Plain => "plain",
        }
    }
}

/// Initials shown when a hero has no portrait.
pub fn initials(h: &str) -> String {
    let words: Vec<String> = h
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ' ' {
                c
            } else {
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let s: String = if words.len() > 1 {
        words.iter().filter_map(|w| w.chars().next()).collect()
    } else {
        words.first().cloned().unwrap_or_default()
    };
    let s: String = s.chars().take(2).collect::<String>().to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

/// Hero portrait with the ring of its universe; initials when there is no icon or it fails to load.
#[component]
pub fn Portrait(
    #[prop(into)] hero: String,
    #[prop(default = 40)] size: u32,
    #[prop(default = Tone::Plain)] tone: Tone,
    #[prop(optional)] class: &'static str,
) -> impl IntoView {
    let dim = use_dim();
    let broken = RwSignal::new(false);
    let h = hero.clone();
    let icon = move || dim.with(|d| d.icon(&h).map(str::to_string));
    let h2 = hero.clone();
    let ring = move || dim.with(|d| d.ring(Some(&h2)));
    let style = move || format!("--s: {size}px; --ring: {}", ring());
    let ini = initials(&hero);
    let (title, alt) = (hero.clone(), hero);
    view! {
        <span class=format!("ds-pt ds-pt-{}{}{class}", tone.class(), if class.is_empty() { "" } else { " " }) style=style title=title>
            {move || match icon() {
                Some(src) if !broken.get() => view! {
                    <img src=src alt=alt.clone() loading="lazy" on:error=move |_| broken.set(true) />
                }.into_any(),
                _ => view! { <span class="ds-ini">{ini.clone()}</span> }.into_any(),
            }}
            {(tone == Tone::Ban).then(|| view! { <i class="ds-slash"></i> })}
        </span>
    }
}

/// Section heading with its two-digit number. `sub` is plain text; `children`, when given, replace it with rich
/// content (a pill that toggles a list).
#[component]
pub fn SecHead(
    num: u32,
    #[prop(into)] title: String,
    #[prop(optional, into)] sub: Option<String>,
    #[prop(optional)] children: Option<ChildrenFn>,
) -> impl IntoView {
    view! {
        <div class="ds-sec-hd">
            <span class="num">{format!("{num:02}")}</span>
            <h2>{title}</h2>
            {match (children, sub) {
                (Some(c), _) => Some(view! { <small>{c()}</small> }.into_any()),
                (None, Some(s)) => Some(view! { <small>{s}</small> }.into_any()),
                (None, None) => None,
            }}
        </div>
    }
}

/// Link to an app page. A plain `<a>` with the real `/v2/...` href: the router takes over the click (client-side
/// navigation) and the browser still handles middle-click, copy link and open in a new tab.
#[component]
pub fn Link(
    #[prop(into)] to: String,
    #[prop(optional, into)] class: Signal<String>,
    #[prop(optional, into)] style: String,
    children: Children,
) -> impl IntoView {
    view! { <a href=app_href(&to) class=class style=style>{children()}</a> }
}

/// `GET url`, read again whenever a signal read by `url` changes (the page's filter, a live-bus counter): what a
/// `useQuery` with a key does in React. `None` while loading.
pub fn fetch<T: DeserializeOwned + 'static>(
    url: impl Fn() -> String + 'static,
) -> LocalResource<Result<T, String>> {
    LocalResource::new(move || {
        let u = url();
        async move { net::get_json::<T>(&u).await }
    })
}

/// Style that gives a tile the in-game minimap of a map (`mapArt`).
pub fn map_art(map: &str) -> String {
    format!(
        "--img: url('{}')",
        storm_web_core::aggregates::minimap_image(map)
    )
}

/// Pill that is "on" when `on` is true.
fn pill_class(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

/// Filter bar shared by the aggregates (Heroes, Hero, Maps): mode, own games, account, dates (`AggFilterBar`).
/// `mine_label` adapts the wording ("My heroes" vs "My games").
#[component]
pub fn AggFilterBar(
    value: RwSignal<AggFilter>,
    #[prop(default = "Mine only")] mine_label: &'static str,
) -> impl IntoView {
    let accounts = use_operator();
    view! {
        <div class="ds-filterbar">
            {MODES
                .iter()
                .map(|&(label, m)| view! {
                    <span class=move || pill_class(value.with(|v| v.mode == m)) on:click=move |_| value.update(|v| v.mode = m)>{label}</span>
                })
                .collect_view()}
            <span class="ds-sep"></span>
            <span class=move || pill_class(value.with(|v| v.mine)) on:click=move |_| value.update(|v| v.mine = !v.mine)>{mine_label}</span>
            {move || {
                let list = accounts.get();
                (list.len() > 1).then(|| view! {
                    <select class="ds-input" on:change=move |e| {
                        let v = event_target_value(&e);
                        value.update(|f| f.account = (!v.is_empty()).then_some(v));
                    }>
                        <option value="" selected=move || value.with(|v| v.account.is_none())>"All my accounts"</option>
                        {list.into_iter().map(|a| {
                            let (a1, a2) = (a.clone(), a.clone());
                            view! { <option value=a1 selected=move || value.with(|v| v.account.as_deref() == Some(a2.as_str()))>{a}</option> }
                        }).collect_view()}
                    </select>
                })
            }}
            <label>"from "<input type="date" class="ds-input"
                prop:value=move || value.with(|v| v.from.clone().unwrap_or_default())
                on:input=move |e| { let v = event_target_value(&e); value.update(|f| f.from = (!v.is_empty()).then_some(v)); } /></label>
            <label>"to "<input type="date" class="ds-input"
                prop:value=move || value.with(|v| v.to.clone().unwrap_or_default())
                on:input=move |e| { let v = event_target_value(&e); value.update(|f| f.to = (!v.is_empty()).then_some(v)); } /></label>
            <Show when=move || value.with(|v| v.is_active())>
                <span class="ds-pill" on:click=move |_| value.set(AggFilter::default())>"✕ reset"</span>
            </Show>
        </div>
    }
}
