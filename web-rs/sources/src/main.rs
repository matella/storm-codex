//! Session and music OBS sources of the Rust front-end, one small bundle (Leptos 0.8 CSR): `/v2/ticker`,
//! `/v2/widget`, `/v2/queue` and `/v2/now-playing`. The page is picked from the URL path; the server answers each
//! of those paths with this bundle's `index.html`. Port of the React sources under `web/src/pages`.

mod frame;
mod now_playing;
mod sessions;

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
        // not one of this bundle's sources (the cast and draft overlays are in the overlay bundle)
        _ => {}
    }
}
