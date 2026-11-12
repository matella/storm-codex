//! Top bar, live channel, toast and the two help modals. Port of `components/Layout.tsx`, `Onboarding.tsx` and
//! `WhatsNew.tsx`.

use leptos::prelude::*;
use leptos_router::{components::Outlet, hooks::use_location};
use storm_web_core::{
    live::{effects, reopen_effects, Effects},
    net,
    route::{app_href, app_path, tab_active, TABS},
    whatsnew::{entries_since, APP_VERSION},
};

use crate::ui::{provide_dim, provide_operator, provide_talents, Link};

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

#[component]
pub fn Layout() -> impl IntoView {
    provide_talents();
    provide_dim(); // reference data (universe rings)
    provide_operator(); // operator names (operator perspective everywhere)

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
            flash.set(Some(text));
            net::after_ms(6000, move || flash.set(None));
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

    // the dot disappears when the Patch Notes page is visited
    let loc = use_location();
    Effect::new(move |_| {
        if app_path(&loc.pathname.get()).starts_with("/patches") {
            new_patch.set(false);
        }
    });

    view! {
        <div class="ds-app">
            <header class="ds-top">
                <Link to="/" class="ds-brand">"STORM "<b>"CODEX"</b></Link>
                <nav class="ds-topnav">
                    {TABS
                        .iter()
                        .map(|&(to, label)| {
                            let on = move || tab_active(app_path(&loc.pathname.get()), to);
                            view! {
                                <Link to=to class=Signal::derive(move || if on() { "on".to_string() } else { String::new() })>
                                    {label}
                                    {(to == "/patches").then(|| view! {
                                        <Show when=move || new_patch.get()>
                                            <i class="dot" title="new patch"></i>
                                        </Show>
                                    })}
                                </Link>
                            }
                        })
                        .collect_view()}
                </nav>
                <span class="ds-help" title="Help / what's new" on:click=move |_| tour.set(true)>"?"</span>
                <span class=move || if online.get() { "ds-live" } else { "ds-live off" }>
                    <i></i>
                    {move || if online.get() { "online" } else { "offline" }}
                </span>
            </header>
            <main class="ds-shell">
                {move || flash.get().map(|t| view! { <div class="ds-flash">{t}</div> })}
                <Outlet />
            </main>
            <Onboarding force=tour />
            <WhatsNew />
        </div>
    }
}

const OVERLAY: &str = "position: fixed; inset: 0; background: rgba(0,0,0,.55); display: flex; align-items: center; justify-content: center; z-index: 1000";

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
/// (`force`).
#[component]
fn Onboarding(force: RwSignal<bool>) -> impl IntoView {
    let open = RwSignal::new(net::storage_get(ONBOARDING_KEY).is_none());
    let close = move || {
        net::storage_set(ONBOARDING_KEY, "1");
        open.set(false);
        force.set(false);
    };
    view! {
        <Show when=move || open.get() || force.get()>
            <div on:click=move |_| close() style=OVERLAY>
                <div on:click=|e| e.stop_propagation() style="background: var(--surface); border: 1px solid var(--hairline-strong); border-radius: 14px; padding: 22px 26px; max-width: 560px; max-height: 82vh; overflow-y: auto; box-shadow: 0 20px 60px rgba(0,0,0,.6)">
                    <div style="display: flex; align-items: baseline; gap: 10px; margin-bottom: 16px">
                        <h2 style="margin: 0">"Welcome to Storm Codex"</h2>
                        <span class="pill" style="margin-left: auto" on:click=move |_| close()>"skip"</span>
                    </div>
                    <Step n=1 title="Set your in-game name(s)">
                        "Open "<Link to="/admin" on:click=move |_| close() style="color: var(--accent)">"Admin → My identity"</Link>
                        " and enter your HotS name(s), comma-separated. This drives all your stats, the session panel and overlays."
                    </Step>
                    <Step n=2 title="Connect the uploader (your gaming PC)">
                        "In "<Link to="/admin" on:click=move |_| close() style="color: var(--accent)">"Admin → Upload tokens"</Link>
                        ", create a token, then run the uploader on your PC pointing here (server URL + token + your HotS replays folder). It backfills and watches new games automatically."
                    </Step>
                    <Step n=3 title="Add the OBS overlays (optional)">
                        "Cast control room: add "<span class="mono">{move || format!("{}{}", net::origin(), app_href("/cast/overlay"))}</span>
                        " as a Browser Source (1920×1080, transparent). Its URL is always shown at the top of "<span class="mono">"/cast"</span>
                        ". Other overlays, optional:"
                        <div class="mono" style="font-size: 11px; margin-top: 6px; line-height: 1.8">
                            {move || { let o = net::origin(); format!("{o}{} · {o}{} · {o}{}?me=<name> · {o}{}", app_href("/queue"), app_href("/ticker"), app_href("/widget"), app_href("/now-playing")) }}
                        </div>
                    </Step>
                    <div style="display: flex; justify-content: flex-end; margin-top: 8px">
                        <Link to="/admin" on:click=move |_| close() class="pill on">"Get started ›"</Link>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// "What's new": on opening, when the current version is newer than the last seen one, shows what changed. Only for
/// an existing user (onboarding done), so there is no double modal on the first run.
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
    view! {
        <Show when=move || open.get()>
            <div on:click=move |_| close() style=OVERLAY>
                <div on:click=|e| e.stop_propagation() style="background: var(--surface); border: 1px solid var(--hairline-strong); border-radius: 14px; padding: 20px 24px; max-width: 520px; max-height: 80vh; overflow-y: auto; box-shadow: 0 20px 60px rgba(0,0,0,.6)">
                    <div style="display: flex; align-items: baseline; gap: 10px; margin-bottom: 12px">
                        <h2 style="margin: 0">"What's new"</h2>
                        <span class="mono muted" style="font-size: 11px">{APP_VERSION}</span>
                        <span class="pill" style="margin-left: auto" on:click=move |_| close()>"got it"</span>
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
                </div>
            </div>
        </Show>
    }
}
