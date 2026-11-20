//! `/goal` source: live progress of the operator's daily goal (`docs/specs/2026-11-20-streamer-widgets.md`,
//! widget 7). Polls `GET /api/goal/progress` every 10 s (same safety-net cadence as the other session
//! sources); renders nothing — a blank transparent frame, not a placeholder message — when no goal is
//! set, since an OBS source with no content should not draw a visible box.

use crate::frame::OverlayFrame;
use leptos::{prelude::*, task::spawn_local};
use std::time::Duration;
use storm_web_core::{
    goal::{is_active, Progress},
    route::Anchor,
};

const POLL_MS: u64 = 10_000;

pub fn mount() {
    mount_to_body(Goal);
}

/// Re-reads `/api/goal/progress` every [`POLL_MS`]; a failed read keeps the last answer (same
/// behaviour as `now_playing::use_now_playing`).
fn use_progress() -> RwSignal<Progress> {
    let progress = RwSignal::new(Progress::default());
    let fetch = move || {
        spawn_local(async move {
            if let Ok(p) = storm_web_core::net::get_json::<Progress>("/api/goal/progress").await {
                progress.set(p);
            }
        })
    };
    fetch();
    set_interval(fetch, Duration::from_millis(POLL_MS));
    progress
}

#[component]
fn Goal() -> impl IntoView {
    let progress = use_progress();
    view! {
        <OverlayFrame anchor=Anchor::BottomLeft pad=36.0>
            {move || progress.with(|p| {
                if !is_active(p) {
                    // blank frame: no goal set
                    return view! { <div></div> }.into_any();
                }
                let line = storm_web_core::goal::display_line(p).unwrap_or_default();
                let pct = p.target.filter(|t| *t > 0).map(|t| (p.done as f64 / t as f64 * 100.0).min(100.0));
                let done_now = p.done >= p.target.unwrap_or(i64::MAX);
                view! {
                    <div style="display:flex;flex-direction:column;gap:6px;width:260px;background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:14px;padding:12px 16px;box-shadow:0 10px 30px rgba(0,0,0,.5)">
                        <span class="kick" style=format!("font-size:10px;color:{}", if done_now { "var(--win)" } else { "var(--accent)" })>"DAILY GOAL"</span>
                        <span class="mono" style="font-size:18px;font-weight:700">{line}</span>
                        {pct.map(|pct| view! {
                            <div style="height:5px;border-radius:3px;background:rgba(255,255,255,.12);overflow:hidden">
                                <div style=format!("width:{pct}%;height:100%;background:{};border-radius:3px;transition:width .4s ease", if done_now { "var(--win)" } else { "var(--accent)" })></div>
                            </div>
                        })}
                    </div>
                }
                .into_any()
            })}
        </OverlayFrame>
    }
}
