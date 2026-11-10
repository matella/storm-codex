//! Framework-neutral core of the cast-overlay spike: wire model, pure logic, browser I/O.
pub mod logic;
pub mod model;
pub mod profile;
#[cfg(target_arch = "wasm32")]
pub mod net;
