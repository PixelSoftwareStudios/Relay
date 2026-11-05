//! The ordered processing chain: high-pass, AEC, NS, AGC, limiter.

use thiserror::Error;

use super::{
	Agc, AgcConfig, DspError, DspLimits, EchoCanceller, HighPass, NoOpAec, NoiseSuppressor,
	Provides, StageKind,
};
use crate::format::{BlockTiming, FRAME_SAMPLES};
use crate::mixer::Limiter;

/// User-facing switches. Only the high-pass filter is on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DspPreferences {
	pub high_pass: bool,
	pub echo_cancel: bool,
	pub noise_suppression: bool,
	pub auto_gain: bool,
	pub limiter: bool,
}

impl Default for DspPreferences {
	fn default() -> Self {
		Self {
			high_pass: true,
			echo_cancel: false,
			noise_suppression: false,
			auto_gain: false,
			limiter: false,
		}
	}
}

#[derive(Debug, Error)]
pub enum ChainError {
	#[error("{0:?} is provided by more than one stage")]
	DuplicateStage(StageKind),
	#[error("{0:?} is enabled but no backend that provides it was supplied")]
	MissingBackend(StageKind),
	#[error("frame must hold exactly {FRAME_SAMPLES} samples, got {0}")]
	BadFrame(usize),
	#[error("noise suppressor produced {0} samples instead of one whole frame")]
	Unaligned(usize),
	#[error(transparent)]
	Dsp(#[from] DspError),
}

/// A validated, prepared chain operating on 10 ms frames in place.
pub struct DspChain {
	high_pass: Option<HighPass>,
	aec: Box<dyn EchoCanceller>,
	ns: Option<Box<dyn NoiseSuppressor>>,
	agc: Option<Agc>,
	limiter: Option<Limiter>,
	scratch: Vec<f32>,
	provides: Provides,
}

impl DspChain {
	/// Build and validate. A backend that already provides a stage (for
	/// example NS plus AGC) conflicts with enabling that stage separately,
	/// so double processing is impossible by construction.
	pub fn new(
		prefs: DspPreferences,
		ns: Option<Box<dyn NoiseSuppressor>>,
		aec: Option<Box<dyn EchoCanceller>>,
	) -> Result<Self, ChainError> {
		let mut provides = Provides::NONE;
		let mut claim = |p: Provides| -> Result<(), ChainError> {
			if let Some(kind) = provides.overlap(p) {
				return Err(ChainError::DuplicateStage(kind));
			}
			provides = provides.union(p);
			Ok(())
		};

		if prefs.high_pass {
			claim(Provides::of(StageKind::HighPass))?;
		}
		let aec: Box<dyn EchoCanceller> = match (prefs.echo_cancel, aec) {
			(true, Some(aec)) if aec.provides().contains(StageKind::EchoCancel) => aec,
			(true, _) => return Err(ChainError::MissingBackend(StageKind::EchoCancel)),
			// Disabled by preference: a supplied canceller is dropped.
			(false, _) => Box::new(NoOpAec),
		};
		claim(aec.provides())?;
		let ns = match (prefs.noise_suppression, ns) {
			(true, Some(ns)) => {
				claim(ns.descriptor().provides)?;
				if !ns.descriptor().provides.contains(StageKind::NoiseSuppress) {
					return Err(ChainError::MissingBackend(StageKind::NoiseSuppress));
				}
				Some(ns)
			}
			(true, None) => return Err(ChainError::MissingBackend(StageKind::NoiseSuppress)),
			(false, _) => None,
		};
		if prefs.auto_gain {
			claim(Provides::of(StageKind::AutoGain))?;
		}
		if prefs.limiter {
			claim(Provides::of(StageKind::Limit))?;
		}
		let mut ns = ns;
		if let Some(backend) = ns.as_mut() {
			backend.prepare(&DspLimits {
				max_block_samples: FRAME_SAMPLES,
				max_scratch_bytes: backend.descriptor().max_scratch_bytes,
			})?;
		}
		Ok(Self {
			high_pass: prefs
				.high_pass
				.then(|| HighPass::new(HighPass::DEFAULT_CUTOFF_HZ)),
			aec,
			ns,
			agc: prefs.auto_gain.then(|| Agc::new(AgcConfig::default())),
			limiter: prefs.limiter.then(Limiter::default),
			scratch: vec![0.0; FRAME_SAMPLES * 2],
			provides,
		})
	}

	/// Everything the chain does, for display and diagnostics.
	pub fn provides(&self) -> Provides {
		self.provides
	}

	/// Feed the far-end reference for echo cancellation.
	pub fn push_render(&mut self, render: &[f32]) -> Result<(), ChainError> {
		Ok(self.aec.push_render(render)?)
	}

	/// Process one capture frame in place. Allocation-free.
	pub fn process(&mut self, frame: &mut [f32], timing: BlockTiming) -> Result<(), ChainError> {
		if frame.len() != FRAME_SAMPLES {
			return Err(ChainError::BadFrame(frame.len()));
		}
		if let Some(hp) = &mut self.high_pass {
			hp.process(frame);
		}
		self.aec.process_capture(frame)?;
		if let Some(ns) = &mut self.ns {
			let progress = ns.process_into(frame, &mut self.scratch, timing)?;
			if progress.produced != FRAME_SAMPLES {
				return Err(ChainError::Unaligned(progress.produced));
			}
			frame.copy_from_slice(&self.scratch[..FRAME_SAMPLES]);
		}
		if let Some(agc) = &mut self.agc {
			agc.process(frame);
		}
		if let Some(limiter) = &mut self.limiter {
			for sample in frame.iter_mut() {
				*sample = limiter.process(*sample);
			}
		}
		Ok(())
	}

	/// Restart all stage state at a discontinuity.
	pub fn reset(&mut self, reason: super::Discontinuity) {
		if let Some(hp) = &mut self.high_pass {
			hp.reset();
		}
		self.aec.reset();
		if let Some(ns) = &mut self.ns {
			ns.reset(reason);
		}
		if let Some(agc) = &mut self.agc {
			agc.reset();
		}
	}
}
