//! Sliding replay window. `check` is read-only; `commit` is called only after the record has
//! authenticated and passed structural validation, so forged counters cannot move it.

use crate::limits::MEDIA_REPLAY_WINDOW;

const WORDS: usize = (MEDIA_REPLAY_WINDOW / 64) as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayVerdict {
	Fresh,
	Duplicate,
	TooOld,
}

/// Bit `d` of the bitmap records whether sequence `highest - d` has been accepted.
#[derive(Debug, Clone)]
pub struct ReplayWindow {
	highest: Option<u64>,
	bits: [u64; WORDS],
}

impl Default for ReplayWindow {
	fn default() -> Self {
		Self::new()
	}
}

impl ReplayWindow {
	pub const fn new() -> Self {
		Self {
			highest: None,
			bits: [0; WORDS],
		}
	}

	pub fn check(&self, sequence: u64) -> ReplayVerdict {
		let Some(highest) = self.highest else {
			return ReplayVerdict::Fresh;
		};
		if sequence > highest {
			return ReplayVerdict::Fresh;
		}
		let distance = highest - sequence;
		if distance >= MEDIA_REPLAY_WINDOW {
			return ReplayVerdict::TooOld;
		}
		if self.bit(distance) {
			ReplayVerdict::Duplicate
		} else {
			ReplayVerdict::Fresh
		}
	}

	/// Records an accepted sequence. Call only after authentication succeeded.
	pub fn commit(&mut self, sequence: u64) {
		match self.highest {
			Some(highest) if sequence <= highest => {
				let distance = highest - sequence;
				if distance < MEDIA_REPLAY_WINDOW {
					self.bits[(distance / 64) as usize] |= 1 << (distance % 64);
				}
			}
			Some(highest) => {
				self.shift(sequence - highest);
				self.highest = Some(sequence);
				self.bits[0] |= 1;
			}
			None => {
				self.highest = Some(sequence);
				self.bits[0] = 1;
			}
		}
	}

	fn bit(&self, distance: u64) -> bool {
		self.bits[(distance / 64) as usize] >> (distance % 64) & 1 == 1
	}

	fn shift(&mut self, by: u64) {
		if by >= MEDIA_REPLAY_WINDOW {
			self.bits = [0; WORDS];
			return;
		}
		let words = (by / 64) as usize;
		let bits = (by % 64) as u32;
		for i in (0..WORDS).rev() {
			let mut value = if i >= words {
				self.bits[i - words] << bits
			} else {
				0
			};
			if bits > 0 && i > words {
				value |= self.bits[i - words - 1] >> (64 - bits);
			}
			self.bits[i] = value;
		}
	}
}
