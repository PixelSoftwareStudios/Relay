use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::time::{Clock, MonoTime};

/// A manually advanced clock. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct FakeClock {
	millis: Arc<AtomicU64>,
}

impl FakeClock {
	/// Starts at the origin.
	pub fn new() -> Self {
		Self::default()
	}

	/// Moves time forward.
	pub fn advance(&self, delta: Duration) -> MonoTime {
		let step = u64::try_from(delta.as_millis()).unwrap_or(u64::MAX);
		let previous = self.millis.fetch_add(step, Ordering::SeqCst);
		MonoTime::from_millis(previous.saturating_add(step))
	}

	/// Moves time forward by whole milliseconds.
	pub fn advance_millis(&self, millis: u64) -> MonoTime {
		self.advance(Duration::from_millis(millis))
	}
}

impl Clock for FakeClock {
	fn now(&self) -> MonoTime {
		MonoTime::from_millis(self.millis.load(Ordering::SeqCst))
	}
}
