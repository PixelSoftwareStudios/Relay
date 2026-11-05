//! Evaluation-only suppressor candidates. Never compiled by default and never
//! reachable from `create_suppressor`; they exist so the benchmark harness can
//! compare them with the Off baseline.

#[cfg(feature = "eval-nnnoiseless")]
mod nnnoiseless;
#[cfg(feature = "eval-sonora")]
mod sonora;

#[cfg(feature = "eval-nnnoiseless")]
pub use nnnoiseless::NnnoiselessSuppressor;
#[cfg(feature = "eval-sonora")]
pub use sonora::{SonoraProfile, SonoraSuppressor};
