//! Audio comparison tooling: corpus I/O, mixing, metrics and the runner.

pub mod corpus;
pub mod metrics;
#[cfg(feature = "neural")]
pub mod neural;
pub mod real;
pub mod runner;
pub mod stoi;
pub mod summary;
pub mod synth;
pub mod wav;
pub mod worker;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchError {
	#[error("wav error: {0}")]
	Wav(String),
	#[error("io error: {0}")]
	Io(#[from] std::io::Error),
	#[error("backend {0} is not available in this build (enable the matching cargo feature)")]
	BackendUnavailable(String),
	#[error("dsp error: {0}")]
	Dsp(String),
	#[error("corpus problem: {0}")]
	Corpus(String),
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, BenchError>;
