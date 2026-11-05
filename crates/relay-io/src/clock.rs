//! Real clocks. The pure crates never read time; they are handed these.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, read fresh (0 if the clock is before the epoch).
pub fn unix_secs() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |d| d.as_secs())
}

/// Milliseconds since the Unix epoch, read fresh.
pub fn unix_millis() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The current wall time for stamping records.
pub fn system_time() -> SystemTime {
	SystemTime::now()
}

/// Wall plus monotonic time with the monotonic origin at construction.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
	origin: Instant,
}

impl SystemClock {
	pub fn new() -> Self {
		Self {
			origin: Instant::now(),
		}
	}

	/// Wall-clock milliseconds since the Unix epoch.
	pub fn wall_ms(&self) -> u64 {
		unix_millis()
	}

	/// Monotonic milliseconds since this clock was created.
	pub fn mono_ms(&self) -> u64 {
		u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
	}
}

impl Default for SystemClock {
	fn default() -> Self {
		Self::new()
	}
}

impl relay_messages::Clock for SystemClock {
	fn wall_ms(&self) -> u64 {
		SystemClock::wall_ms(self)
	}

	fn mono_ms(&self) -> u64 {
		SystemClock::mono_ms(self)
	}
}
