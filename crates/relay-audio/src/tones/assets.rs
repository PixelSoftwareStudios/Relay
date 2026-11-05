//! Embedded sounds: raw little-endian i16 mono PCM at 48 kHz, audible part only.

use super::Sound;

const RING: &[u8] = include_bytes!("../../../../assets/sound/ring.raw");
const RINGBACK: &[u8] = include_bytes!("../../../../assets/sound/ringback.raw");
const CALL_END: &[u8] = include_bytes!("../../../../assets/sound/call-end.raw");
const CONNECTED: &[u8] = include_bytes!("../../../../assets/sound/connected.raw");

/// Samples per second of the embedded files.
const RATE: usize = 48_000;

impl Sound {
	/// Every embedded sound.
	pub const ALL: [Sound; 4] = [
		Sound::Ring,
		Sound::Ringback,
		Sound::CallEnd,
		Sound::Connected,
	];

	pub(super) fn index(self) -> usize {
		self as usize
	}

	pub(super) fn bytes(self) -> &'static [u8] {
		match self {
			Sound::Ring => RING,
			Sound::Ringback => RINGBACK,
			Sound::CallEnd => CALL_END,
			Sound::Connected => CONNECTED,
		}
	}

	/// Loop length in samples: the audible part plus trailing silence.
	/// One-shots return their own length.
	pub fn period_samples(self) -> usize {
		match self {
			Sound::Ring => 2 * RATE,
			Sound::Ringback => 3 * RATE,
			Sound::CallEnd | Sound::Connected => self.bytes().len() / 2,
		}
	}
}

pub(super) fn decode(bytes: &[u8]) -> Vec<f32> {
	bytes
		.as_chunks::<2>()
		.0
		.iter()
		.map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
		.collect()
}
