//! One decoder per call; concealment and FEC are explicit hooks.

use opus::{Channels, Decoder};

use crate::error::{AudioError, Result};
use crate::format::{FRAME_SAMPLES, INTERNAL_RATE};

/// How an output frame was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeKind {
	Normal,
	/// Packet-loss concealment: no packet was available.
	Concealed,
	/// The previous frame recovered from in-band FEC in the next packet.
	FecRecovered,
}

/// Mono 48 kHz Opus decoder owned by one call.
pub struct CallDecoder {
	decoder: Decoder,
	consecutive_errors: u32,
}

impl CallDecoder {
	pub fn new() -> Result<Self> {
		Ok(Self {
			decoder: Decoder::new(INTERNAL_RATE, Channels::Mono)?,
			consecutive_errors: 0,
		})
	}

	/// Consecutive failed decodes; callers escalate to a call-specific failure.
	pub fn consecutive_errors(&self) -> u32 {
		self.consecutive_errors
	}

	/// Decode a 10 ms packet into `out` (exactly [`FRAME_SAMPLES`] long).
	/// The packet's sample count is validated before decoding.
	pub fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<DecodeKind> {
		Self::check_out(out)?;
		let samples = self
			.decoder
			.get_nb_samples(packet)
			.map_err(|e| self.fail(e.into()))?;
		if samples != FRAME_SAMPLES {
			return Err(self.fail(AudioError::BadFrameSize {
				expected: FRAME_SAMPLES,
				actual: samples,
			}));
		}
		self.run(packet, out, false)?;
		Ok(DecodeKind::Normal)
	}

	/// Conceal one missing frame from decoder state.
	pub fn conceal(&mut self, out: &mut [f32]) -> Result<DecodeKind> {
		Self::check_out(out)?;
		self.run(&[], out, false)?;
		Ok(DecodeKind::Concealed)
	}

	/// Recover the frame *before* `next_packet` from its in-band FEC data.
	/// `next_packet` must still be decoded normally afterwards; it is the
	/// caller's job not to feed a packet twice for any other reason.
	pub fn recover_previous(&mut self, next_packet: &[u8], out: &mut [f32]) -> Result<DecodeKind> {
		Self::check_out(out)?;
		self.run(next_packet, out, true)?;
		Ok(DecodeKind::FecRecovered)
	}

	/// Drop decoder history on a generation change.
	pub fn reset(&mut self) -> Result<()> {
		self.consecutive_errors = 0;
		Ok(self.decoder.reset_state()?)
	}

	fn run(&mut self, packet: &[u8], out: &mut [f32], fec: bool) -> Result<()> {
		match self.decoder.decode_float(packet, out, fec) {
			Ok(n) if n == FRAME_SAMPLES => {
				self.consecutive_errors = 0;
				Ok(())
			}
			Ok(n) => Err(self.fail(AudioError::BadFrameSize {
				expected: FRAME_SAMPLES,
				actual: n,
			})),
			Err(e) => Err(self.fail(e.into())),
		}
	}

	fn check_out(out: &[f32]) -> Result<()> {
		if out.len() == FRAME_SAMPLES {
			Ok(())
		} else {
			Err(AudioError::BufferTooSmall {
				needed: FRAME_SAMPLES,
				available: out.len(),
			})
		}
	}

	fn fail(&mut self, error: AudioError) -> AudioError {
		self.consecutive_errors += 1;
		error
	}
}
