//! Shared pieces of the app: reference data and settings, and the `ds` components used by several
//! pages (`components/ds/index.tsx`).

use leptos::{prelude::*, task::spawn_local};
use serde::de::DeserializeOwned;
use storm_web_core::{
    aggregates::{AggFilter, MODES},
    herodetail::DimTalent,
    heroes::{Dim, DimHero},
    net,
    route::app_href,
};

/// Shares hero reference data and keeps the last good map while a refreshed copy loads.
pub fn provide_dim() {
    let dim = RwSignal::new(Dim::default());
    provide_context(dim);
    let data =
        fetch_reference::<std::collections::HashMap<String, DimHero>>(|| "/api/dim/heroes".into());
    Effect::new(move |_| {
        if let Some(Ok(m)) = data.get() {
            dim.set(Dim::new(m));
        }
    });
}

pub fn use_dim() -> RwSignal<Dim> {
    expect_context::<RwSignal<Dim>>()
}

/// Talent reference data (`GET /api/dim/talents`, about 240 kB): loaded lazily, then shared across pages.
#[derive(Clone, Copy)]
struct Talents {
    map: RwSignal<std::collections::HashMap<String, DimTalent>>,
    started: RwSignal<bool>,
}

pub fn provide_talents() {
    let t = Talents {
        map: RwSignal::new(Default::default()),
        started: RwSignal::new(false),
    };
    provide_context(t);
    let refresh = crate::reference::use_reference().refresh;
    let data = LocalResource::new(move || {
        let started = t.started.get();
        if started {
            refresh.track();
        }
        async move {
            if started {
                net::get_json("/api/dim/talents").await.ok()
            } else {
                None
            }
        }
    });
    Effect::new(move |_| {
        if let Some(Some(m)) = data.get() {
            t.map.set(m);
        }
    });
}

/// Talents by tree id; empty until loaded (names fall back to the decamelised id meanwhile).
pub fn use_talents() -> RwSignal<std::collections::HashMap<String, DimTalent>> {
    let t = expect_context::<Talents>();
    if !t.started.get_untracked() {
        t.started.set(true);
    }
    t.map
}

/// Runs `f` every `ms` milliseconds until the calling component is disposed (`setInterval` with its cleanup).
pub fn use_interval(ms: u32, f: impl Fn() + Clone + 'static) {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    fn tick(ms: u32, alive: Arc<AtomicBool>, f: impl Fn() + Clone + 'static) {
        net::after_ms(ms, move || {
            if alive.load(Ordering::Relaxed) {
                f();
                tick(ms, alive, f);
            }
        });
    }
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    on_cleanup(move || flag.store(false, Ordering::Relaxed));
    tick(ms, alive, f);
}

/// Manual settings and verified folder identities have independent lifecycles: polling the
/// latter never writes the former (including while an editor has a draft).
#[derive(Clone, Copy)]
struct Operator {
    manual: RwSignal<Vec<String>>,
    folders: RwSignal<Vec<storm_web_core::standalone::FolderIdentity>>,
    toons: Memo<Vec<String>>,
}

pub fn provide_operator(standalone: crate::standalone::Standalone) {
    let manual = RwSignal::new(Vec::<String>::new());
    let folders = RwSignal::new(Vec::<storm_web_core::standalone::FolderIdentity>::new());
    let toons = Memo::new(move |_| {
        folders.with(|folders| {
            let mut toons: Vec<_> = folders.iter().filter_map(|f| f.toon.clone()).collect();
            toons.sort_unstable();
            toons.dedup();
            toons
        })
    });
    provide_context(Operator {
        manual,
        folders,
        toons,
    });
    spawn_local(async move {
        if let Ok(s) = net::fetch_settings().await {
            manual.set(s.operator_names);
        }
    });
    let present = Memo::new(move |_| standalone.status.with(Option::is_some));
    let pending = RwSignal::new(false);
    let refresh = move || {
        if !present.get_untracked() || pending.get_untracked() {
            return;
        }
        pending.set(true);
        spawn_local(async move {
            if let Ok(next) = net::get_json::<Vec<storm_web_core::standalone::FolderIdentity>>(
                "/api/standalone/identity",
            )
            .await
            {
                if folders.with_untracked(|current| *current != next) {
                    folders.set(next);
                }
            }
            pending.set(false);
        });
    };
    Effect::new(move |_| {
        present.track();
        refresh();
    });
    use_interval(3_000, refresh);
}

