//! The Storm Codex app of the Rust front-end (Leptos 0.8 CSR), served under `/v2/`. Port of `web/src/App.tsx`: one
//! router, the top-bar layout, and the pages (ported in the order of docs/specs/2026-11-09-rust-frontend-migration-design.md §8).

mod layout;
mod matchui;
mod pages;
mod ui;

use leptos::{mount::mount_to, prelude::*, wasm_bindgen::JsCast, web_sys};
use leptos_router::{
    components::{ParentRoute, Redirect, Route, Router, Routes},
    path,
};
use storm_web_core::route::LEGACY_REDIRECTS;

use layout::Layout;
use pages::{
    heroes::Heroes, leagues::Leagues, maps::Maps, matches::Matches, patches::Patches,
    player::Player, players::Players, session::Session, synergies::Synergies, Pending,
};

/// Redirect of an old page (`legacyRoutes.ts`), kept for bookmarks.
#[component]
fn Legacy(to: &'static str) -> impl IntoView {
    view! { <Redirect path=to /> }
}

#[component]
fn App() -> impl IntoView {
    view! {
        <Router base="/v2">
            <Routes fallback=|| view! { <div class="empty">"Not found."</div> }>
                <ParentRoute path=path!("") view=Layout>
                    <Route path=path!("") view=Session />
                    <Route path=path!("live") view=|| view! { <Pending name="Live" /> } />
                    <Route path=path!("matches") view=Matches />
                    <Route path=path!("match/:id") view=|| view! { <Pending name="Match" /> } />
                    <Route path=path!("players") view=Players />
                    <Route path=path!("player/:toon") view=Player />
                    <Route path=path!("heroes") view=Heroes />
                    <Route path=path!("hero/:name") view=|| view! { <Pending name="Hero" /> } />
                    <Route path=path!("synergies") view=Synergies />
                    <Route path=path!("patches") view=Patches />
                    <Route path=path!("hero-changes") view=|| view! { <Pending name="Hero changes" /> } />
                    <Route path=path!("patch/:id") view=|| view! { <Pending name="Patch" /> } />
                    <Route path=path!("maps") view=Maps />
                    <Route path=path!("trends") view=|| view! { <Pending name="Trends" /> } />
                    <Route path=path!("leagues") view=Leagues />
                    <Route path=path!("cast") view=|| view! { <Pending name="Cast" /> } />
                    <Route path=path!("scouting") view=|| view! { <Pending name="Scouting" /> } />
                    <Route path=path!("scouting/:id") view=|| view! { <Pending name="Scouting report" /> } />
                    <Route path=path!("admin") view=|| view! { <Pending name="Admin" /> } />
                    <Route path=path!("draft") view=|| view! { <Legacy to=LEGACY_REDIRECTS[0].1 /> } />
                    <Route path=path!("draft/replay") view=|| view! { <Legacy to=LEGACY_REDIRECTS[1].1 /> } />
                </ParentRoute>
            </Routes>
        </Router>
    }
}

fn main() {
    let root = leptos::prelude::document()
        .get_element_by_id("root")
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(root) = root {
        mount_to(root, App).forget();
    }
}
