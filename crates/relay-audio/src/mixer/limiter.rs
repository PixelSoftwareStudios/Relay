//! Sample-wise safety limiter: instant attack, smooth release, hard ceiling.

/// Level above which gain reduction starts.
const THRESHOLD: f32 = 0.9;
/// Release time constant in samples (about 50 ms at 48 kHz).
const RELEASE_COEFF: f32 = 0.999_58;

#[derive(Debug, Clone)]
pub struct Limiter {
	gain: f32,
	limited_samples: u64,
}

impl Default for Limiter {
	fn default() -> Self {
		Self {
			gain: 1.0,
			limited_samples: 0,
		}
	}
}

impl Limiter {
	/// Samples on which gain reduction was applied.
	pub fn limited_samples(&self) -> u64 {
		self.limited_samples
	}

	pub fn process(&mut self, sample: f32) -> f32 {
		let peak = sample.abs();
		let needed = if peak * self.gain > THRESHOLD {
			THRESHOLD / peak
		} else {
			1.0
		};
		if needed < self.gain {
			self.gain = needed;
			self.limited_samples += 1;
		} else {
			self.gain = needed + (self.gain - needed) * RELEASE_COEFF;
			if self.gain < 1.0 {
				self.limited_samples += 1;
			}
		}
		(sample * self.gain).clamp(-1.0, 1.0)
	}
}
