//! The pages of the app, one module each.

pub mod admin;
pub mod hero;
pub mod hero_changes;
pub mod heroes;
pub mod leagues;
pub mod live;
pub mod maps;
pub mod match_detail;
pub mod matches;
pub mod patch;
pub mod patches;
pub mod player;
pub mod players;
pub mod scouting;
pub mod session;
pub mod synergies;
pub mod trends;

use leptos::prelude::*;

/// A page that is not ported yet (the list of what remains is in docs/STATUS.md).
#[component]
pub fn Pending(name: &'static str) -> impl IntoView {
    view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">{format!("{name}: not ported to the Rust front-end yet.")}</div></div> }
}
