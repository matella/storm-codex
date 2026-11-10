//! Player search and linked accounts (spec docs/specs/2026-10-25-recherche-joueurs-design.md).

pub mod api;
pub mod chat;
pub mod features;
pub mod identity;
pub mod names;
pub mod profiles;
pub mod search;
pub mod similarity;

#[cfg(test)]
pub(crate) mod testutil;
