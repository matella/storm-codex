//! Rapports de scouting (spec `docs/specs/2026-09-26-rapports-scouting-design.md`) : replays d'une
//! équipe adverse → faits chiffrés → pack LLM → analyse réimportée. Cœur pur (`summary`, `side`,
//! `facts`, `pack`, `xlsx`, `analysis`) + couche HTTP/DB (`api`). Les replays de scouting vivent
//! dans leurs propres tables : ils ne touchent jamais `matches`/`match_players`.

pub mod analysis;
pub mod api;
pub mod facts;
pub mod pack;
pub mod side;
pub mod summary;
pub mod xlsx;
