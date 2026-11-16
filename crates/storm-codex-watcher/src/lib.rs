//! Replay ingestion for the standalone Storm Codex install (port of `client-rs`, trimmed).
//!
//! The standalone server embeds this crate instead of running a separate uploader:
//! - [`detector`] finds the Heroes of the Storm replay folders (Documents known folder, so
//!   OneDrive-relocated folders work) or every replay folder under a chosen parent;
//! - [`upload`] posts one replay to `POST {base}/api/upload` and maps the answer
//!   (uploaded / duplicate / rejected / retryable, with exponential backoff);
//! - [`ledger`] persists the file names already handled so a restart does not re-send them;
//! - [`backfill`] uploads the existing archive, oldest first, 4 at a time, with progress;
//! - [`watch`] emits new replays once they are completely written.
//!
//! The server does the hashing and dedup; this crate has no GUI, tray or updater code.

pub mod backfill;
pub mod detector;
pub mod ledger;
pub mod upload;
pub mod watch;
