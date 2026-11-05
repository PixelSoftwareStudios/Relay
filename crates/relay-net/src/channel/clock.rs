//! Wall-clock source for certificate, invite, delegation, revocation and tag-epoch decisions.
//!
//! These expiries are calendar facts, so they use the wall clock (`SystemTime`), sampled for
//! every decision. A process-start sample plus a monotonic offset would be wrong after a
//! suspend (monotonic clocks may stop while the machine sleeps) and after a late time sync.
//! The monotonic clock stays the right tool for lockout pacing, which only measures elapsed
//! time.
//!
//! Policy: expiry is **strict** (no tolerance in the accepting direction), so a slow clock can
//! only make us stricter after it is corrected. [`ClampedClock`] never reports a value below
//! one it already reported, so a clock stepped backwards (or an NTP rollback on a hostile LAN)
//! cannot resurrect an expired certificate or invite. Tag epochs tolerate one period either way
//! ([`SKEW_TOLERANCE_SECS`] documents the intended bound for operator-facing checks).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Clock error between devices that discovery and proofs are designed to absorb: tag epochs
/// accept one hour either side.
pub const SKEW_TOLERANCE_SECS: u64 = super::limits::TAG_EPOCH_SECS;

/// Seconds since the Unix epoch, read fresh on every call.
pub trait UnixClock: Send + Sync {
	fn now(&self) -> u64;
}

/// The system wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl UnixClock for SystemClock {
	fn now(&self) -> u64 {
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_secs())
	}
}

/// Wraps a clock so its readings never decrease.
#[derive(Debug, Default)]
pub struct ClampedClock<C> {
	inner: C,
	highest: AtomicU64,
}

impl<C: UnixClock> ClampedClock<C> {
	pub fn new(inner: C) -> Self {
		Self {
			inner,
			highest: AtomicU64::new(0),
		}
	}
}

impl<C: UnixClock> UnixClock for ClampedClock<C> {
	fn now(&self) -> u64 {
		let reading = self.inner.now();
		self.highest
			.fetch_max(reading, Ordering::AcqRel)
			.max(reading)
	}
}

/// A settable clock for tests.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
	pub fn new(now: u64) -> Self {
		Self(AtomicU64::new(now))
	}

	pub fn set(&self, now: u64) {
		self.0.store(now, Ordering::Release);
	}

	pub fn advance(&self, secs: u64) {
		self.0.fetch_add(secs, Ordering::AcqRel);
	}
}

impl UnixClock for ManualClock {
	fn now(&self) -> u64 {
		self.0.load(Ordering::Acquire)
	}
}
