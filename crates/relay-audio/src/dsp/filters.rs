//! Rumble-removing high-pass filter and a conservative AGC.

use biquad::{Biquad, Coefficients, DirectForm2Transposed, Q_BUTTERWORTH_F32, ToHertz, Type};

use crate::format::INTERNAL_RATE;

/// Second-order Butterworth high-pass at 48 kHz (`biquad`, transposed direct form II, `f32`).
#[derive(Debug, Clone)]
pub struct HighPass {
	filter: DirectForm2Transposed<f32>,
}

impl HighPass {
	/// Voice-safe default corner.
	pub const DEFAULT_CUTOFF_HZ: f64 = 80.0;

	/// A corner outside the valid range is clamped rather than rejected.
	pub fn new(cutoff_hz: f64) -> Self {
		let rate = INTERNAL_RATE as f32;
		let cutoff = (cutoff_hz as f32).clamp(1.0, rate * 0.45);
		let coefficients = Coefficients::<f32>::from_params(
			Type::HighPass,
			rate.hz(),
			cutoff.hz(),
			Q_BUTTERWORTH_F32,
		)
		.expect("clamped corner is within (0, fs/2)");
		Self {
			filter: DirectForm2Transposed::<f32>::new(coefficients),
		}
	}

	pub fn reset(&mut self) {
		self.filter.reset_state();
	}

	/// Filter in place. Non-finite input becomes silence.
	pub fn process(&mut self, frame: &mut [f32]) {
		for sample in frame {
			let x = if sample.is_finite() { *sample } else { 0.0 };
			*sample = self.filter.run(x);
		}
	}
}

/// AGC limits. Gain can never exceed `max_gain_db`, never moves while the
/// input is below the gate (so room noise is not pumped up), and drifts back
/// to unity during gated passages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgcConfig {
	pub target_dbfs: f32,
	pub max_gain_db: f32,
	pub min_gain_db: f32,
	/// Frames quieter than this hold or release gain instead of raising it.
	pub gate_dbfs: f32,
	pub attack_db_per_s: f32,
	pub release_db_per_s: f32,
	/// Rate at which gain returns toward 0 dB while gated.
	pub relax_db_per_s: f32,
}

impl Default for AgcConfig {
	fn default() -> Self {
		Self {
			target_dbfs: -20.0,
			max_gain_db: 12.0,
			min_gain_db: -12.0,
			gate_dbfs: -50.0,
			attack_db_per_s: 30.0,
			release_db_per_s: 3.0,
			relax_db_per_s: 12.0,
		}
	}
}

/// Frame-wise AGC with per-sample gain ramps. Allocation-free.
#[derive(Debug, Clone)]
pub struct Agc {
	config: AgcConfig,
	gain_db: f32,
}

impl Agc {
	pub fn new(config: AgcConfig) -> Self {
		Self {
			config,
			gain_db: 0.0,
		}
	}

	pub fn gain_db(&self) -> f32 {
		self.gain_db
	}

	pub fn reset(&mut self) {
		self.gain_db = 0.0;
	}

	/// Adjust gain from this frame's level and apply it in place.
	pub fn process(&mut self, frame: &mut [f32]) {
		if frame.is_empty() {
			return;
		}
		let c = self.config;
		// A faulty upstream stage must not latch the gain: scrub non-finite samples first.
		for sample in frame.iter_mut().filter(|s| !s.is_finite()) {
			*sample = 0.0;
		}
		let seconds = frame.len() as f32 / INTERNAL_RATE as f32;
		let power = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
		let level_db = 10.0 * (power + 1e-12).log10();
		let previous = self.gain_db;
		if level_db < c.gate_dbfs {
			let step = c.relax_db_per_s * seconds;
			self.gain_db -= self.gain_db.clamp(-step, step);
		} else {
			let wanted = (c.target_dbfs - level_db).clamp(c.min_gain_db, c.max_gain_db);
			let up = c.release_db_per_s * seconds;
			let down = c.attack_db_per_s * seconds;
			self.gain_db += (wanted - self.gain_db).clamp(-down, up);
		}
		self.gain_db = self.gain_db.clamp(c.min_gain_db, c.max_gain_db);
		let from = 10.0_f32.powf(previous / 20.0);
		let to = 10.0_f32.powf(self.gain_db / 20.0);
		let step = (to - from) / frame.len() as f32;
		let mut gain = from;
		for sample in frame {
			gain += step;
			*sample *= gain;
		}
	}
}
