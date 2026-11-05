//! The explicit zero-delay baseline.

use super::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress,
	ProcessStatus, Provides, ThreadSafety,
};
use crate::format::{BlockTiming, INTERNAL_RATE};

/// Copies input to output with no processing and no delay. It catches
/// latency introduced by the framework itself.
pub struct OffSuppressor {
	descriptor: DspDescriptor,
}

impl Default for OffSuppressor {
	fn default() -> Self {
		Self {
			descriptor: DspDescriptor {
				backend_id: "off".to_owned(),
				version: env!("CARGO_PKG_VERSION").to_owned(),
				model_sha256: None,
				sample_rate_hz: INTERNAL_RATE,
				channels: 1,
				hop_samples: 1,
				analysis_window_samples: 1,
				lookahead_samples: Some(0),
				declared_delay_samples: Some(0),
				warmup_samples: 0,
				state_bytes: Some(0),
				max_scratch_bytes: 0,
				thread_safety: ThreadSafety::SingleWorker,
				license_evidence: None,
				provides: Provides::NONE,
			},
		}
	}
}

impl NoiseSuppressor for OffSuppressor {
	fn descriptor(&self) -> &DspDescriptor {
		&self.descriptor
	}

	fn prepare(&mut self, _limits: &DspLimits) -> Result<(), DspError> {
		Ok(())
	}

	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		timing: BlockTiming,
	) -> Result<ProcessProgress, DspError> {
		let n = input.len().min(output.len());
		output[..n].copy_from_slice(&input[..n]);
		Ok(ProcessProgress {
			consumed: n,
			produced: n,
			first_output_index: timing.first_sample,
			status: if n == 0 {
				ProcessStatus::NeedMoreInput
			} else {
				ProcessStatus::Ok
			},
			discontinuity: None,
		})
	}

	fn reset(&mut self, _reason: Discontinuity) {}
}
