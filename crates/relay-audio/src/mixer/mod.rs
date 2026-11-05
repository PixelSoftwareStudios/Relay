//! Per-call gain, master gain and a safety limiter over preallocated scratch.

mod limiter;

pub use limiter::Limiter;

use crate::error::{AudioError, Result};
use crate::format::MAX_CALLS;

/// Highest accepted linear gain (about +6 dB).
pub const MAX_GAIN: f32 = 2.0;
/// Unity gain, the safe default.
pub const DEFAULT_GAIN: f32 = 1.0;

/// Identifies one call's input in the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallSlot(u8);

#[derive(Debug, Clone, Copy)]
struct Slot {
	active: bool,
	target: f32,
	current: f32,
}

const EMPTY_SLOT: Slot = Slot {
	active: false,
	target: DEFAULT_GAIN,
	current: DEFAULT_GAIN,
};

/// Mixes one quantum at a time: `begin`, `add` each call, `finish`.
pub struct Mixer {
	acc: Vec<f32>,
	slots: [Slot; MAX_CALLS],
	master_target: f32,
	master_current: f32,
	limiter: Limiter,
	rejected_samples: u64,
}

impl Mixer {
	/// `frame_len` is the quantum size; allocates the accumulator once.
	pub fn new(frame_len: usize) -> Self {
		Self {
			acc: vec![0.0; frame_len],
			slots: [EMPTY_SLOT; MAX_CALLS],
			master_target: DEFAULT_GAIN,
			master_current: DEFAULT_GAIN,
			limiter: Limiter::default(),
			rejected_samples: 0,
		}
	}

	pub fn frame_len(&self) -> usize {
		self.acc.len()
	}

	/// Non-finite input samples replaced by silence so far.
	pub fn rejected_samples(&self) -> u64 {
		self.rejected_samples
	}

	pub fn limiter(&self) -> &Limiter {
		&self.limiter
	}

	/// Claim a free slot; `None` when all [`MAX_CALLS`] are in use.
	pub fn open_slot(&mut self) -> Option<CallSlot> {
		let index = self.slots.iter().position(|s| !s.active)?;
		self.slots[index] = Slot {
			active: true,
			..EMPTY_SLOT
		};
		Some(CallSlot(index as u8))
	}

	pub fn close_slot(&mut self, slot: CallSlot) {
		self.slots[usize::from(slot.0)] = EMPTY_SLOT;
	}

	/// Set a call's gain, capped to `0..=MAX_GAIN`. Non-finite values are refused.
	pub fn set_call_gain(&mut self, slot: CallSlot, gain: f32) -> Result<()> {
		self.slots[usize::from(slot.0)].target = checked_gain(gain)?;
		Ok(())
	}

	pub fn set_master_gain(&mut self, gain: f32) -> Result<()> {
		self.master_target = checked_gain(gain)?;
		Ok(())
	}

	/// Clear the accumulator for a new quantum.
	pub fn begin(&mut self) {
		self.acc.fill(0.0);
	}

	/// Add one call's decoded audio. Gain changes ramp across the quantum.
	pub fn add(&mut self, slot: CallSlot, input: &[f32]) -> Result<()> {
		if input.len() != self.acc.len() {
			return Err(AudioError::BadFrameSize {
				expected: self.acc.len(),
				actual: input.len(),
			});
		}
		let state = &mut self.slots[usize::from(slot.0)];
		if !state.active {
			return Err(AudioError::InvalidArgument("slot is not open"));
		}
		let step = (state.target - state.current) / input.len() as f32;
		let mut gain = state.current;
		for (acc, &sample) in self.acc.iter_mut().zip(input) {
			gain += step;
			if sample.is_finite() {
				*acc += sample * gain;
			} else {
				self.rejected_samples += 1;
			}
		}
		state.current = state.target;
		Ok(())
	}

	/// Apply master gain and the limiter, writing the quantum to `out`.
	pub fn finish(&mut self, out: &mut [f32]) -> Result<()> {
		if out.len() != self.acc.len() {
			return Err(AudioError::BufferTooSmall {
				needed: self.acc.len(),
				available: out.len(),
			});
		}
		let step = (self.master_target - self.master_current) / out.len() as f32;
		let mut gain = self.master_current;
		for (dst, &sum) in out.iter_mut().zip(&self.acc) {
			gain += step;
			*dst = self.limiter.process(sum * gain);
		}
		self.master_current = self.master_target;
		Ok(())
	}
}

fn checked_gain(gain: f32) -> Result<f32> {
	if gain.is_finite() {
		Ok(gain.clamp(0.0, MAX_GAIN))
	} else {
		Err(AudioError::InvalidArgument("gain must be finite"))
	}
}
