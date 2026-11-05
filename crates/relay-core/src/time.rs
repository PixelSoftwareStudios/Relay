//! Explicit monotonic time. The domain never reads a clock itself.

use std::time::Duration;

/// A monotonic instant expressed in milliseconds since an arbitrary origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MonoTime(u64);

impl MonoTime {
	/// The origin.
	pub const ZERO: Self = Self(0);

	/// Builds an instant from milliseconds.
	pub const fn from_millis(millis: u64) -> Self {
		Self(millis)
	}

	/// Milliseconds since the origin.
	pub const fn as_millis(self) -> u64 {
		self.0
	}

	/// Adds a duration, saturating at the far future.
	pub fn saturating_add(self, delta: Duration) -> Self {
		let millis = u64::try_from(delta.as_millis()).unwrap_or(u64::MAX);
		Self(self.0.saturating_add(millis))
	}

	/// Time elapsed since `earlier`, or zero if `earlier` is in the future.
	pub fn saturating_since(self, earlier: Self) -> Duration {
		Duration::from_millis(self.0.saturating_sub(earlier.0))
	}
}

/// Source of monotonic time, implemented by the runtime and by test fakes.
pub trait Clock {
	/// The current monotonic instant.
	fn now(&self) -> MonoTime;
}
