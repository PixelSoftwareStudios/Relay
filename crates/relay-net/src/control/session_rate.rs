//! Per-link flood guard for session messages, built on the `relay-messages` token bucket.
//!
//! Honest senders already obey `relay-messages`' 10 messages/s outbound limit and the engine's
//! store answers excess with `RateLimited` acks. This guard is the coarser backstop below that:
//! a peer exceeding it by a wide margin is hostile or broken, and the link is closed.

use std::time::Instant;

use relay_messages::limits::{MESSAGE_BURST, MESSAGE_RATE_PER_SEC};
use relay_messages::rate::TokenBucket;

/// Headroom over the per-sender limit so a legitimate burst plus acks never trips the guard.
const HEADROOM: u32 = 4;

/// Token bucket on a caller-supplied millisecond clock.
#[derive(Debug, Clone)]
pub struct SessionRateLimit {
	bucket: TokenBucket,
}

impl SessionRateLimit {
	/// Default guard: 4 x the per-sender limit.
	pub fn new(now_ms: u64) -> Self {
		Self::with_rate(
			u64::from(MESSAGE_RATE_PER_SEC * HEADROOM),
			u64::from(MESSAGE_BURST * HEADROOM),
			now_ms,
		)
	}

	pub fn with_rate(per_sec: u64, burst: u64, now_ms: u64) -> Self {
		Self {
			bucket: TokenBucket::new(per_sec, burst, now_ms),
		}
	}

	/// Takes one unit; false when the link exceeds its budget.
	pub fn allow(&mut self, now_ms: u64) -> bool {
		self.bucket.try_take(1, now_ms)
	}
}

/// Wall-clock adapter used by the receiver.
#[derive(Debug)]
pub(super) struct LinkClock(Instant);

impl LinkClock {
	pub(super) fn start() -> Self {
		Self(Instant::now())
	}

	pub(super) fn now_ms(&self) -> u64 {
		u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
	}
}
