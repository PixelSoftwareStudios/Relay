//! The echo-cancellation slot. Only the no-op implementation exists today.

use super::{DspError, Provides};

/// Echo canceller with an explicit render-reference input.
///
/// The render path feeds what is being played; the capture path removes its
/// echo from the microphone frame in place.
pub trait EchoCanceller: Send {
	/// What this canceller actually does; a no-op provides nothing.
	fn provides(&self) -> Provides;

	/// Supply the 10 ms of far-end audio that was (or will be) played.
	fn push_render(&mut self, render: &[f32]) -> Result<(), DspError>;

	/// Process one 10 ms capture frame in place.
	fn process_capture(&mut self, frame: &mut [f32]) -> Result<(), DspError>;

	fn reset(&mut self);
}

/// Occupies the slot without touching audio.
#[derive(Debug, Default)]
pub struct NoOpAec;

impl EchoCanceller for NoOpAec {
	fn provides(&self) -> Provides {
		Provides::NONE
	}

	fn push_render(&mut self, _render: &[f32]) -> Result<(), DspError> {
		Ok(())
	}

	fn process_capture(&mut self, _frame: &mut [f32]) -> Result<(), DspError> {
		Ok(())
	}

	fn reset(&mut self) {}
}
