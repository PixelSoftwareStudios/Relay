//! Internal audio domain constants and timing metadata.

/// Internal processing rate in samples per second.
pub const INTERNAL_RATE: u32 = 48_000;
/// Samples in one 10 ms frame of the internal domain.
pub const FRAME_SAMPLES: usize = 480;
/// Maximum simultaneous calls the mixer accepts.
pub const MAX_CALLS: usize = 8;

/// Timing carried by every block of internal-domain audio.
///
/// Sample indices count elapsed source samples, including intervals during
/// which nothing was delivered (mute, overrun), so they keep describing the
/// source timeline instead of being re-based after a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockTiming {
	/// Input generation; bumped on every discontinuity.
	pub generation: u64,
	/// Index of the first sample in the 48 kHz source timeline.
	pub first_sample: u64,
}

/// Why a stream timeline was interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discontinuity {
	Start,
	Mute,
	Overrun,
	DeviceChange,
	Reset,
}

/// Replace non-finite samples with silence; returns how many were replaced.
pub fn sanitize(samples: &mut [f32]) -> usize {
	let mut replaced = 0;
	for sample in samples {
		if !sample.is_finite() {
			*sample = 0.0;
			replaced += 1;
		}
	}
	replaced
}
