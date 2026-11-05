//! One encoder per call; state persists across frames.

use opus::{Application, Bitrate, Channels, Encoder};

use super::MAX_PACKET_BYTES;
use crate::error::{AudioError, Result};
use crate::format::{FRAME_SAMPLES, INTERNAL_RATE};

/// Encoder settings. Defaults are starting points, not tuned values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
	pub bitrate_bps: i32,
	pub complexity: i32,
	/// Negotiated in-band FEC; off until both sides agree.
	pub inband_fec: bool,
	/// Expected loss used to size FEC, in percent.
	pub expected_loss_pct: i32,
	/// DTX stays off for early timing tests.
	pub dtx: bool,
}

impl Default for EncoderConfig {
	fn default() -> Self {
		Self {
			bitrate_bps: 32_000,
			complexity: 5,
			inband_fec: false,
			expected_loss_pct: 0,
			dtx: false,
		}
	}
}

/// Mono 48 kHz, 10 ms-per-packet Opus encoder owned by one call.
pub struct CallEncoder {
	encoder: Encoder,
	config: EncoderConfig,
}

impl CallEncoder {
	/// Allocates; construct before the call route becomes ready.
	pub fn new(config: EncoderConfig) -> Result<Self> {
		if !(24_000..=48_000).contains(&config.bitrate_bps) {
			return Err(AudioError::InvalidArgument("bitrate outside 24-48 kbit/s"));
		}
		let mut encoder = Encoder::new(INTERNAL_RATE, Channels::Mono, Application::Voip)?;
		encoder.set_bitrate(Bitrate::Bits(config.bitrate_bps))?;
		encoder.set_complexity(config.complexity.clamp(0, 10))?;
		encoder.set_inband_fec(config.inband_fec)?;
		encoder.set_packet_loss_perc(config.expected_loss_pct.clamp(0, 100))?;
		encoder.set_dtx(config.dtx)?;
		Ok(Self { encoder, config })
	}

	pub fn config(&self) -> EncoderConfig {
		self.config
	}

	/// Encoder lookahead in samples (codec delay for the latency ledger).
	pub fn lookahead_samples(&mut self) -> Result<i32> {
		Ok(self.encoder.get_lookahead()?)
	}

	/// Encode exactly one 10 ms frame into `out`, returning the packet length.
	/// Allocation-free.
	pub fn encode(&mut self, frame: &[f32], out: &mut [u8]) -> Result<usize> {
		if frame.len() != FRAME_SAMPLES {
			return Err(AudioError::BadFrameSize {
				expected: FRAME_SAMPLES,
				actual: frame.len(),
			});
		}
		let cap = out.len().min(MAX_PACKET_BYTES);
		Ok(self.encoder.encode_float(frame, &mut out[..cap])?)
	}

	/// Forget codec history, e.g. at a mute boundary or new talk spurt.
	pub fn reset(&mut self) -> Result<()> {
		Ok(self.encoder.reset_state()?)
	}
}
