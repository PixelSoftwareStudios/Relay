//! Explicit channel selection and mono fan-out.

use super::Sample;

/// How a multi-channel capture device is reduced to mono.
///
/// Blindly summing channels can clip or cancel out-of-phase input, so the
/// default is an explicit channel; averaging is a documented opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelSelect {
	/// Use exactly this zero-based channel.
	Channel(u16),
	/// Average all channels.
	Average,
}

impl Default for ChannelSelect {
	fn default() -> Self {
		Self::Channel(0)
	}
}

impl ChannelSelect {
	/// Whether the selection is valid for a device with `channels` channels.
	pub fn is_valid_for(self, channels: u16) -> bool {
		channels > 0
			&& match self {
				Self::Channel(index) => index < channels,
				Self::Average => true,
			}
	}

	/// Reduce interleaved `input` to mono, writing `out[..input.len() / channels]`.
	///
	/// `out` must hold at least that many samples.
	pub fn to_mono<S: Sample>(self, input: &[S], channels: usize, out: &mut [f32]) {
		let frames = input.len() / channels;
		match self {
			Self::Channel(index) => {
				let index = usize::from(index).min(channels - 1);
				for (slot, frame) in out[..frames].iter_mut().zip(input.chunks_exact(channels)) {
					*slot = frame[index].to_f32();
				}
			}
			Self::Average => {
				let scale = 1.0 / channels as f32;
				for (slot, frame) in out[..frames].iter_mut().zip(input.chunks_exact(channels)) {
					*slot = frame.iter().map(|s| s.to_f32()).sum::<f32>() * scale;
				}
			}
		}
	}
}

/// Duplicate mono into interleaved device frames, saturating at the
/// conversion boundary. `out.len()` must be a multiple of `channels`.
pub fn fan_out<S: Sample>(mono: &[f32], channels: usize, out: &mut [S]) {
	for (frame, &value) in out.chunks_exact_mut(channels).zip(mono) {
		frame.fill(S::from_f32(value));
	}
}