pub fn use_operator() -> RwSignal<Vec<String>> {
    expect_context::<Operator>().manual
}

pub fn use_owner_toons() -> Memo<Vec<String>> {
    expect_context::<Operator>().toons
}

pub fn use_account_options() -> Memo<Vec<(String, String)>> {
    let op = expect_context::<Operator>();
    Memo::new(move |_| {
        op.manual.with(|manual| {
            op.folders
                .with(|folders| storm_web_core::standalone::account_options(manual, folders))
        })
    })
}

#[component]
pub fn FolderIdentities() -> impl IntoView {
    let folders = expect_context::<Operator>().folders;
    let pending = RwSignal::new(None::<String>);
    let errors = RwSignal::new(std::collections::HashMap::<String, String>::new());

    let set_confirmed = move |toon: String, confirmed: bool| {
        folders.update(|fs| {
            for f in fs
                .iter_mut()
                .filter(|f| f.toon.as_deref() == Some(toon.as_str()))
            {
                f.confirmed = confirmed;
            }
        });
    };

    let toggle = move |toon: String, currently_confirmed: bool| {
        if pending.get_untracked().is_some() {
            return;
        }
        pending.set(Some(toon.clone()));
        errors.update(|e| {
            e.remove(&toon);
        });
        set_confirmed(toon.clone(), !currently_confirmed);
        spawn_local(async move {
            let result = if currently_confirmed {
                net::write(
                    "DELETE",
                    &format!(
                        "/api/admin/identity/confirm/{}",
                        storm_web_core::route::uri_component(&toon)
                    ),
                    None,
                    None,
                )
                .await
            } else {
                let body = serde_json::json!({ "toon": toon }).to_string();
                net::write("POST", "/api/admin/identity/confirm", Some(body), None).await
            };
            match result {
                Ok((s, _)) if (200..300).contains(&s) => {}
                Ok((s, _)) => {
                    set_confirmed(toon.clone(), currently_confirmed);
                    errors.update(|e| {
                        e.insert(toon.clone(), format!("Could not save (HTTP {s})."));
                    });
                }
                Err(_) => {
                    set_confirmed(toon.clone(), currently_confirmed);
                    errors.update(|e| {
                        e.insert(
                            toon.clone(),
                            "Could not save: the local server did not answer.".into(),
                        );
                    });
                }
            }
            pending.set(None);
        });
    };

    view! {
        <div class="sa-sub">"Automatically detected by replay folder"</div>
        {move || folders.with(|folders| {
            if folders.is_empty() {
                return view! { <div class="sa-none">"No folder identities available yet."</div> }.into_any();
            }
            folders.iter().map(|f| {
                let toon = f.toon.clone();
                let confirmed = f.confirmed;
                let error = toon.as_ref().and_then(|t| errors.with(|e| e.get(t).cloned()));
                view! {
                    <div class="sa-folder">
                        <span class="mono">{f.folder.clone()}</span>
                        <span>{f.name.clone().unwrap_or_else(|| "Owner not detected".into())}</span>
                        {toon.map(|t| {
                            let busy_toon = t.clone();
                            let click_toon = t.clone();
                            view! {
                                <span class="sa-identity-toggle">
                                    {if confirmed {
                                        view! { <span class="sa-ok">"Added to My Identity ✓"</span> }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                    <button class="ds-btn"
                                        disabled=move || pending.with(|p| p.as_deref() == Some(busy_toon.as_str()))
                                        on:click=move |_| toggle(click_toon.clone(), confirmed)>
                                        {if confirmed { "Remove" } else { "Add to My Identity" }}
                                    </button>
                                </span>
                            }
                        })}
                        {error.map(|e| view! { <span class="sa-err">{e}</span> })}
                    </div>
                }
            }).collect_view().into_any()
        })}
    }
}

/// Portrait tone: the ring and the slash of a ban (`Tone` in ds/index.tsx).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ban,
    Pick,
    Them,
    Plain,
    Loss,
    Red,
    Blue,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Tone::Ban => "ban",
            Tone::Pick => "pick",
            Tone::Them => "them",
            Tone::Loss => "loss",
            Tone::Red => "red",
            Tone::Blue => "blue",
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

/// Revision-keyed portrait URLs retry an earlier missing image and bypass a cached old one.
pub(crate) fn portrait_src(path: &str, revision: Option<&str>) -> String {
    match revision.filter(|_| path.starts_with("/images/heroes/")) {
        Some(revision) => format!(
            "{path}?ref={}",
            storm_web_core::route::uri_component(revision)
        ),
        None => path.to_owned(),
    }
}

/// Hero portrait with the ring of its universe; initials when there is no icon or it fails to load.
/// A9/A20: `eager` loads the above-the-fold cover/first-row portraits immediately with `fetchpriority="high"`
/// (the rest stay `loading="lazy"`); `captioned` marks the portrait decorative (`alt=""`) when the hero name is
/// already printed next to it, avoiding redundant alt text for screen readers.
#[component]
pub fn Portrait(
    #[prop(into)] hero: String,
    #[prop(default = 40)] size: u32,
    #[prop(default = Tone::Plain)] tone: Tone,
    #[prop(optional)] class: &'static str,
    #[prop(default = false)] eager: bool,
    #[prop(default = false)] captioned: bool,
) -> impl IntoView {
    let dim = use_dim();
    let broken = RwSignal::new(None::<String>);
    let revision = crate::reference::use_reference().revision;
    let h = hero.clone();
    let icon = move || {
        dim.with(|d| {
            d.icon(&h)
                .map(|p| revision.with(|r| portrait_src(p, r.as_deref())))
        })
    };
    let h2 = hero.clone();
    let ring = move || dim.with(|d| d.ring(Some(&h2)));
    let style = move || format!("--s: {size}px; --ring: {}", ring());
    let ini = initials(&hero);
    let (title, alt) = (hero.clone(), if captioned { String::new() } else { hero });
    let loading = if eager { "eager" } else { "lazy" };
    let fetchpriority = if eager { "high" } else { "auto" };
    view! {
        <span class=format!("ds-pt ds-pt-{}{}{class}", tone.class(), if class.is_empty() { "" } else { " " }) style=style title=title>
            {move || match icon() {
                Some(src) if !broken.with(|b| b.as_ref() == Some(&src)) => view! {
                    <img src=src.clone() alt=alt.clone() loading=loading fetchpriority=fetchpriority on:error=move |_| broken.set(Some(src.clone())) />
                }.into_any(),
                _ => view! { <span class="ds-ini">{ini.clone()}</span> }.into_any(),
            }}
            {(tone == Tone::Ban).then(|| view! { <i class="ds-slash"></i> })}
        </span>
    }
}

/// Round hero portrait with the colour of its universe (`Avatar`); initials when there is no icon or it fails.
#[component]
pub fn Avatar(hero: Option<String>, #[prop(default = 24)] size: u32) -> impl IntoView {
    let dim = use_dim();
    let broken = RwSignal::new(None::<String>);
    let revision = crate::reference::use_reference().revision;
    let h = hero.clone();
    let ring = Signal::derive(move || dim.with(|d| d.ring_var(h.as_deref())));
    let border =
        Signal::derive(move || format!("{}px solid {}", (size as f64 / 16.0).max(1.5), ring.get()));
    let h = hero.clone();
    let icon = move || {
        dim.with(|d| {
            h.as_deref()
                .and_then(|x| d.icon(x))
                .map(|p| revision.with(|r| portrait_src(p, r.as_deref())))
        })
    };
    let title = hero.clone().unwrap_or_default();
    let ini = initials(hero.as_deref().unwrap_or(""));
    let alt = title.clone();
    view! {
        {move || match icon() {
            Some(src) if !broken.with(|b| b.as_ref() == Some(&src)) => view! {
                <img src=src.clone() alt=alt.clone() title=hero.clone() loading="lazy" on:error=move |_| broken.set(Some(src.clone()))
                    style=move || format!("width: {size}px; height: {size}px; border-radius: 50%; object-fit: cover; border: {}; background: #181a22; flex-shrink: 0", border.get()) />
            }.into_any(),
            _ => view! {
                <span class="av" title=title.clone()
                    style=move || format!("width: {size}px; height: {size}px; border: {}; color: {}; background: #181a22; font-size: {}px", border.get(), ring.get(), size as f64 * 0.34)>{ini.clone()}</span>
            }.into_any(),
        }}
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

/// Re-read an active view after a standalone activation without remounting the page: reference dimensions, and
/// reads whose hero attribution a reference activation can repair in place (match detail, 2D replay, aggregates).
/// Keyed on `ReferenceSync::refresh`, so the first status poll does not read the page a second time.
pub fn fetch_reference<T: DeserializeOwned + 'static>(
    url: impl Fn() -> String + 'static,
) -> LocalResource<Result<T, String>> {
    let refresh = crate::reference::use_reference().refresh;
    fetch(move || {
        refresh.track();
        url()
    })
}

/// Statistics read from the current replay corpus and operator settings, not just from a filter URL.
/// Reference activation can repair hero attribution in place, so they also follow the reference revision.
pub fn fetch_operator_stats<T: DeserializeOwned + 'static>(
    url: impl Fn() -> String + 'static,
) -> LocalResource<Result<T, String>> {
    fetch(operator_stats_url(url))
}

/// Track the revisions that invalidate operator statistics, including for custom reads.
pub(crate) fn operator_stats_url(
    url: impl Fn() -> String + 'static,
) -> impl Fn() -> String + 'static {
    let op = expect_context::<Operator>();
    let matches = expect_context::<crate::layout::LiveBus>().matches;
    let refresh = crate::reference::use_reference().refresh;
    move || {
        refresh.track();
        matches.track();
        op.manual.track();
        op.toons.track();
        url()
    }
}

/// Snapshot the archive URL and revision together, before any async read. A generation also changes
/// when a refresh keeps the same URL (import, reference activation or operator settings).
pub fn archive_generation(url: impl Fn() -> String + Send + Sync + 'static) -> Memo<(u64, String)> {
    let op = expect_context::<Operator>();
    let matches = expect_context::<crate::layout::LiveBus>().matches;
    let refresh = crate::reference::use_reference().refresh;
    Memo::new(move |previous: Option<&(u64, String)>| {
        refresh.track();
        matches.track();
        op.manual.track();
        op.toons.track();
        (previous.map_or(1, |(n, _)| n + 1), url())
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
pub(crate) fn pill_class(on: bool) -> &'static str {
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
    let accounts = use_account_options();
    view! {
        <div class="ds-filterbar">
            {MODES
                .iter()
                .map(|&(label, m)| view! {
                    <button type="button" class=move || pill_class(value.with(|v| v.mode == m)) aria-pressed=move || value.with(|v| v.mode == m).to_string() on:click=move |_| value.update(|v| v.mode = m)>{label}</button>
                })
                .collect_view()}
            <span class="ds-sep"></span>
            <button type="button" class=move || pill_class(value.with(|v| v.mine)) aria-pressed=move || value.with(|v| v.mine).to_string() on:click=move |_| value.update(|v| v.mine = !v.mine)>{mine_label}</button>
            {move || {
                let list = accounts.get();
                (list.len() > 1).then(|| view! {
                    <select class="ds-input" on:change=move |e| {
                        let v = event_target_value(&e);
                        value.update(|f| f.account = (!v.is_empty()).then_some(v));
                    }>
                        <option value="" selected=move || value.with(|v| v.account.is_none())>"All my accounts"</option>
                        {list.into_iter().map(|(a1, a)| {
                            let a2 = a1.clone();
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
                <button type="button" class="ds-pill" on:click=move |_| value.set(AggFilter::default())>"✕ reset"</button>
            </Show>
        </div>
    }
}
