//! Rumble-removing high-pass filter and a conservative AGC.

use crate::format::INTERNAL_RATE;

/// Second-order Butterworth high-pass at 48 kHz.
#[derive(Debug, Clone)]
pub struct HighPass {
	b: [f64; 3],
	a: [f64; 2],
	z: [f64; 2],
}

impl HighPass {
	/// Voice-safe default corner.
	pub const DEFAULT_CUTOFF_HZ: f64 = 80.0;

	pub fn new(cutoff_hz: f64) -> Self {
		let w0 = std::f64::consts::TAU * cutoff_hz / f64::from(INTERNAL_RATE);
		let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
		let cos = w0.cos();
		let a0 = 1.0 + alpha;
		Self {
			b: [
				(1.0 + cos) / 2.0 / a0,
				-(1.0 + cos) / a0,
				(1.0 + cos) / 2.0 / a0,
			],
			a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
			z: [0.0; 2],
		}
	}

	pub fn reset(&mut self) {
		self.z = [0.0; 2];
	}

	/// Filter in place (transposed direct form II). Non-finite input becomes silence.
	pub fn process(&mut self, frame: &mut [f32]) {
		for sample in frame {
			let x = if sample.is_finite() {
				f64::from(*sample)
			} else {
				0.0
			};
			let y = self.b[0] * x + self.z[0];
			self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
			self.z[1] = self.b[2] * x - self.a[1] * y;
			*sample = y as f32;
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
