//! Hero reference data shared by the overlay pages (`useDimHeroes`): the overlay is outside the app layout that
//! normally loads it.

use leptos::{prelude::*, task::spawn_local};
use std::collections::HashMap;
use storm_web_core::{
    heroes::{Dim, DimHero},
    net,
};

/// Loads the reference data once and shares it as a context (`RwSignal<Dim>`); portraits fall back to initials
/// until it is there.
pub fn provide_dim() {
    let dim = RwSignal::new(Dim::default());
    provide_context(dim);
    spawn_local(async move {
        if let Ok(m) = net::get_json::<HashMap<String, DimHero>>("/api/dim/heroes").await {
            dim.set(Dim::new(m));
        }
    });
}
