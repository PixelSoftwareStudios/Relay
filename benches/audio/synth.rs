//! Deterministic synthetic signals and SNR mixing. Synthetic speech-like
//! signals only exercise the machinery; they say nothing about real speech.

use std::f32::consts::TAU;

/// Small deterministic generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
	pub fn new(seed: u64) -> Self {
		Self(seed.max(1))
	}

	fn next_u64(&mut self) -> u64 {
		self.0 ^= self.0 >> 12;
		self.0 ^= self.0 << 25;
		self.0 ^= self.0 >> 27;
		self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
	}

	/// Uniform in `[-1, 1)`.
	pub fn uniform(&mut self) -> f32 {
		(self.next_u64() >> 40) as f32 / (1_u64 << 23) as f32 - 1.0
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseKind {
	White,
	/// Low-frequency heavy noise.
	Brown,
	/// Mains hum with harmonics plus a little white noise.
	Hum,
}

impl NoiseKind {
	pub const ALL: [NoiseKind; 3] = [Self::White, Self::Brown, Self::Hum];

	pub fn name(self) -> &'static str {
		match self {
			Self::White => "white",
			Self::Brown => "brown",
			Self::Hum => "hum",
		}
	}
}

/// Voiced, syllable-modulated harmonic signal with a gliding pitch.
pub fn speech_like(seconds: f32, rate: u32, seed: u64) -> Vec<f32> {
	let n = (seconds * rate as f32) as usize;
	let _ = seed; // deterministic by construction; seed reserved for variants
	let (mut phase, mut env_phase) = (0.0_f32, 0.0_f32);
	let mut out = Vec::with_capacity(n);
	for i in 0..n {
		let t = i as f32 / rate as f32;
		let f0 = 130.0 + 40.0 * (TAU * 0.7 * t).sin();
		phase = (phase + f0 / rate as f32).fract();
		env_phase = (env_phase + 3.0 / rate as f32).fract();
		// Syllables: smooth on/off envelope with silent gaps.
		let env = ((env_phase * TAU).sin() * 1.6).clamp(0.0, 1.0);
		let voiced: f32 = (1..=12)
			.map(|k| (TAU * phase * k as f32).sin() / (k as f32).powf(1.1))
			.sum();
		out.push(voiced * env * 0.25);
	}
	out
}

pub fn noise(kind: NoiseKind, seconds: f32, rate: u32, seed: u64) -> Vec<f32> {
	let n = (seconds * rate as f32) as usize;
	let mut rng = Rng::new(seed);
	let mut acc = 0.0_f32;
	(0..n)
		.map(|i| {
			let t = i as f32 / rate as f32;
			match kind {
				NoiseKind::White => rng.uniform(),
				NoiseKind::Brown => {
					acc = (acc + rng.uniform() * 0.05).clamp(-1.0, 1.0) * 0.998;
					acc
				}
				NoiseKind::Hum => {
					let hum: f32 = (1..=6)
						.map(|k| (TAU * 50.0 * k as f32 * t).sin() / k as f32)
						.sum();
					hum * 0.5 + rng.uniform() * 0.05
				}
			}
		})
		.collect()
}

pub fn rms(samples: &[f32]) -> f64 {
	if samples.is_empty() {
		return 0.0;
	}
	(samples
		.iter()
		.map(|&s| f64::from(s) * f64::from(s))
		.sum::<f64>()
		/ samples.len() as f64)
		.sqrt()
}

/// Scale `noise` so the mix has the requested SNR against `clean`, then
/// saturate at full scale. Returns an empty vector when either input is silent.
pub fn mix_at_snr(clean: &[f32], noise: &[f32], snr_db: f64) -> Vec<f32> {
	let n = clean.len().min(noise.len());
	let (c, w) = (rms(&clean[..n]), rms(&noise[..n]));
	if c == 0.0 || w == 0.0 {
		return Vec::new();
	}
	let gain = (c / w * 10f64.powf(-snr_db / 20.0)) as f32;
	(0..n)
		.map(|i| (clean[i] + noise[i] * gain).clamp(-1.0, 1.0))
		.collect()
}
