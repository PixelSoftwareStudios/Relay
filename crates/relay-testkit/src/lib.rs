//! Support code shared by tests and benches across the workspace.
//!
//! Depend on it only from `[dev-dependencies]`. It is a workspace member but not a default
//! member, so a plain `cargo build` never compiles it.

#![deny(missing_docs)]

mod alloc;

pub use alloc::{CountingAlloc, allocations_in, total_allocations};
