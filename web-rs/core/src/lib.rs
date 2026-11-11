//! Framework-neutral core of the Rust front-end: wire models, pure logic, browser I/O (wasm only).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
pub mod heroes;
pub mod logic;
pub mod model;
#[cfg(target_arch = "wasm32")]
pub mod net;
pub mod scenes;
