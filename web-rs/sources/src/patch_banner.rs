//! `/patch-banner`: shows up to 7 real-world days when a new patch arrives and the operator hasn't been shown
//! it yet (localStorage "last seen" id, same pattern as the app's `standalone_welcome_decided` key — there is
//! no dismiss button, OBS sources are not clicked, so being shown once is the acknowledgment). Reuses the
//! existing `GET /api/patches` (no backend change); pure visibility rule in `storm_web_core::aggregates`.

use crate::frame::OverlayFrame;
use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    aggregates::{patch_banner_visible, PatchItem, PatchList},
    net,
    route::Anchor,
};

pub fn mount_patch_banner() {
    mount_to_body(standalone);
}

const LAST_SEEN_KEY: &str = "patch_banner_last_seen";

fn standalone() -> impl IntoView {
    let visible = watch_patch_banner();
    view! {
        <OverlayFrame anchor=Anchor::TopLeft pad=28.0>
            {patch_banner_content(move || visible.get().map(|p| p.patch_name))}
        </OverlayFrame>
    }
}

fn watch_patch_banner() -> Memo<Option<PatchItem>> {
    let latest = RwSignal::new(None::<PatchItem>);
    let fetch = move || {
        spawn_local(async move {
            if let Ok(list) = net::get_json::<PatchList>("/api/patches").await {
                latest.set(list.items.into_iter().next());
            }
        })
    };
    fetch();
    net::start_live_events(move |ev| {
        if ev == "patch.new" {
            fetch()
        }
    });
    let visible = Memo::new(move |_| {
        latest.with(|p| {
            let p = p.as_ref()?;
            let live_ms = p.live_date.as_deref().map(net::ts);
            let last_seen = net::storage_get(LAST_SEEN_KEY);
            let show = patch_banner_visible(
                &p.internal_id,
                live_ms,
                net::now_ms() as f64,
                last_seen.as_deref(),
            );
            show.then(|| p.clone())
        })
    });
    Effect::new(move |_| {
        if let Some(p) = visible.get() {
            // being shown is the acknowledgment: next time, a still-fresh same patch no longer flashes
            net::storage_set(LAST_SEEN_KEY, &p.internal_id);
        }
    });
    visible
}

pub fn patch_banner_content(
    visible: impl Fn() -> Option<String> + Send + 'static,
) -> impl IntoView {
    view! {
        {move || match visible() {
            None => view! { <div></div> }.into_any(),
            Some(p) => banner(p).into_any(),
        }}
    }
}

fn banner(name: String) -> impl IntoView {
    view! {
        <div style="display:inline-flex;align-items:center;gap:10px;background:rgba(14,16,22,.92);border:1px solid var(--u-nexus);border-radius:12px;padding:10px 18px;box-shadow:0 8px 30px rgba(0,0,0,.5)">
            <span style="font-size:18px">"🆕"</span>
            <div>
                <div style="font-size:13px;font-weight:700;color:var(--u-nexus)">"New patch"</div>
                <div style="font-size:12.5px;color:var(--text-2)">{name}</div>
            </div>
        </div>
    }
}
