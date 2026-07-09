mod extract;
mod maps;
mod model;
pub use model::*;

use storm_replay::Replay;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("replay: {0}")]
    Replay(#[from] storm_replay::Error),
    #[error("donnée manquante: {0}")]
    Missing(&'static str),
}

/// Projette un replay en modèle visionneuse (géométrie seule, clé playerId replay).
pub fn build_model(replay: &Replay) -> Result<ViewerModel, Error> {
    extract::build(replay)
}

/// Mapping autoritaire `playerId (tracker, 1..=10) → toon_handle` — pour que le serveur
/// attache les métadonnées joueurs de Postgres. Dérivé de `SPlayerSetupEvent` (m_playerId/m_slotId)
/// croisé avec `details().players[*].working_set_slot_id → toon_handle`. Réutilisé par Chunk 2.
pub fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, Error> {
    extract::player_toons(replay)
}
