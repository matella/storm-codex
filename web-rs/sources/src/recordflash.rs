//! `/record-flash` (spec `docs/specs/2026-11-20-streamer-widgets.md` item 3): flashes "New record:
//! <stat> (<value>)" for ~8 s when the newest game set a personal best. Reuses `frame::recent_matches`
//! (the hook `/ticker`, `/widget` and `/queue` already use, refetched on the server's `match.parsed`
//! event — the spec's "matches.updated") for the newest match id, and `GET /api/records?mine=true&limit=1`
//! to check whether that match is any stat's top row.

use crate::frame::{recent_matches, OverlayFrame};
use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    insights::{new_personal_record, short_num, Records},
    net,
    route::Anchor,
};

/// How long the flash stays up.
const FLASH_MS: u32 = 8_000;

pub fn mount() {
    mount_to_body(RecordFlash);
}

#[component]
fn RecordFlash() -> impl IntoView {
    let matches = recent_matches(1);
    let flash = RwSignal::new(None::<(String, f64)>);
    // the newest match id already checked, so a re-fetch of the same latest game (e.g. the 10 s
    // safety-net poll) does not re-flash or re-query `/api/records`
    let checked = RwSignal::new(None::<i64>);

    Effect::new(move |_| {
        let Some(id) = matches.with(|ms| ms.first().map(|m| m.id)) else {
            return;
        };
        if checked.get_untracked() == Some(id) {
            return;
        }
        checked.set(Some(id));
        spawn_local(async move {
            if let Ok(r) = net::get_json::<Records>("/api/records?mine=true&limit=1").await {
                if let Some(found) = new_personal_record(&r.stats, id) {
                    flash.set(Some(found));
                    net::after_ms(FLASH_MS, move || flash.set(None));
                }
            }
        });
    });

    view! {
        <OverlayFrame anchor=Anchor::TopRight top=250.0>
            {move || flash.get().map(|(label, value)| view! {
                <div class="rf-pop" style="display:inline-flex;align-items:center;gap:10px;background:linear-gradient(90deg,#241a04,#16120a);border:1px solid #f5c542;border-radius:12px;padding:10px 18px;box-shadow:0 0 24px rgba(245,197,66,.45),0 8px 30px rgba(0,0,0,.5);font-size:16px;font-weight:600">
                    <span style="font-size:18px">"🏆"</span>
                    <span style="color:#f5c542;letter-spacing:.03em">"NEW RECORD"</span>
                    <span class="mono" style="color:var(--text)">{format!("{} · {}", label, short_num(value))}</span>
                </div>
            })}
        </OverlayFrame>
    }
}
