//! Streaming noise-suppression adapter boundary.
//!
//! A compile-time contract, not a plugin ABI. Only validated, allowlisted
//! backends ship; unqualified neural backends are stubbed behind errors.

mod aec;
mod bench;
mod chain;
mod contract;
#[cfg(any(feature = "eval-sonora", feature = "eval-nnnoiseless"))]
pub mod eval;
mod filters;
mod manifest;
mod off;
mod registry;
mod stage;

pub use aec::{EchoCanceller, NoOpAec};
pub use bench::{BenchReport, BenchSummary, Harness, HarnessConfig, measure_delay_samples};
pub use chain::{ChainError, DspChain, DspPreferences};
pub use contract::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress,
	ProcessStatus, ThreadSafety,
};
pub use filters::{Agc, AgcConfig, HighPass};
pub use manifest::{ManifestError, ModelManifest, Status};
pub use off::OffSuppressor;
pub use registry::{BackendKind, create_suppressor};
pub use stage::{Provides, StageKind};
