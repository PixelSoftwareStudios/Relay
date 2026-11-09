//! Sliding bitmap window used for duplicate and replay detection.

/// Outcome of checking a number against the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayVerdict {
	/// Newer than anything seen or inside the window and unseen.
	Fresh,
	/// Already seen.
	Duplicate,
	/// Older than the window can remember.
	TooOld,
}

/// Window of `WORDS * 64` numbers ending at the highest accepted number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayWindow<const WORDS: usize> {
	bits: [u64; WORDS],
	highest: Option<u64>,
}

impl<const WORDS: usize> Default for ReplayWindow<WORDS> {
	fn default() -> Self {
		Self::new()
	}
}

impl<const WORDS: usize> ReplayWindow<WORDS> {
	/// Number of distinct numbers remembered.
	pub const SIZE: u64 = WORDS as u64 * 64;

	/// Creates an empty window; `WORDS` must be nonzero.
	///
	/// ```compile_fail
	/// let _ = relay_protocol::seq::ReplayWindow::<0>::new();
	/// ```
	pub const fn new() -> Self {
		const { assert!(WORDS > 0, "replay window requires at least one word") };
		Self {
			bits: [0; WORDS],
			highest: None,
		}
	}

	/// Classifies `n` without changing state.
	pub fn check(&self, n: u64) -> ReplayVerdict {
		let Some(highest) = self.highest else {
			return ReplayVerdict::Fresh;
		};
		if n > highest {
			return ReplayVerdict::Fresh;
		}
		let age = highest - n;
		if age >= Self::SIZE {
			ReplayVerdict::TooOld
		} else if self.bit(age) {
			ReplayVerdict::Duplicate
		} else {
			ReplayVerdict::Fresh
		}
	}

	/// Records `n`. Call only after authentication succeeded.
	pub fn accept(&mut self, n: u64) {
		match self.highest {
			None => {
				self.highest = Some(n);
				self.bits = [0; WORDS];
				self.set(0);
			}
			Some(highest) if n > highest => {
				self.shift(n - highest);
				self.highest = Some(n);
				self.set(0);
			}
			Some(highest) => {
				let age = highest - n;
				if age < Self::SIZE {
					self.set(age);
				}
			}
		}
	}

	/// Forgets everything.
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	fn bit(&self, age: u64) -> bool {
		self.bits[(age / 64) as usize] >> (age % 64) & 1 == 1
	}

	fn set(&mut self, age: u64) {
		self.bits[(age / 64) as usize] |= 1 << (age % 64);
	}

	fn shift(&mut self, by: u64) {
		if by >= Self::SIZE {
			self.bits = [0; WORDS];
			return;
		}
		let words = (by / 64) as usize;
		let bits = (by % 64) as u32;
		for i in (0..WORDS).rev() {
			let mut value = 0;
			if i >= words {
				value = self.bits[i - words] << bits;
				if bits > 0 && i > words {
					value |= self.bits[i - words - 1] >> (64 - bits);
				}
			}
			self.bits[i] = value;
		}
	}
}
