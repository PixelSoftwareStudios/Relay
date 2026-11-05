//! Streaming noise-suppression adapter boundary.
//!
//! A compile-time contract, not a plugin ABI. Only validated, allowlisted
//! backends ship; unqualified neural backends are stubbed behind errors.

mod bench;
mod contract;
mod manifest;
mod off;
mod registry;

pub use bench::{BenchReport, BenchSummary, Harness, HarnessConfig, measure_delay_samples};
pub use contract::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress,
	ProcessStatus, ThreadSafety,
};
pub use manifest::{ManifestError, ModelManifest, Status};
pub use off::OffSuppressor;
pub use registry::{BackendKind, create_suppressor};
