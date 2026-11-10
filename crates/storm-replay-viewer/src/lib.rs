mod extract;
mod maps;
mod model;
pub use model::*;

use storm_replay::Replay;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("replay: {0}")]
    Replay(#[from] storm_replay::Error),
    #[error("missing data: {0}")]
    Missing(&'static str),
}

/// Projects a replay into the viewer model (geometry only, keyed by replay playerId).
pub fn build_model(replay: &Replay) -> Result<ViewerModel, Error> {
    extract::build(replay)
}

/// Authoritative mapping `playerId (tracker, 1..=10) → toon_handle` — so that the server
/// attaches the Postgres player metadata. Derived from `SPlayerSetupEvent` (m_playerId/m_slotId)
/// cross-referenced with `details().players[*].working_set_slot_id → toon_handle`. Reused by Chunk 2.
pub fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, Error> {
    extract::player_toons(replay)
}
