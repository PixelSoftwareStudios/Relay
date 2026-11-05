//! Time for the channel driver: wall-clock seconds for certificates and
//! epochs, and a monotonic duration for attempt limiting. Both advance
//! together and can be pushed forward in tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Shared clock. Cloning shares the test offset.
#[derive(Debug, Clone)]
pub struct Clock {
	origin: Instant,
	base_unix: u64,
	skew_secs: Arc<AtomicU64>,
}

impl Clock {
	/// The system clock.
	pub fn system() -> Self {
		let base_unix = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_secs());
		Self::starting_at(base_unix)
	}

	/// A clock whose wall time starts at `unix` (tests).
	pub fn starting_at(unix: u64) -> Self {
		Self {
			origin: Instant::now(),
			base_unix: unix,
			skew_secs: Arc::new(AtomicU64::new(0)),
		}
	}

	/// Monotonic time since the clock started. Never goes backwards.
	pub fn mono(&self) -> Duration {
		self.origin.elapsed() + Duration::from_secs(self.skew_secs.load(Ordering::Acquire))
	}

	/// Unix seconds, derived from the monotonic clock so a wall-clock step cannot
	/// reopen a lockout.
	pub fn unix(&self) -> u64 {
		self.base_unix + self.mono().as_secs()
	}

	/// Moves both clocks forward (tests: epoch rotation, expiry, lockout).
	pub fn advance(&self, by: Duration) {
		self.skew_secs.fetch_add(by.as_secs(), Ordering::AcqRel);
	}
}
