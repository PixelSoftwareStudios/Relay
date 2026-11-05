//! Carries partial input across callbacks until a full frame exists.

use crate::format::{BlockTiming, FRAME_SAMPLES};

/// Accumulates 48 kHz mono samples into fixed 10 ms frames.
pub struct FrameAssembler {
	buf: [f32; FRAME_SAMPLES],
	len: usize,
	next_index: u64,
	generation: u64,
}

impl Default for FrameAssembler {
	fn default() -> Self {
		Self {
			buf: [0.0; FRAME_SAMPLES],
			len: 0,
			next_index: 0,
			generation: 0,
		}
	}
}

impl FrameAssembler {
	/// Drop partial data and start a new generation at `first_index`.
	pub fn restart(&mut self, first_index: u64) {
		self.len = 0;
		self.next_index = first_index;
		self.generation += 1;
	}

	pub fn generation(&self) -> u64 {
		self.generation
	}

	/// Samples held back waiting for the rest of a frame.
	pub fn pending(&self) -> usize {
		self.len
	}

	/// Append samples, emitting each completed frame with its timing.
	pub fn push(&mut self, mut samples: &[f32], mut emit: impl FnMut(&[f32], BlockTiming)) {
		while !samples.is_empty() {
			let take = (FRAME_SAMPLES - self.len).min(samples.len());
			self.buf[self.len..self.len + take].copy_from_slice(&samples[..take]);
			self.len += take;
			samples = &samples[take..];
			if self.len == FRAME_SAMPLES {
				let timing = BlockTiming {
					generation: self.generation,
					first_sample: self.next_index,
				};
				emit(&self.buf, timing);
				self.next_index += FRAME_SAMPLES as u64;
				self.len = 0;
			}
		}
	}
}
