//! `/recap`: condensed session recap card for OBS (record, streak, best hero this session, standout game).
//! Reuses `GET /api/sessions?limit=1` → `GET /api/sessions/{id}` (`Review`), the same endpoints and server-side
//! baseline scoring as the app's Session page; no backend change.

use crate::frame::{latest_review, provide_dim, Avatar, OverlayFrame};
use leptos::prelude::*;
use storm_web_core::{
    review::{streak_label, Review, ReviewGame, Row},
    route::Anchor,
    session::award_label,
};

pub fn mount_recap() {
    mount_to_body(standalone);
}

fn standalone() -> impl IntoView {
    provide_dim();
    let review = latest_review(false).read_only();
    view! {
        <OverlayFrame anchor=Anchor::BottomLeft pad=28.0>
            {recap_content(review)}
        </OverlayFrame>
    }
}

pub fn recap_content(review: ReadSignal<Option<Review>>) -> impl IntoView {
    view! {
        {move || match review.get() {
            None => view! { <div></div> }.into_any(),
            Some(r) if r.record.wins + r.record.losses == 0 => view! { <div></div> }.into_any(),
            Some(r) => recap_card(r).into_any(),
        }}
    }
}

fn recap_card(r: Review) -> impl IntoView {
    let best_hero: Option<Row> = r.heroes.first().cloned();
    let standout: Option<ReviewGame> = r
        .best
        .and_then(|id| r.games.iter().find(|g| g.match_id == id).cloned());
    let tone = if r.streaks.current > 0 {
        "var(--win)"
    } else if r.streaks.current < 0 {
        "var(--loss)"
    } else {
        "var(--text-2)"
    };
    let best_hero_row = best_hero.map(|h| {
        let losses = h.games - h.wins;
        view! {
            <div style="display:flex;align-items:center;gap:8px;margin-top:10px">
                <Avatar hero=Some(h.name.clone()) size=26.0 />
                <span style="flex:1;font-size:13px">{h.name}</span>
                <span class="mono" style="font-size:12.5px">
                    <span style="color:var(--win)">{h.wins}</span>"–"<span style="color:var(--loss)">{losses}</span>
                </span>
            </div>
        }
    });
    let standout_row = standout.map(|g| {
        let gtone = if g.win { "var(--win)" } else { "var(--loss)" };
        let k = g.kills.unwrap_or(0.0).round() as i64;
        let a = g.assists.unwrap_or(0.0).round() as i64;
        let d = g.deaths.unwrap_or(0.0).round() as i64;
        view! {
            <div style="display:flex;align-items:center;gap:8px;margin-top:10px;padding-top:10px;border-top:1px solid var(--hairline)">
                <Avatar hero=g.hero.clone() size=30.0 />
                <div style="flex:1;min-width:0">
                    <div style="font-size:12.5px;color:var(--text-2)">"Standout game"</div>
                    <div class="mono" style="font-size:12.5px">
                        <span style=format!("color:{gtone}")>{if g.win { "W" } else { "L" }}</span>
                        " · "{format!("{k}/{a}/{d}")}
                        {award_label(g.award.as_deref()).map(|aw| {
                            let text = if aw.mvp {
                                "👑 MVP".to_string()
                            } else {
                                format!("{} {}", aw.icon, aw.label)
                            };
                            view! { <span style="color:var(--text-2)" title=aw.label>{format!(" · {text}")}</span> }
                        })}
                    </div>
                </div>
            </div>
        }
    });
    view! {
        <div style="width:300px;background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:14px;padding:16px 18px;box-shadow:0 8px 30px rgba(0,0,0,.5)">
            <div class="kick" style="font-size:11px">"Session recap"</div>
            <div style="display:flex;align-items:baseline;gap:10px;margin-top:6px">
                <span class="mono" style="font-size:26px;font-weight:700">
                    <span style="color:var(--win)">{r.record.wins}"W"</span>" – "<span style="color:var(--loss)">{r.record.losses}"L"</span>
                </span>
                <span class="mono" style=format!("font-size:13px;color:{tone}")>{streak_label(r.streaks.current)}</span>
            </div>
            {best_hero_row}
            {standout_row}
        </div>
    }
}
