//! `/stream-pool`: "Played this stream" hero pool — compact bar list (portrait, games, W–L) of every hero
//! played in the operator's current session. Reuses `Review.heroes` from `GET /api/sessions/{id}`, the same
//! per-hero session rows the app's Session page renders; no backend change.

use crate::frame::{latest_review, provide_dim, Avatar, OverlayFrame};
use leptos::prelude::*;
use storm_web_core::{
    review::{Review, Row},
    route::Anchor,
};

pub fn mount_stream_pool() {
    mount_to_body(standalone);
}

/// At most this many hero rows (a long session still fits a 1920x1080 OBS canvas).
const MAX_ROWS: usize = 12;

fn standalone() -> impl IntoView {
    provide_dim();
    let review = latest_review(true).read_only();
    view! {
        <OverlayFrame anchor=Anchor::BottomRight pad=28.0>
            {stream_pool_content(review)}
        </OverlayFrame>
    }
}

pub fn stream_pool_content(review: ReadSignal<Option<Review>>) -> impl IntoView {
    view! {
        {move || {
            let rows: Vec<Row> = review
                .get()
                .map(|r| r.heroes.into_iter().take(MAX_ROWS).collect())
                .unwrap_or_default();
            if rows.is_empty() {
                return view! { <div></div> }.into_any();
            }
            pool_card(rows).into_any()
        }}
    }
}

fn pool_card(rows: Vec<Row>) -> impl IntoView {
    let rows = rows.into_iter().map(|h| {
        let losses = h.games - h.wins;
        view! {
            <div style="display:flex;align-items:center;gap:9px;padding:5px 0">
                <Avatar hero=Some(h.name.clone()) size=26.0 />
                <span style="flex:1;min-width:0;font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{h.name}</span>
                <span class="mono" style="font-size:11.5px;color:var(--text-2)">{format!("{}g", h.games)}</span>
                <span class="mono" style="font-size:12.5px">
                    <span style="color:var(--win)">{h.wins}</span>"–"<span style="color:var(--loss)">{losses}</span>
                </span>
            </div>
        }
    }).collect_view();
    view! {
        <div style="width:270px;background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:14px;padding:14px 16px;box-shadow:0 8px 30px rgba(0,0,0,.5)">
            <div class="kick" style="font-size:11px">"Played this stream"</div>
            <div style="margin-top:6px">{rows}</div>
        </div>
    }
}
