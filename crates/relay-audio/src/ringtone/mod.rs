//! A user-chosen ringtone: decode once, off the real-time thread, into the ring format.
//!
//! [`process`] turns an uploaded file (MP3, Ogg Vorbis, FLAC, WAV/PCM, AAC) into mono 48 kHz
//! samples ready for [`crate::tones::ToneControl::set_custom_ring`]: resampled, levelled to the
//! built-in ring's loudness, faded in and out and followed by a pause, so looping it never
//! clicks and never plays back to back. The result is stored as Ogg Opus
//! ([`to_stored`] / [`from_stored`]); the real-time side only ever reads the finished buffer.

mod decode;
mod stored;

use std::f32::consts::PI;

use thiserror::Error;

use crate::convert::StreamResampler;
use crate::format::INTERNAL_RATE;

/// Peak of a processed ringtone at volume 1.0: -12 dBFS, the same as the built-in chime.
pub const RING_PEAK: f32 = 0.25;
/// RMS ceiling of a processed ringtone at volume 1.0 (about -23 dBFS), so dense music does not
/// end up louder than the sparse chime at the same peak.
pub const RING_RMS: f32 = 0.07;
/// Largest boost a quiet file gets (24 dB).
const MAX_BOOST: f32 = 16.0;
/// Fade at both ends (30 ms).
const FADE_SAMPLES: usize = 1_440;
/// Silence after the audible part: the pause between rings (1 s).
pub const PAUSE_SAMPLES: usize = INTERNAL_RATE as usize;
/// Shortest accepted audio (0.3 s).
const MIN_SAMPLES: usize = 14_400;
/// Below this peak the file counts as silent.
const SILENCE: f32 = 1e-3;

/// Limits applied to an uploaded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingtoneLimits {
	/// Largest accepted file in bytes.
	pub max_bytes: usize,
	/// Longest accepted audio in seconds.
	pub max_seconds: u32,
}

/// Why a file cannot be a ringtone.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RingtoneError {
	#[error("the file is empty")]
	Empty,
	#[error("the file is larger than {max_bytes} bytes")]
	TooLarge { max_bytes: usize },
	#[error("the audio is longer than {max_seconds} seconds")]
	TooLong { max_seconds: u32 },
	#[error("the audio is shorter than a third of a second")]
	TooShort,
	#[error("the audio format is not supported")]
	Unsupported,
	#[error("the audio could not be decoded: {0}")]
	Corrupt(String),
	#[error("the audio is silent")]
	Silent,
}

impl RingtoneError {
	/// Stable machine code, for the API and the translated message.
	pub fn code(&self) -> &'static str {
		match self {
			Self::Empty => "RINGTONE_EMPTY",
			Self::TooLarge { .. } => "RINGTONE_TOO_LARGE",
			Self::TooLong { .. } => "RINGTONE_TOO_LONG",
			Self::TooShort => "RINGTONE_TOO_SHORT",
			Self::Unsupported => "RINGTONE_UNSUPPORTED",
			Self::Corrupt(_) => "RINGTONE_CORRUPT",
			Self::Silent => "RINGTONE_SILENT",
		}
	}
}

/// Decodes and prepares `bytes`. Blocking and allocating: never call it from a real-time thread.
pub fn process(bytes: &[u8], limits: RingtoneLimits) -> Result<Vec<f32>, RingtoneError> {
	if bytes.is_empty() {
		return Err(RingtoneError::Empty);
	}
	if bytes.len() > limits.max_bytes {
		return Err(RingtoneError::TooLarge {
			max_bytes: limits.max_bytes,
		});
	}
	let decoded = decode::decode(bytes, limits.max_seconds)?;
	let mut samples = to_ring_rate(decoded.samples, decoded.rate)?;
	if samples.len() > limits.max_seconds as usize * INTERNAL_RATE as usize {
		return Err(RingtoneError::TooLong {
			max_seconds: limits.max_seconds,
		});
	}
	if samples.len() < MIN_SAMPLES {
		return Err(RingtoneError::TooShort);
	}
	level(&mut samples)?;
	fade(&mut samples);
	samples.resize(samples.len() + PAUSE_SAMPLES, 0.0);
	Ok(samples)
}

/// Resamples to 48 kHz, dropping the filter delay so the sound starts where the file does.
fn to_ring_rate(samples: Vec<f32>, rate: u32) -> Result<Vec<f32>, RingtoneError> {
	if rate == INTERNAL_RATE {
		return Ok(samples);
	}
	let expected = (samples.len() as u64 * u64::from(INTERNAL_RATE)).div_ceil(u64::from(rate));
	let mut resampler = StreamResampler::new_fixed(rate, INTERNAL_RATE, 1024)
		.map_err(|e| RingtoneError::Corrupt(e.to_string()))?;
	let delay = resampler.delay_samples();
	let mut out = Vec::with_capacity(expected as usize + delay + 2048);
	let flush = vec![0.0_f32; 2048 + delay * rate as usize / INTERNAL_RATE as usize];
	for block in [&samples[..], &flush[..]] {
		resampler
			.push(block, |chunk| out.extend_from_slice(chunk))
			.map_err(|e| RingtoneError::Corrupt(e.to_string()))?;
	}
	out.drain(..delay.min(out.len()));
	out.truncate(expected as usize);
	Ok(out)
}

/// Removes DC, then scales to [`RING_PEAK`] and [`RING_RMS`], whichever is quieter.
fn level(samples: &mut [f32]) -> Result<(), RingtoneError> {
	let mean = samples.iter().sum::<f32>() / samples.len() as f32;
	let (mut peak, mut energy) = (0.0_f32, 0.0_f64);
	for s in samples.iter_mut() {
		*s -= mean;
		peak = peak.max(s.abs());
		energy += f64::from(*s) * f64::from(*s);
	}
	let rms = (energy / samples.len() as f64).sqrt() as f32;
	if peak < SILENCE || !peak.is_finite() {
		return Err(RingtoneError::Silent);
	}
	let gain = (RING_PEAK / peak)
		.min(RING_RMS / rms.max(1e-6))
		.min(MAX_BOOST);
	for s in samples.iter_mut() {
		*s *= gain;
	}
	Ok(())
}

/// Raised-cosine fade at both ends, so a cut-off file starts and ends at exactly zero.
fn fade(samples: &mut [f32]) {
	let n = FADE_SAMPLES.min(samples.len() / 2);
	for i in 0..n {
		let g = 0.5 - 0.5 * (PI * i as f32 / n as f32).cos();
		samples[i] *= g;
		let last = samples.len() - 1 - i;
		samples[last] *= g;
	}
}

/// The stored form of a processed ringtone: mono 48 kHz Ogg Opus. `None` if encoding fails.
pub fn to_stored(samples: &[f32]) -> Option<Vec<u8>> {
	stored::encode(samples)
}

/// Reads [`to_stored`] output back to exactly the samples that were stored; `None` when the
/// bytes are not such a file.
pub fn from_stored(bytes: &[u8]) -> Option<Vec<f32>> {
	stored::decode(bytes)
}
