//! Session and music OBS sources of the Rust front-end, one small bundle (Leptos 0.8 CSR): `/v2/ticker`,
//! `/v2/widget`, `/v2/queue`, `/v2/now-playing`, `/v2/recap`, `/v2/stream-pool`, `/v2/patch-banner`,
//! `/v2/record-flash` and `/v2/nemesis`. The page is picked from the URL path; the server answers each
//! of those paths with this bundle's `index.html`. Port of the React sources under `web/src/pages` (the
//! last five are new, spec `docs/specs/2026-11-20-streamer-widgets.md`).

mod frame;
mod nemesis;
mod now_playing;
mod patch_banner;
mod recap;
mod recordflash;
mod sessions;
mod stream_pool;

use storm_web_core::{
    net,
    route::{page_of, Page},
};

fn main() {
    // every source is transparent so OBS shows what is under it
    net::set_body_transparent();
    match page_of(&net::pathname()) {
        Some(Page::Ticker) => sessions::mount_ticker(),
        Some(Page::Widget) => sessions::mount_widget(),
        Some(Page::Queue) => sessions::mount_queue(),
        Some(Page::NowPlaying) => now_playing::mount(),
        Some(Page::Recap) => recap::mount_recap(),
        Some(Page::StreamPool) => stream_pool::mount_stream_pool(),
        Some(Page::PatchBanner) => patch_banner::mount_patch_banner(),
        Some(Page::RecordFlash) => recordflash::mount(),
        Some(Page::Nemesis) => nemesis::mount(),
        // not one of this bundle's sources (the cast and draft overlays are in the overlay bundle)
        _ => {}
    }
}
