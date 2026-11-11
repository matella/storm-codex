//! Cast and draft OBS overlays of the Rust front-end, one small bundle (Leptos 0.8 CSR): `/v2/cast/overlay` and
//! `/v2/draft/overlay`. The page is picked from the URL path; the server answers each of those paths with this
//! bundle's `index.html`. Port of `web/src/pages/CastOverlay.tsx`, `DraftOverlay.tsx` and `web/src/cast/*Scene.tsx`.

mod cast;
mod dim;
mod draft;

use storm_web_core::{
    net,
    route::{page_of, Page},
};

fn main() {
    // every source is transparent so OBS shows what is under it
    net::set_body_transparent();
    match page_of(&net::pathname()) {
        Some(Page::CastOverlay) => cast::mount(),
        Some(Page::DraftOverlay) => draft::mount(),
        // not one of this bundle's sources (the session and music sources are in the sources bundle)
        _ => {}
    }
}
