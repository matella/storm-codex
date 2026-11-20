//! Pieces shared by the OBS sources: the 1920x1080 frame, the hero avatar, the reference data and the
//! match-list hooks. Port of `OverlayFrame.tsx`, `Avatar.tsx` and the `useDimHeroes` / `useSettings` /
//! `useLiveUpdates` plumbing of the session sources.

use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    heroes::{Dim, DimHero},
    net,
    route::{frame_position, Anchor, Offsets},
    session::MatchSummary,
};

/// Loads the hero reference data once and shares it (`useDimHeroes`); the overlay is outside the app layout
/// that normally does it.
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

/// Operator names (`app_settings.operator_names`, read once), empty until loaded.
pub fn operator_names() -> RwSignal<Vec<String>> {
    let names = RwSignal::new(Vec::<String>::new());
    spawn_local(async move {
        if let Ok(s) = net::fetch_settings().await {
            names.set(s.operator_names);
        }
    });
    names
}

/// The `limit` most recent matches, re-read after every `match.parsed` event. A failed read keeps the last list.
pub fn recent_matches(limit: u32) -> RwSignal<Vec<MatchSummary>> {
    let list = RwSignal::new(Vec::<MatchSummary>::new());
    let fetch = move || {
        spawn_local(async move {
            if let Ok(m) = net::fetch_matches(limit).await {
                list.set(m);
            }
        })
    };
    fetch();
    net::start_live_events(move |ev| {
        if ev == "match.parsed" {
            fetch()
        }
    });
    list
}

/// The operator's latest session review (`GET /api/sessions?limit=1` then `GET /api/sessions/{id}`, same
/// endpoints as the app's Session page), re-read after every `match.parsed` event — same pattern as `/widget`
/// and `/queue`. `None` until loaded, when there is no session, or on a read failure (the last value is kept).
pub fn latest_review() -> RwSignal<Option<storm_web_core::review::Review>> {
    use storm_web_core::review::{Review, SessionList};
    let review = RwSignal::new(None::<Review>);
    let fetch = move || {
        spawn_local(async move {
            let Ok(list) = net::get_json::<SessionList>("/api/sessions?limit=1").await else {
                return;
            };
            let Some(id) = list.sessions.first().map(|s| s.id) else {
                review.set(None);
                return;
            };
            if let Ok(Some(r)) =
                net::get_json_or_not_found::<Review>(&format!("/api/sessions/{id}")).await
            {
                review.set(Some(r));
            }
        })
    };
    fetch();
    net::start_live_events(move |ev| {
        if ev == "match.parsed" {
            fetch()
        }
    });
    review
}

/// OBS canvas 1920x1080, transparent: every source shares the same frame, so alignment and uniform scaling are
/// predictable. The content sits in a corner with a constant `pad`; explicit offsets win over the corner.
#[component]
pub fn OverlayFrame(
    anchor: Anchor,
    #[prop(default = 28.0)] pad: f64,
    #[prop(optional)] top: Option<f64>,
    children: Children,
) -> impl IntoView {
    let pos = frame_position(
        anchor,
        pad,
        Offsets {
            top,
            ..Offsets::default()
        },
    );
    view! {
        <div style="position:fixed;top:0;left:0;width:1920px;height:1080px;overflow:hidden">
            <div style=format!("position:absolute;{pos}")>{children()}</div>
        </div>
    }
}

/// Hero portrait ringed in the universe colour; initials when the portrait is unknown or fails to load.
#[component]
pub fn Avatar(hero: Option<String>, #[prop(default = 24.0)] size: f64) -> impl IntoView {
    let dim = expect_context::<RwSignal<Dim>>();
    let broken = RwSignal::new(false);
    let h = hero.clone();
    move || {
        let ring = dim.with(|d| d.ring_var(h.as_deref()));
        let border = format!("{}px solid {ring}", (size / 16.0).max(1.5));
        let icon = h
            .as_deref()
            .and_then(|h| dim.with(|d| d.icon(h).map(str::to_string)));
        match icon.filter(|_| !broken.get()) {
            Some(src) => view! {
                <img src=src alt=h.clone().unwrap_or_default() title=h.clone() loading="lazy"
                    on:error=move |_| broken.set(true)
                    style=format!("width:{size}px;height:{size}px;border-radius:50%;object-fit:cover;border:{border};background:#181a22;flex-shrink:0") />
            }
            .into_any(),
            None => view! {
                <span class="av" title=h.clone()
                    style=format!("width:{size}px;height:{size}px;border:{border};color:{ring};background:#181a22;font-size:{}px", size * 0.34)>
                    {initials(h.as_deref())}
                </span>
            }
            .into_any(),
        }
    }
}

/// `initials`: two upper-case letters of the name, `··` when unknown.
fn initials(name: Option<&str>) -> String {
    match name {
        Some(n) if !n.is_empty() => n.chars().take(2).collect::<String>().to_uppercase(),
        _ => "··".into(),
    }
}
