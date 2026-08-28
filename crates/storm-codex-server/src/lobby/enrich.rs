//! Enrichissement du lobby décodé : résolution d'identité (`toon_handle`) et agrégats d'archive
//! (`history`, `build`, `me_stats`). Non-op pour l'instant — la tâche 4 y branchera la résolution
//! BattleTag → `toon_handle` via `match_players_name_tag_idx` (migration 0009) et les agrégats.
//!
//! Déclaré ici (plutôt que dans la tâche 4) car `lobby::api::ingest` l'appelle déjà : le point
//! d'extension existe dès la tâche 3, son contenu arrive à la tâche 4.
use crate::lobby::LobbyState;
use sqlx::PgPool;

/// Enrichit l'état en place. Best-effort : ne doit jamais faire échouer l'ingestion.
pub async fn enrich(_db: &PgPool, _state: &mut LobbyState) {}
