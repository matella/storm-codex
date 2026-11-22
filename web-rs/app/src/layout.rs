//! Top bar, live channel, toast and the two help modals. Port of `components/Layout.tsx`, `Onboarding.tsx` and
//! `WhatsNew.tsx`.

use leptos::{prelude::*, wasm_bindgen, wasm_bindgen::JsCast, web_sys};
use leptos_router::{components::Outlet, hooks::use_location, hooks::use_navigate};
use storm_web_core::{
    live::{effects, reopen_effects, Effects},
    net,
    route::{app_base, app_href, app_path, is_app_route, page_title, tab_active, NavEntry, NAV},
    whatsnew::{entries_since, APP_VERSION},
};

use crate::{
    reference::provide_reference,
    standalone::{provide_standalone, Standalone, UpdateBanner},
    ui::{provide_dim, provide_operator, provide_talents, Link},
};
use storm_web_core::standalone::Phase;

const ONBOARDING_KEY: &str = "onboarding_done";
const WHATSNEW_KEY: &str = "whatsnew_seen";

/// The signals pages read to know that data changed on the server (what `invalidateQueries` does in React): a page
/// reads its signal inside its fetch and so fetches again on each change.
#[derive(Clone, Copy)]
pub struct LiveBus {
    pub matches: RwSignal<u32>,
    pub patches: RwSignal<u32>,
    pub draft: RwSignal<u32>,
    pub cast: RwSignal<u32>,
    pub live: RwSignal<u32>,
}

fn bump(s: RwSignal<u32>) {
    s.update(|n| *n += 1);
}

/// A21: whether the browser both supports `document.startViewTransition` and the user allows motion
/// (`prefers-reduced-motion: reduce` skips it, as the spec requires). Feature-detected at call time (the
/// pinned `web-sys` has no typed binding for this newly-available API) so unsupported browsers (and the
/// reduced-motion case) fall back to plain navigation with no behaviour change.
fn start_view_transition_fn() -> Option<js_sys::Function> {
    let window = web_sys::window()?;
    let reduced = window
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches());
    if reduced {
        return None;
    }
    let document = window.document()?;
    let v = js_sys::Reflect::get(
        &document,
        &wasm_bindgen::JsValue::from_str("startViewTransition"),
    )
    .ok()?;
    v.is_function().then(|| v.unchecked_into())
}

/// A21: same-document view transitions on internal navigations, as progressive enhancement. A capture-phase
/// click listener on the app root runs before the router's own delegated click handling; when the API is
/// unsupported or motion is reduced, it does nothing and the normal click (and the router's handling of it)
/// proceeds unchanged. Same-origin links outside the app's routes are always handed to the browser (see below).
fn view_transition_click(
    ev: web_sys::MouseEvent,
    navigate: impl Fn(&str, leptos_router::NavigateOptions),
) {
    if ev.default_prevented()
        || ev.button() != 0
        || ev.ctrl_key()
        || ev.meta_key()
        || ev.shift_key()
        || ev.alt_key()
    {
        return;
    }
    let Some(target) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return;
    };
    let Some(a) = target
        .closest("a[href]")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok())
    else {
        return;
    };
    if !a.target().is_empty() || a.has_attribute("download") {
        return;
    }
    // same-origin check: HtmlAnchorElement resolves `.origin()` against the current document
    let page_origin = web_sys::window().and_then(|w| w.location().origin().ok());
    if page_origin.as_deref() != Some(a.origin().as_str()) {
        return;
    }
    // A same-origin link outside the app's routes (`/api/...` exports, OBS sources) is a real document: load it as
    // the browser would. Merely returning is not enough, because the router's own click handling would still take
    // it over as an in-app navigation (blank page, no request) unless the link carries `download`/`target`.
    if !is_app_route(app_base(), &a.pathname()) {
        ev.prevent_default();
        if let Some(w) = web_sys::window() {
            let _ = w.location().assign(&a.href());
        }
        return;
    }
    let Some(start) = start_view_transition_fn() else {
        return;
    };
    let path = format!("{}{}", app_path(&a.pathname()), a.search());
    ev.prevent_default();
    let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        navigate(&path, Default::default());
    }) as Box<dyn FnMut()>);
    let this = web_sys::window()
        .and_then(|w| w.document())
        .map(wasm_bindgen::JsValue::from);
    let _ = start.call1(
        this.as_ref().unwrap_or(&wasm_bindgen::JsValue::UNDEFINED),
        cb.as_ref().unchecked_ref(),
    );
    cb.forget();
}

