//! Benchmark and evaluation tooling, organised by domain.
//!
//! Only measurements taken on a real run are ever reported; anything that
//! cannot be observed stays `null`.

pub mod audio;
#[path = "listening/mod.rs"]
pub mod listening;
