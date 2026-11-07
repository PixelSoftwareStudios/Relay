//! The product audio-processing stage: WebRTC noise suppression, AGC2 and AEC3 through the
//! pure-Rust `sonora` port (BSD-3-Clause), behind the [`NoiseSuppressor`] contract.
//!
//! `sonora` allocates on every frame, so this stage must run on a media worker and never in a
//! device callback. With nothing enabled [`create_processor`] returns the plain
//! [`OffSuppressor`] copy, so a disabled stage costs nothing.

use sonora::config::{
	AdaptiveDigital, EchoCanceller, GainController2, NoiseSuppression, NoiseSuppressionLevel,
};
use sonora::{AudioProcessing, Config, StreamConfig};

use super::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, OffSuppressor,
	ProcessProgress, ProcessStatus, Provides, StageKind, ThreadSafety,
};
use crate::format::{BlockTiming, FRAME_SAMPLES, INTERNAL_RATE};

/// How hard noise suppression works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseLevel {
	/// About 6 dB.
	Low,
	/// About 12 dB.
	#[default]
	Moderate,
	/// About 18 dB.
	High,
}

impl NoiseLevel {
	fn to_sonora(self) -> NoiseSuppressionLevel {
		match self {
			Self::Low => NoiseSuppressionLevel::Low,
			Self::Moderate => NoiseSuppressionLevel::Moderate,
			Self::High => NoiseSuppressionLevel::High,
		}
	}
}

/// Which WebRTC stages run. The high-pass filter stays off: the chain's own stage owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SonoraConfig {
	/// Noise suppression at this level; `None` leaves it off.
	pub noise_suppression: Option<NoiseLevel>,
	/// Adaptive digital gain (AGC2) with its limiter.
	pub auto_gain: bool,
	/// Echo cancellation (AEC3). Needs the far-end reference through
	/// [`NoiseSuppressor::push_render`]; no caller supplies one yet (speaker mode, WP-12).
	pub echo_cancel: bool,
}

impl Default for SonoraConfig {
	/// Moderate noise suppression only.
	fn default() -> Self {
		Self {
			noise_suppression: Some(NoiseLevel::Moderate),
			auto_gain: false,
			echo_cancel: false,
		}
	}
}

impl SonoraConfig {
	/// Nothing enabled: the stage would only copy.
	pub fn is_bypass(&self) -> bool {
		self.noise_suppression.is_none() && !self.auto_gain && !self.echo_cancel
	}

	fn provides(&self) -> Provides {
		let mut provides = Provides::NONE;
		if self.noise_suppression.is_some() {
			provides = provides.with(StageKind::NoiseSuppress);
		}
		if self.auto_gain {
			provides = provides.with(StageKind::AutoGain).with(StageKind::Limit);
		}
		if self.echo_cancel {
			provides = provides.with(StageKind::EchoCancel);
		}
		provides
	}

	fn name(&self) -> String {
		let mut name = String::from("sonora");
		if let Some(level) = self.noise_suppression {
			name.push_str(match level {
				NoiseLevel::Low => "-ns-low",
				NoiseLevel::Moderate => "-ns",
				NoiseLevel::High => "-ns-high",
			});
		}
		if self.auto_gain {
			name.push_str("-agc");
		}
		if self.echo_cancel {
			name.push_str("-aec");
		}
		name
	}

	fn to_sonora(self) -> Config {
		Config {
			noise_suppression: self.noise_suppression.map(|level| NoiseSuppression {
				level: level.to_sonora(),
				..NoiseSuppression::default()
			}),
			gain_controller2: self.auto_gain.then(|| GainController2 {
				adaptive_digital: Some(AdaptiveDigital {
					// Speech starts at the level the microphone gain gave it and is raised
					// slowly; the ceiling keeps a quiet room from being amplified into hiss.
					initial_gain_db: 0.0,
					max_gain_db: 30.0,
					..AdaptiveDigital::default()
				}),
				..GainController2::default()
			}),
			echo_canceller: self.echo_cancel.then(EchoCanceller::default),
			..Config::default()
		}
	}
}

/// The processor for `config`: a plain copy when nothing is enabled, otherwise sonora.
pub fn create_processor(config: SonoraConfig) -> Box<dyn NoiseSuppressor> {
	if config.is_bypass() {
		Box::new(OffSuppressor::default())
	} else {
		Box::new(SonoraSuppressor::new(config))
	}
}

/// Delay noise suppression adds (the WebRTC band split and overlap, about 7 ms). Measured by
/// `the_declared_delay_matches_the_measured_one` in the crate tests; gain control adds none.
pub const SONORA_NS_DELAY_SAMPLES: usize = 334;

pub struct SonoraSuppressor {
	apm: AudioProcessing,
	descriptor: DspDescriptor,
	render_sink: Vec<f32>,
}

impl SonoraSuppressor {
	pub fn new(config: SonoraConfig) -> Self {
		let stream = StreamConfig::new(INTERNAL_RATE, 1);
		let apm = AudioProcessing::builder()
			.config(config.to_sonora())
			.capture_config(stream)
			.render_config(stream)
			.build();
		Self {
			apm,
			descriptor: DspDescriptor {
				backend_id: config.name(),
				version: "sonora 0.2.0".to_owned(),
				model_sha256: None,
				sample_rate_hz: INTERNAL_RATE,
				channels: 1,
				hop_samples: FRAME_SAMPLES,
				analysis_window_samples: FRAME_SAMPLES,
				lookahead_samples: None,
				declared_delay_samples: Some(if config.noise_suppression.is_some() {
					SONORA_NS_DELAY_SAMPLES
				} else {
					0
				}),
				warmup_samples: 0,
				state_bytes: None,
				max_scratch_bytes: 0,
				thread_safety: ThreadSafety::SingleWorker,
				license_evidence: Some("BSD-3-Clause (crate metadata)".to_owned()),
				provides: config.provides(),
			},
			render_sink: vec![0.0; FRAME_SAMPLES],
		}
	}
}

impl NoiseSuppressor for SonoraSuppressor {
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
		if input.len() < FRAME_SAMPLES {
			return Ok(ProcessProgress {
				consumed: 0,
				produced: 0,
				first_output_index: timing.first_sample,
				status: ProcessStatus::NeedMoreInput,
				discontinuity: None,
			});
		}
		if output.len() < FRAME_SAMPLES {
			return Err(DspError::OutputTooSmall {
				needed: FRAME_SAMPLES,
				available: output.len(),
			});
		}
		self.apm
			.process_capture_f32(
				&[&input[..FRAME_SAMPLES]],
				&mut [&mut output[..FRAME_SAMPLES]],
			)
			.map_err(|e| DspError::Backend(e.to_string()))?;
		Ok(ProcessProgress {
			consumed: FRAME_SAMPLES,
			produced: FRAME_SAMPLES,
			first_output_index: timing.first_sample,
			status: ProcessStatus::Ok,
			discontinuity: None,
		})
	}

	/// Far-end reference for echo cancellation (one 10 ms frame).
	fn push_render(&mut self, render: &[f32]) -> Result<(), DspError> {
		if render.len() != FRAME_SAMPLES {
			return Err(DspError::Backend("render frame must be 480 samples".into()));
		}
		self.apm
			.process_render_f32(&[render], &mut [&mut self.render_sink[..]])
			.map_err(|e| DspError::Backend(e.to_string()))
	}

	fn reset(&mut self, _reason: Discontinuity) {
		let stream = StreamConfig::new(INTERNAL_RATE, 1);
		self.apm.initialize(stream, stream, stream, stream);
	}
}