#[component]
pub fn Layout() -> impl IntoView {
    // desktop app: import status (404 on the box: nothing shown), first-load redirect to /welcome
    let standalone = provide_standalone();
    provide_reference(standalone);
    provide_talents();
    provide_dim(); // reference data (universe rings)
    provide_operator(standalone);
    let importing = move || {
        standalone
            .status
            .with(|s| s.as_ref().is_some_and(|s| s.phase != Phase::Ready))
    };

    let bus = LiveBus {
        matches: RwSignal::new(0),
        patches: RwSignal::new(0),
        draft: RwSignal::new(0),
        cast: RwSignal::new(0),
        live: RwSignal::new(0),
    };
    provide_context(bus);
    let online = RwSignal::new(false);
    let flash = RwSignal::new(None::<String>);
    let flash_generation = RwSignal::new(0u32);
    let new_patch = RwSignal::new(false);
    let tour = RwSignal::new(false);

    let apply = move |e: Effects| {
        for (on, sig) in [
            (e.matches_changed, bus.matches),
            (e.patches_changed, bus.patches),
            (e.draft_changed, bus.draft),
            (e.cast_changed, bus.cast),
            (e.live_changed, bus.live),
        ] {
            if on {
                bump(sig);
            }
        }
        if e.online {
            online.set(true);
        }
        if e.new_patch {
            new_patch.set(true);
        }
        if let Some(text) = e.flash {
            // Identical messages still have distinct lifetimes (e.g. replays on the same map).
            let generation = flash_generation.get_untracked().wrapping_add(1);
            flash_generation.set(generation);
            flash.set(Some(text));
            net::after_ms(6000, move || {
                if flash_generation.get_untracked() == generation {
                    flash.set(None);
                }
            });
        }
    };
    let opened = RwSignal::new(false); // has the channel connected once already
    net::start_live_full(move |ev| {
        if ev.kind == "open" {
            // after a reconnection, what was missed in between is read again
            apply(reopen_effects(opened.get_untracked()));
            opened.set(true);
        } else {
            apply(effects(&ev.kind, ev.map.as_deref(), ev.name.as_deref()));
        }
    });

    // A21: capture-phase click listener (runs before the router's own delegated click handling) for
    // progressive-enhancement view transitions on internal navigations. `use_navigate()` is called here,
    // inside this component's render, where the Router context is live -- not inside the handler itself,
    // which later fires as a bare DOM callback with no active reactive Owner and would panic looking up
    // that context.
    let root_ref = NodeRef::<leptos::html::Div>::new();
    let navigate = use_navigate();
    Effect::new(move |_| {
        let Some(el) = root_ref.get() else { return };
        let target: &web_sys::EventTarget = el.as_ref();
        let navigate = navigate.clone();
        let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::MouseEvent| {
            view_transition_click(ev, navigate.clone())
        })
            as Box<dyn FnMut(web_sys::MouseEvent)>);
        let _ = target.add_event_listener_with_callback_and_bool(
            "click",
            cb.as_ref().unchecked_ref(),
            true,
        );
        // The layout mounts once for the app's whole lifetime (it wraps every route), so this listener
        // is never removed; `forget` keeps the closure alive for as long as it can fire.
        cb.forget();
    });

    // the dot disappears when the Patch Notes page is visited
    let loc = use_location();
    Effect::new(move |_| {
        if app_path(&loc.pathname.get()).starts_with("/patches") {
            new_patch.set(false);
        }
    });

    // Grouped/dropdown nav: which category (if any) is currently open. Closed on outside
    // click, Escape, leaving the group with focus, or picking an item.
    let nav_open = RwSignal::new(None::<&'static str>);
    Effect::new(move |_| {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let click_cb =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::MouseEvent| {
                if nav_open.get_untracked().is_none() {
                    return;
                }
                let inside = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|t| t.closest(".ds-navgroup").ok().flatten())
                    .is_some();
                if !inside {
                    nav_open.set(None);
                }
            })
                as Box<dyn FnMut(web_sys::MouseEvent)>);
        let _ =
            document.add_event_listener_with_callback("click", click_cb.as_ref().unchecked_ref());
        click_cb.forget();

        let key_cb =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
                if ev.key() != "Escape" || nav_open.get_untracked().is_none() {
                    return;
                }
                // The key event originates at the focused element. Only return focus from
                // the open disclosure, not from another trigger or the rest of the page.
                let trigger = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|t| t.closest(".ds-navgroup").ok().flatten())
                    .and_then(|g| {
                        g.query_selector("button[aria-expanded=true]")
                            .ok()
                            .flatten()
                    })
                    .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok());
                if let Some(trigger) = trigger {
                    let _ = trigger.focus();
                }
                nav_open.set(None);
            })
                as Box<dyn FnMut(web_sys::KeyboardEvent)>);
        let _ =
            document.add_event_listener_with_callback("keydown", key_cb.as_ref().unchecked_ref());
        key_cb.forget();
    });

    // A6: per-route document.title (was always "Storm Codex"), and focus moved to the page heading so
    // screen readers and keyboard users get a cue that the route changed.
    Effect::new(move |_| {
        let title = page_title(app_path(&loc.pathname.get()));
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            document.set_title(&format!("{title} · Storm Codex"));
            if let Ok(Some(h)) = document.query_selector("#main h1, #main .ds-title") {
                if h.get_attribute("tabindex").is_none() {
                    let _ = h.set_attribute("tabindex", "-1");
                }
                if let Some(el) = h.dyn_ref::<web_sys::HtmlElement>() {
                    let _ = el.focus();
                }
            }
        }
    });

    view! {
        <div class="ds-app" node_ref=root_ref>
            <a class="ds-skip" href="#main">"Skip to content"</a>
            <header class="ds-top">
                <Link to="/" class="ds-brand">"STORM "<b>"CODEX"</b></Link>
                <nav class="ds-topnav" aria-label="Main">
                    {NAV
                        .iter()
                        .map(|entry| match entry {
                            NavEntry::Top(to, label) => {
                                let (to, label) = (*to, *label);
                                let on = move || tab_active(app_path(&loc.pathname.get()), to);
                                view! {
                                    <a href=app_href(to) class=move || if on() { "on".to_string() } else { String::new() }
                                        aria-current=move || on().then_some("page")>
                                        {label}
                                    </a>
                                }
                                .into_any()
                            }
                            NavEntry::Group(label, items) => {
                                view! { <NavGroup label=*label items=*items open=nav_open new_patch=new_patch /> }
                                    .into_any()
                            }
                        })
                        .collect_view()}
                    <a href=app_href("/overlay-builder") class=move || if tab_active(app_path(&loc.pathname.get()), "/overlay-builder") { "on" } else { "" }
                        aria-current=move || tab_active(app_path(&loc.pathname.get()), "/overlay-builder").then_some("page")>
                        "Overlay Builder"
                    </a>
                    <Show when=importing>
                        <a href=app_href("/welcome") class=move || if tab_active(app_path(&loc.pathname.get()), "/welcome") { "on".to_string() } else { String::new() }
                            aria-current=move || tab_active(app_path(&loc.pathname.get()), "/welcome").then_some("page")>
                            "Import"
                        </a>
                    </Show>
                </nav>
                <button type="button" class="ds-help" title="Help / what's new" aria-label="Help / what's new" on:click=move |_| tour.set(true)>"?"</button>
                <span class=move || if online.get() { "ds-live" } else { "ds-live off" }>
                    <i></i>
                    {move || if online.get() { "online" } else { "offline" }}
                </span>
            </header>
            <UpdateBanner />
            <main class="ds-shell" id="main" tabindex="-1">
                {move || flash.get().map(|t| view! { <div class="ds-flash">{t}</div> })}
                <Outlet />
            </main>
            <Onboarding force=tour standalone=standalone />
            <WhatsNew />
        </div>
    }
}

