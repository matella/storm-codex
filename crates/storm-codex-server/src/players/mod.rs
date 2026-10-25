//! Recherche de joueurs et comptes liés (spec docs/specs/2026-10-25-recherche-joueurs-design.md).

pub mod names;
pub mod api;
pub mod search;

#[cfg(test)]
pub(crate) mod testutil;
