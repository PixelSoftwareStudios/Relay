//! Time for the channel driver: wall-clock seconds for certificates and
//! epochs, and a monotonic duration for attempt limiting.
//!
//! Calendar decisions read the wall clock on every call through a clamp that
//! never reports a smaller value than one already seen, so suspend does not
//! stretch an expiry and a clock stepped backwards cannot revive one. Only the
//! limiter's pacing uses the monotonic clock. Both can be pushed forward in
//! tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use relay_net::channel::clock::{ClampedClock, ManualClock, SystemClock, UnixClock};

/// Shared clock. Cloning shares the test offset.
#[derive(Clone)]
pub struct Clock {
	origin: Instant,
	wall: Arc<dyn UnixClock>,
	skew_secs: Arc<AtomicU64>,
}

impl std::fmt::Debug for Clock {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Clock").field("unix", &self.unix()).finish()
	}
}

impl Clock {
	/// The system clock.
	pub fn system() -> Self {
		Self::with(Arc::new(ClampedClock::new(SystemClock)))
	}

	/// A clock whose wall time is frozen at `unix` until advanced (tests).
	pub fn starting_at(unix: u64) -> Self {
		Self::with(Arc::new(ManualClock::new(unix)))
	}

	fn with(wall: Arc<dyn UnixClock>) -> Self {
		Self {
			origin: Instant::now(),
			wall,
			skew_secs: Arc::new(AtomicU64::new(0)),
		}
	}

	/// Monotonic time since the clock started. Never goes backwards.
	pub fn mono(&self) -> Duration {
		self.origin.elapsed() + self.skew()
	}

	/// Unix seconds from the wall clock, never below an earlier reading.
	pub fn unix(&self) -> u64 {
		self.wall.now() + self.skew().as_secs()
	}

	/// Moves both clocks forward (tests: epoch rotation, expiry, lockout).
	pub fn advance(&self, by: Duration) {
		self.skew_secs.fetch_add(by.as_secs(), Ordering::AcqRel);
	}

	fn skew(&self) -> Duration {
		Duration::from_secs(self.skew_secs.load(Ordering::Acquire))
	}
}