/// One category of the grouped/dropdown nav (e.g. "Stats", "Tools"): a disclosure button (ARIA
/// Authoring Practices "Disclosure" pattern -- a plain toggle button plus a normal list of links,
/// not a full `menu`/`menuitem` role that would need its own arrow-key model) that shows/hides a
/// list of tabs. The trigger highlights when the current route is one of its items, so users still
/// know where they are with the group collapsed. Closing on outside click/Escape and on picking an
/// item is handled by the caller (`Layout`) via the shared `open` signal.
#[component]
fn NavGroup(
    label: &'static str,
    items: &'static [(&'static str, &'static str)],
    open: RwSignal<Option<&'static str>>,
    new_patch: RwSignal<bool>,
) -> impl IntoView {
    let loc = use_location();
    let is_open = move || open.get() == Some(label);
    let active = move || {
        let pathname = loc.pathname.get();
        let p = app_path(&pathname);
        items.iter().any(|&(to, _)| tab_active(p, to))
    };
    let menu_id = format!("ds-navgroup-{}", label.to_lowercase());
    let group = NodeRef::<leptos::html::Div>::new();
    view! {
        <div class="ds-navgroup" node_ref=group on:focusout=move |ev: web_sys::FocusEvent| {
            if open.get_untracked() != Some(label) {
                return;
            }
            let next = ev.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
            if !group.get_untracked().is_some_and(|g| g.contains(next.as_ref())) {
                open.set(None);
            }
        }>
            <button
                type="button"
                class=move || if active() { "on" } else { "" }
                aria-expanded=move || is_open().to_string()
                aria-controls=menu_id.clone()
                on:click=move |_| open.update(|o| *o = if *o == Some(label) { None } else { Some(label) })
            >
                {label}
                <i class="ds-navgroup-caret" aria-hidden="true">"▾"</i>
            </button>
            <ul class="ds-navmenu" id=menu_id hidden=move || !is_open()>
                {items
                    .iter()
                    .map(|&(to, item_label)| {
                        let on = move || tab_active(app_path(&loc.pathname.get()), to);
                        view! {
                            <li>
                                <a href=app_href(to) class=move || if on() { "on".to_string() } else { String::new() }
                                    aria-current=move || on().then_some("page")
                                    on:click=move |_| open.set(None)>
                                    {item_label}
                                    {(to == "/patches").then(|| view! {
                                        <Show when=move || new_patch.get()>
                                            <i class="dot" title="new patch"></i>
                                        </Show>
                                    })}
                                </a>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        </div>
    }
}

#[component]
fn Step(n: u32, title: &'static str, children: Children) -> impl IntoView {
    view! {
        <div style="display: flex; gap: 12px; margin-bottom: 14px">
            <span class="mono" style="flex-shrink: 0; width: 24px; height: 24px; border-radius: 999px; background: var(--accent); color: #0b0d12; display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 12px">{n}</span>
            <div>
                <div style="font-weight: 600; margin-bottom: 2px">{title}</div>
                <div style="font-size: 13px; color: var(--text-2); line-height: 1.5">{children()}</div>
            </div>
        </div>
    }
}

/// First-launch assistant (identity, uploader, OBS sources): shown until finished once, re-opened by the `?` button
/// (`force`). A4: real `<dialog>` + `showModal()` (Esc, focus containment and return, `::backdrop`).
#[component]
fn Onboarding(force: RwSignal<bool>, standalone: Standalone) -> impl IntoView {
    let open = RwSignal::new(net::storage_get(ONBOARDING_KEY).is_none());
    let shown = move || {
        (open.get() && standalone.known.get() && standalone.status.with(Option::is_none))
            || force.get()
    };
    let close = move || {
        net::storage_set(ONBOARDING_KEY, "1");
        open.set(false);
        force.set(false);
    };
    let dialog_ref = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        let Some(d) = dialog_ref.get() else { return };
        if shown() {
            if !d.open() {
                let _ = d.show_modal();
            }
        } else if d.open() {
            d.close();
        }
    });
    view! {
        // not over the desktop app's welcome screen: its steps (tokens, uploader) are the box setup
        <dialog node_ref=dialog_ref class="sa-dialog" on:close=move |_| close() on:click=move |e| {
            let t: web_sys::Element = event_target(&e);
            if dialog_ref.get().is_some_and(|d| t == **d) { close(); }
        } style="background: var(--surface); border: 1px solid var(--hairline-strong); border-radius: 14px; padding: 22px 26px; max-width: 560px; max-height: 82vh; overflow-y: auto; color: inherit; margin: auto; box-shadow: 0 20px 60px rgba(0,0,0,.6)">
                    <div style="display: flex; align-items: baseline; gap: 10px; margin-bottom: 16px">
                        <h2 style="margin: 0">"Welcome to Storm Codex"</h2>
                        <button type="button" class="pill" style="margin-left: auto" on:click=move |_| close()>"skip"</button>
                    </div>
                    <Step n=1 title="Set your in-game name(s)">
                        "Open "<Link to="/admin" on:click=move |_| close() style="color: var(--accent); text-decoration: underline">"Admin → My identity"</Link>
                        " and enter your HotS name(s), comma-separated. This drives all your stats, the session panel and overlays."
                    </Step>
                    <Step n=2 title="Connect the uploader (your gaming PC)">
                        "In "<Link to="/admin" on:click=move |_| close() style="color: var(--accent); text-decoration: underline">"Admin → Upload tokens"</Link>
                        ", create a token, then run the uploader on your PC pointing here (server URL + token + your HotS replays folder). It backfills and watches new games automatically."
                    </Step>
                    <Step n=3 title="Add the OBS overlays (optional)">
                        "Cast control room: add "<span class="mono">{move || format!("{}{}", net::origin(), app_href("/cast/overlay"))}</span>
                        " as a Browser Source (1920×1080, transparent). Its URL is always shown at the top of "<span class="mono">"/cast"</span>
                        ". Other overlays, optional:"
                        <div class="mono" style="font-size: 11px; margin-top: 6px; line-height: 1.8">
                            {move || { let o = net::origin(); format!("{o}{} · {o}{} · {o}{}?me=<name>", app_href("/queue"), app_href("/ticker"), app_href("/widget")) }}
                        </div>
                    </Step>
                    <div style="display: flex; justify-content: flex-end; margin-top: 8px">
                        <Link to="/admin" on:click=move |_| close() class="pill on">"Get started ›"</Link>
                    </div>
        </dialog>
    }
}

/// "What's new": on opening, when the current version is newer than the last seen one, shows what changed. Only for
/// an existing user (onboarding done), so there is no double modal on the first run. A4: real `<dialog>`.
#[component]
fn WhatsNew() -> impl IntoView {
    let seen = net::storage_get(WHATSNEW_KEY).unwrap_or_default();
    let entries = entries_since(&seen, false);
    let open = RwSignal::new(
        net::storage_get(ONBOARDING_KEY).is_some()
            && seen.as_str() < APP_VERSION
            && !entries.is_empty(),
    );
    let close = move || {
        net::storage_set(WHATSNEW_KEY, APP_VERSION);
        open.set(false);
    };
    let dialog_ref = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        let Some(d) = dialog_ref.get() else { return };
        if open.get() {
            if !d.open() {
                let _ = d.show_modal();
            }
        } else if d.open() {
            d.close();
        }
    });
    view! {
        <dialog node_ref=dialog_ref class="sa-dialog" on:close=move |_| close() on:click=move |e| {
            let t: web_sys::Element = event_target(&e);
            if dialog_ref.get().is_some_and(|d| t == **d) { close(); }
        } style="background: var(--surface); border: 1px solid var(--hairline-strong); border-radius: 14px; padding: 20px 24px; max-width: 520px; max-height: 80vh; overflow-y: auto; color: inherit; margin: auto; box-shadow: 0 20px 60px rgba(0,0,0,.6)">
                    <div style="display: flex; align-items: baseline; gap: 10px; margin-bottom: 12px">
                        <h2 style="margin: 0">"What's new"</h2>
                        <span class="mono muted" style="font-size: 11px">{APP_VERSION}</span>
                        <button type="button" class="pill" style="margin-left: auto" on:click=move |_| close()>"got it"</button>
                    </div>
                    {entries
                        .iter()
                        .map(|c| view! {
                            <div style="margin-bottom: 14px">
                                <div class="kick" style="font-size: 12px; margin-bottom: 6px">{c.title}</div>
                                <ul style="margin: 0; padding-left: 18px; line-height: 1.6; font-size: 13.5px">
                                    {c.items.iter().map(|it| view! { <li>{*it}</li> }).collect_view()}
                                </ul>
                            </div>
                        })
                        .collect_view()}
        </dialog>
    }
}
