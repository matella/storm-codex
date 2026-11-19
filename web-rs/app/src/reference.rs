//! Standalone reference status and controls. Kept at layout scope so navigation never interrupts a check.

use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    net::{self, REFERENCE_PATH},
    standalone::{error_message, ReferencePhase, ReferenceStatus, StatusRequests},
};

use crate::standalone::Standalone;

#[derive(Clone, Copy)]
pub struct ReferenceSync {
    pub status: RwSignal<Option<ReferenceStatus>>,
    pub revision: Memo<Option<String>>,
    requests: RwSignal<StatusRequests>,
    message: RwSignal<Option<Result<&'static str, String>>>,
}

pub fn use_reference() -> ReferenceSync {
    expect_context::<ReferenceSync>()
}

pub fn provide_reference(standalone: Standalone) {
    let status = RwSignal::new(None::<ReferenceStatus>);
    let sync = ReferenceSync {
        status,
        revision: Memo::new(move |_| {
            status.with(|s| {
                s.as_ref()
                    .and_then(ReferenceStatus::active_revision)
                    .map(str::to_owned)
            })
        }),
        requests: RwSignal::new(StatusRequests::default()),
        message: RwSignal::new(None),
    };
    provide_context(sync);

    let present = Memo::new(move |_| standalone.status.with(Option::is_some));
    Effect::new(move |_| {
        if present.get() {
            poll_reference(standalone, sync);
        }
    });
}

fn poll_reference(standalone: Standalone, sync: ReferenceSync) {
    // Follow the layout signals, not a route owner's cleanup (which can run on navigation).
    if standalone.status.try_with_untracked(Option::is_some) != Some(true) {
        return;
    }
    let next_poll = move || net::after_ms(3_000, move || poll_reference(standalone, sync));
    let Some(request) = sync
        .requests
        .try_update(StatusRequests::begin_poll)
        .flatten()
    else {
        next_poll();
        return;
    };
    spawn_local(async move {
        if let Ok(next) = net::fetch_reference_status().await {
            if sync.requests.try_update(|r| r.accept_poll(request)) == Some(true) {
                let unsupported = next.is_none();
                if sync.status.try_with_untracked(|old| *old != next) == Some(true) {
                    sync.status.try_set(next);
                }
                if unsupported {
                    return;
                }
            }
        }
        next_poll();
    });
}

#[component]
pub fn ReferenceControls() -> impl IntoView {
    let sync = use_reference();
    let available = move || sync.status.with(Option::is_some);
    let enabled = move || sync.status.with(|s| s.as_ref().is_some_and(|s| s.enabled));
    let busy = Memo::new(move |_| sync.requests.with(StatusRequests::saving));
    let action = move |enable: Option<bool>| {
        if sync.requests.try_update(StatusRequests::begin_save) != Some(true) {
            return;
        }
        sync.message.set(None);
        spawn_local(async move {
            let (method, path, body) = match enable {
                Some(enabled) => (
                    "PUT",
                    REFERENCE_PATH,
                    Some(serde_json::json!({ "enabled": enabled }).to_string()),
                ),
                None => ("POST", "/api/standalone/reference/refresh", None),
            };
            let outcome = match net::write(method, path, body, Some("")).await {
                Ok((200, text)) if enable.is_some() => match serde_json::from_str(&text) {
                    Ok(next) => {
                        sync.status.set(Some(next));
                        None
                    }
                    Err(_) => Some(Err("Invalid reference sync status.".into())),
                },
                Ok((202, _)) if enable.is_none() => Some(Ok("Reference check queued.")),
                Ok((409, _)) if enable.is_none() => Some(Err(
                    "Reference downloads are disabled. Enable them before retrying.".into(),
                )),
                Ok((code, text)) => Some(Err(error_message(code, &text))),
                Err(_) => Some(Err("Could not reach the local server.".into())),
            };
            sync.message.set(outcome);
            sync.requests.update(StatusRequests::finish_save);
        });
    };
    view! {
        <Show when=available>
            <div class="ds-label" style="margin-top: 18px">"Game reference data"</div>
            <div role="status">
                {move || sync.status.with(|s| s.as_ref().map(|s| match s.phase {
                    ReferencePhase::Waiting if !s.enabled => "Automatic downloads are off.",
                    ReferencePhase::Waiting => "Waiting for a reference download.",
                    ReferencePhase::Syncing => "Checking for reference updates…",
                    ReferencePhase::Ready => "Reference data is ready.",
                    ReferencePhase::Error => "Reference sync failed. Check your connection and retry.",
                }))}
                {move || sync.status.with(|s| s.as_ref().map(|s| match &s.version {
                    Some(v) if s.artwork => format!(" Cached version: {v} (available offline)."),
                    Some(v) if s.enabled => format!(" Cached version: {v}, but its artwork is missing on this PC."),
                    Some(v) => format!(
                        " Cached version: {v}, but its artwork is missing on this PC. Enable automatic downloads to restore it."
                    ),
                    None => " No cached version yet.".into(),
                }))}
            </div>
            <div class="sa-row" style="margin-top: 8px">
                <label>
                    <input type="checkbox" prop:checked=enabled disabled=move || busy.get()
                        on:click=move |e| {
                            // The server's committed value drives the checkbox, even if a write fails.
                            e.prevent_default();
                            action(Some(!sync.status.with_untracked(|s| s.as_ref().is_some_and(|s| s.enabled))));
                        } />
                    " Automatically download game reference data"
                </label>
                <button class="ds-btn" disabled=move || busy.get() || !enabled()
                    on:click=move |_| action(None)>"Retry reference sync"</button>
                <span role="status">{move || sync.message.get().map(|m| match m {
                    Ok(text) => view! { <span class="sa-ok">{text}</span> }.into_any(),
                    Err(text) => view! { <span class="sa-err">{text}</span> }.into_any(),
                })}</span>
            </div>
        </Show>
    }
}
