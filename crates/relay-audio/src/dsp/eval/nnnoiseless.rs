//! RNNoise candidate through the pure-Rust `nnnoiseless` port (BSD-3-Clause,
//! weights shipped inside the crate). Evaluation only.

use nnnoiseless::{DenoiseState, FRAME_SIZE};

use crate::dsp::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress,
	ProcessStatus, Provides, StageKind, ThreadSafety,
};
use crate::format::{BlockTiming, FRAME_SAMPLES, INTERNAL_RATE};

/// RNNoise expects samples at 16-bit integer scale.
const PCM_SCALE: f32 = 32_768.0;

pub struct NnnoiselessSuppressor {
	state: Box<DenoiseState<'static>>,
	descriptor: DspDescriptor,
	input: [f32; FRAME_SIZE],
	output: [f32; FRAME_SIZE],
	last_vad: f32,
}

impl Default for NnnoiselessSuppressor {
	fn default() -> Self {
		Self::new()
	}
}

impl NnnoiselessSuppressor {
	pub fn new() -> Self {
		Self {
			state: DenoiseState::new(),
			descriptor: DspDescriptor {
				backend_id: "rnnoise-nnnoiseless".to_owned(),
				version: "nnnoiseless 0.5.2".to_owned(),
				model_sha256: None,
				sample_rate_hz: INTERNAL_RATE,
				channels: 1,
				hop_samples: FRAME_SIZE,
				analysis_window_samples: FRAME_SIZE * 2,
				lookahead_samples: None,
				declared_delay_samples: None,
				// The first output frame is a fade-in artefact of the model.
				warmup_samples: FRAME_SIZE,
				state_bytes: None,
				max_scratch_bytes: 0,
				thread_safety: ThreadSafety::SingleWorker,
				license_evidence: Some("BSD-3-Clause (crate metadata)".to_owned()),
				provides: Provides::of(StageKind::NoiseSuppress),
			},
			input: [0.0; FRAME_SIZE],
			output: [0.0; FRAME_SIZE],
			last_vad: 0.0,
		}
	}

	/// Voice-activity probability of the most recent frame.
	pub fn last_vad(&self) -> f32 {
		self.last_vad
	}
}

const _: () = assert!(FRAME_SIZE == FRAME_SAMPLES);

impl NoiseSuppressor for NnnoiselessSuppressor {
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
		if input.len() < FRAME_SIZE {
			return Ok(ProcessProgress {
				consumed: 0,
				produced: 0,
				first_output_index: timing.first_sample,
				status: ProcessStatus::NeedMoreInput,
				discontinuity: None,
			});
		}
		if output.len() < FRAME_SIZE {
			return Err(DspError::OutputTooSmall {
				needed: FRAME_SIZE,
				available: output.len(),
			});
		}
		for (dst, src) in self.input.iter_mut().zip(input) {
			*dst = if src.is_finite() {
				src.clamp(-1.0, 1.0) * PCM_SCALE
			} else {
				0.0
			};
		}
		self.last_vad = self.state.process_frame(&mut self.output, &self.input);
		for (dst, src) in output.iter_mut().zip(&self.output) {
			*dst = (src / PCM_SCALE).clamp(-1.0, 1.0);
		}
		Ok(ProcessProgress {
			consumed: FRAME_SIZE,
			produced: FRAME_SIZE,
			first_output_index: timing.first_sample,
			status: ProcessStatus::Ok,
			discontinuity: None,
		})
	}

	fn reset(&mut self, _reason: Discontinuity) {
		self.state = DenoiseState::new();
	}
}
