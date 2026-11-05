//! Stream health published by error callbacks and consumed by control code.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use super::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
	Running,
	/// The device disappeared.
	DeviceLost,
	/// The configuration changed or the stream must be rebuilt.
	Invalidated,
	BackendError,
}

impl StreamState {
	fn code(self) -> u8 {
		match self {
			Self::Running => 0,
			Self::DeviceLost => 1,
			Self::Invalidated => 2,
			Self::BackendError => 3,
		}
	}

	fn from_code(code: u8) -> Self {
		match code {
			0 => Self::Running,
			1 => Self::DeviceLost,
			2 => Self::Invalidated,
			_ => Self::BackendError,
		}
	}
}

#[derive(Debug, Default)]
struct HealthInner {
	state: AtomicU8,
	xruns: AtomicU64,
	realtime_denied: AtomicBool,
}

/// Cheap clonable health cell; writes are lock-free and allocation-free so
/// error callbacks stay cheap. Recovery work happens elsewhere.
#[derive(Debug, Clone, Default)]
pub struct StreamHealth(Arc<HealthInner>);

impl StreamHealth {
	pub fn state(&self) -> StreamState {
		StreamState::from_code(self.0.state.load(Ordering::Acquire))
	}

	/// Record a failure; a lost device is never downgraded by later errors.
	pub fn report(&self, state: StreamState) {
		let _ = self.0.state.fetch_max(state.code(), Ordering::AcqRel);
	}

	pub fn report_xrun(&self) {
		self.0.xruns.fetch_add(1, Ordering::Relaxed);
	}

	/// The backend could not promote its audio thread to real time. Informational: audio keeps
	/// running, so this never changes [`StreamState`].
	pub fn report_realtime_denied(&self) {
		self.0.realtime_denied.store(true, Ordering::Relaxed);
	}

	pub fn realtime_denied(&self) -> bool {
		self.0.realtime_denied.load(Ordering::Relaxed)
	}

	pub fn xruns(&self) -> u64 {
		self.0.xruns.load(Ordering::Relaxed)
	}
}

/// What the control layer should do after a stream failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
	Continue,
	/// Pause the affected path, tell the user and wait for an explicit choice.
	PauseAndPrompt,
}

/// Recovery policy. There is deliberately no automatic fallback: a lost
/// microphone must not silently become the laptop microphone, and a lost
/// render device must not silently redirect remote audio.
pub fn recovery_for(_direction: Direction, state: StreamState) -> Recovery {
	match state {
		StreamState::Running => Recovery::Continue,
		_ => Recovery::PauseAndPrompt,
	}
}
