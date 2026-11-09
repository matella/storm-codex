//! Scouting reports (spec `docs/specs/2026-09-26-rapports-scouting-design.md`): replays of an
//! opposing team → numeric facts → LLM pack → re-imported analysis. Pure core (`summary`, `side`,
//! `facts`, `pack`, `xlsx`, `analysis`) + HTTP/DB layer (`api`). The scouting replays live
//! in their own tables: they never touch `matches`/`match_players`.

pub mod analysis;
pub mod api;
pub mod facts;
pub mod pack;
pub mod side;
pub mod summary;
pub mod xlsx;
