//! WebRTC audio processing through the pure-Rust `sonora` port (BSD-3-Clause).

use sonora::config::{EchoCanceller, GainController2, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::dsp::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress,
	ProcessStatus, Provides, StageKind, ThreadSafety,
};
use crate::format::{BlockTiming, FRAME_SAMPLES, INTERNAL_RATE};

/// Which WebRTC stages run. The high-pass filter stays off so the chain's own
/// stage owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonoraProfile {
	NsOnly,
	NsAgc,
	NsAec,
}

impl SonoraProfile {
	fn name(self) -> &'static str {
		match self {
			Self::NsOnly => "sonora-ns",
			Self::NsAgc => "sonora-ns-agc",
			Self::NsAec => "sonora-ns-aec",
		}
	}

	fn provides(self) -> Provides {
		let ns = Provides::of(StageKind::NoiseSuppress);
		match self {
			Self::NsOnly => ns,
			Self::NsAgc => ns.with(StageKind::AutoGain),
			Self::NsAec => ns.with(StageKind::EchoCancel),
		}
	}

	fn config(self) -> Config {
		Config {
			noise_suppression: Some(NoiseSuppression::default()),
			gain_controller2: matches!(self, Self::NsAgc).then(GainController2::default),
			echo_canceller: matches!(self, Self::NsAec).then(EchoCanceller::default),
			..Config::default()
		}
	}
}

pub struct SonoraSuppressor {
	apm: AudioProcessing,
	descriptor: DspDescriptor,
	render_sink: Vec<f32>,
}

impl SonoraSuppressor {
	pub fn new(profile: SonoraProfile) -> Self {
		let stream = StreamConfig::new(INTERNAL_RATE, 1);
		let apm = AudioProcessing::builder()
			.config(profile.config())
			.capture_config(stream)
			.render_config(stream)
			.build();
		Self {
			apm,
			descriptor: DspDescriptor {
				backend_id: profile.name().to_owned(),
				version: "sonora 0.2.0".to_owned(),
				model_sha256: None,
				sample_rate_hz: INTERNAL_RATE,
				channels: 1,
				hop_samples: FRAME_SAMPLES,
				analysis_window_samples: FRAME_SAMPLES,
				lookahead_samples: None,
				declared_delay_samples: None,
				warmup_samples: 0,
				state_bytes: None,
				max_scratch_bytes: 0,
				thread_safety: ThreadSafety::SingleWorker,
				license_evidence: Some("BSD-3-Clause (crate metadata)".to_owned()),
				provides: profile.provides(),
			},
			render_sink: vec![0.0; FRAME_SAMPLES],
		}
	}

	/// Far-end reference for the AEC profile (one 10 ms frame).
	pub fn push_render(&mut self, render: &[f32]) -> Result<(), DspError> {
		if render.len() != FRAME_SAMPLES {
			return Err(DspError::Backend("render frame must be 480 samples".into()));
		}
		self.apm
			.process_render_f32(&[render], &mut [&mut self.render_sink[..]])
			.map_err(|e| DspError::Backend(e.to_string()))
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

	fn reset(&mut self, _reason: Discontinuity) {
		let stream = StreamConfig::new(INTERNAL_RATE, 1);
		self.apm.initialize(stream, stream, stream, stream);
	}
}
