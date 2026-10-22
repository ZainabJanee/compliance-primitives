//! Test-only helpers for pretty-printing captured Soroban contract events.
//!
//! Event assertions like
//!
//! ```ignore
//! assert_eq!(env.events().all(), vec![&env, (contract_id, topics, data), ...]);
//! ```
//!
//! fail with a single-line `Debug` dump of raw XDR, which is hard to read.
//! This crate renders events one per block with decoded topics and data:
//!
//! ```text
//! 2 event(s):
//!   [0] contract: CA3D5K…
//!       topics: [sym:allow_add, addr:CB7Q…]
//!       data:   {}
//!   [1] contract: CA3D5K…
//!       topics: [sym:blocked, addr:CB7Q…, addr:CC2M…]
//!       data:   {sym:amount: i128:500}
//! ```
//!
//! (addresses shortened here; the real output prints full strkeys.)
//!
//! ## Usage
//!
//! Add it as a **dev-dependency only**:
//!
//! ```toml
//! [dev-dependencies]
//! compliance-test-trace = { path = "../../crates/test-trace" }
//! ```
//!
//! then, in a test:
//!
//! - [`assert_events_eq`] — drop-in replacement for
//!   `assert_eq!(env.events().all(), expected)` that pretty-prints both sides
//!   on mismatch.
//! - [`print_events`] — dump the currently captured events (shown when the
//!   test fails, or always with `cargo test -- --nocapture`).
//! - [`format_events`] — the formatted string, for custom messages.
//!
//! ## Not in release/wasm builds
//!
//! This crate is only referenced from `[dev-dependencies]`, so it is never
//! linked into a contract's release or wasm artifact. When the workspace is
//! built for a wasm target the crate compiles to an empty `#![no_std]` lib.
#![no_std]

#[cfg(not(target_family = "wasm"))]
mod imp;

#[cfg(not(target_family = "wasm"))]
pub use imp::*;
