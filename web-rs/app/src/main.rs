//! The Storm Codex app of the Rust front-end (Leptos 0.8 CSR), served under `/v2/`. Port of `web/src/App.tsx`: one
//! router, the top-bar layout, and the pages (ported in the order of docs/specs/2026-11-09-rust-frontend-migration-design.md §8).

mod clip;
mod export;
mod layout;
mod matchui;
mod pages;
mod patchui;
mod replay;
mod safehtml;
mod ui;

use leptos::{mount::mount_to, prelude::*, wasm_bindgen::JsCast, web_sys};
use leptos_router::{
    components::{ParentRoute, Redirect, Route, Router, Routes},
    path,
};
use storm_web_core::route::LEGACY_REDIRECTS;

use layout::Layout;
use pages::{
    admin::Admin, cast::Cast, hero::Hero, hero_changes::HeroChanges, heroes::Heroes,
    leagues::Leagues, live::Live, maps::Maps, match_detail::MatchDetail, matches::Matches,
    patch::Patch, patches::Patches, player::Player, players::Players, scouting::Scouting,
    scouting_report::ScoutingReport, session::Session, synergies::Synergies, trends::Trends,
};

/// Redirect of an old page (`legacyRoutes.ts`), kept for bookmarks.
#[component]
fn Legacy(to: &'static str) -> impl IntoView {
    view! { <Redirect path=to /> }
}

#[component]
fn App() -> impl IntoView {
    view! {
        <Router base=storm_web_core::route::app_base()>
            <Routes fallback=|| view! { <div class="empty">"Not found."</div> }>
                <ParentRoute path=path!("") view=Layout>
                    <Route path=path!("") view=Session />
                    <Route path=path!("live") view=Live />
                    <Route path=path!("matches") view=Matches />
                    <Route path=path!("match/:id") view=MatchDetail />
                    <Route path=path!("players") view=Players />
                    <Route path=path!("player/:toon") view=Player />
                    <Route path=path!("heroes") view=Heroes />
                    <Route path=path!("hero/:name") view=Hero />
                    <Route path=path!("synergies") view=Synergies />
                    <Route path=path!("patches") view=Patches />
                    <Route path=path!("hero-changes") view=HeroChanges />
                    <Route path=path!("patch/:id") view=Patch />
                    <Route path=path!("maps") view=Maps />
                    <Route path=path!("trends") view=Trends />
                    <Route path=path!("leagues") view=Leagues />
                    <Route path=path!("cast") view=Cast />
                    <Route path=path!("scouting") view=Scouting />
                    <Route path=path!("scouting/:id") view=ScoutingReport />
                    <Route path=path!("admin") view=Admin />
                    <Route path=path!("draft") view=|| view! { <Legacy to=LEGACY_REDIRECTS[0].1 /> } />
                    <Route path=path!("draft/replay") view=|| view! { <Legacy to=LEGACY_REDIRECTS[1].1 /> } />
                </ParentRoute>
            </Routes>
        </Router>
    }
}

fn main() {
    // served at `/v2/...` (next to React) or at the root (after the switch): decided by the page that was loaded
    storm_web_core::route::init_base(&storm_web_core::net::pathname());
    let root = leptos::prelude::document()
        .get_element_by_id("root")
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(root) = root {
        mount_to(root, App).forget();
    }
}
