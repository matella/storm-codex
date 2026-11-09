//! `storm-stats` — Heroes of the Storm match stats from a decoded replay.
//!
//! Faithful port of the logic of [hots-parser](https://github.com/ebshimizu/hots-parser)
//! (the functional spec of milestone 2), validated by an automatic field-by-field diff against
//! the original (`tools/parity-harness/`). The output is hots-parser's `{match, players, status}`
//! shape; typed views will be exposed as the server needs them (milestone 3).

pub mod constants;
mod convert;
mod process;

pub use process::{process_replay, Output};
