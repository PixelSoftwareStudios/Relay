//! The suppressor trait and its descriptor types.

use thiserror::Error;

use crate::format::BlockTiming;

/// Why a backend's timeline restarted.
pub use crate::format::Discontinuity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadSafety {
	/// Owned and driven by exactly one media worker.
	SingleWorker,
}

/// What a backend declares about itself. `None` means unknown, which is
/// acceptable for research adapters but never for a release default.
#[derive(Debug, Clone, PartialEq)]
pub struct DspDescriptor {
	pub backend_id: String,
	pub version: String,
	pub model_sha256: Option<String>,
	pub sample_rate_hz: u32,
	pub channels: u16,
	/// Input samples consumed per step.
	pub hop_samples: usize,
	pub analysis_window_samples: usize,
	pub lookahead_samples: Option<usize>,
	/// Samples between an input index and the output that reflects it.
	pub declared_delay_samples: Option<usize>,
	pub warmup_samples: usize,
	pub state_bytes: Option<usize>,
	pub max_scratch_bytes: usize,
	pub thread_safety: ThreadSafety,
	pub license_evidence: Option<String>,
	/// Processing stages this backend implements internally.
	pub provides: super::Provides,
}

/// Limits the engine grants at `prepare`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DspLimits {
	/// Largest input slice ever passed to one `process_into` call.
	pub max_block_samples: usize,
	pub max_scratch_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
	Ok,
	/// Not enough input to run a step; nothing was produced.
	NeedMoreInput,
	/// Output is not yet meaningful (warm-up).
	Warming,
}

/// Exact sample accounting for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessProgress {
	pub consumed: usize,
	pub produced: usize,
	/// Source-timeline index of `output[0]`. Never relabelled to hide delay.
	pub first_output_index: u64,
	pub status: ProcessStatus,
	pub discontinuity: Option<Discontinuity>,
}

#[derive(Debug, Error)]
pub enum DspError {
	#[error("backend {backend} is unavailable: {reason}")]
	BackendUnavailable { backend: String, reason: String },
	#[error("backend not prepared")]
	NotPrepared,
	#[error("block of {actual} samples exceeds prepared limit {limit}")]
	BlockTooLarge { actual: usize, limit: usize },
	#[error("output buffer holds {available}, need at least {needed}")]
	OutputTooSmall { needed: usize, available: usize },
	#[error("scratch need {needed} bytes exceeds limit {limit}")]
	ScratchTooLarge { needed: usize, limit: usize },
	#[error("backend failure: {0}")]
	Backend(String),
}

/// Streaming, allocation-free-after-prepare noise suppressor.
pub trait NoiseSuppressor: Send {
	fn descriptor(&self) -> &DspDescriptor;

	/// Allocate, parse models and warm up. Runs off the deadline path.
	fn prepare(&mut self, limits: &DspLimits) -> Result<(), DspError>;

	/// Process what is available; may consume less than offered. The output
	/// is bounded by `output.len()`; backends never allocate to satisfy it.
	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		timing: BlockTiming,
	) -> Result<ProcessProgress, DspError>;

	/// Far-end reference for echo cancellation; backends without AEC ignore it.
	fn push_render(&mut self, _render: &[f32]) -> Result<(), DspError> {
		Ok(())
	}

	/// Restart the stream. Must be bounded and preallocated.
	fn reset(&mut self, reason: Discontinuity);
}
