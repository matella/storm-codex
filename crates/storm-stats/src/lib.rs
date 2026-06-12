//! `storm-stats` — stats de match Heroes of the Storm depuis un replay décodé.
//!
//! Port fidèle de la logique de [hots-parser](https://github.com/ebshimizu/hots-parser)
//! (la spec fonctionnelle du jalon 2), validé par diff automatique champ par champ contre
//! l'original (`tools/parity-harness/`). La sortie est la forme `{match, players, status}`
//! de hots-parser ; des vues typées seront exposées au fil des besoins du serveur (jalon 3).

pub mod constants;
mod convert;
mod process;

pub use process::{process_replay, Output};
