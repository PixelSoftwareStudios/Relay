//! Token bucket on an injected monotonic clock.

/// A token bucket measured in arbitrary units (messages, bytes).
#[derive(Clone, Debug)]
pub struct TokenBucket {
	capacity: u64,
	per_sec: u64,
	tokens_milli: u128,
	last_ms: u64,
}

impl TokenBucket {
	/// Starts full.
	pub fn new(per_sec: u64, burst: u64, now_ms: u64) -> Self {
		Self {
			capacity: burst,
			per_sec,
			tokens_milli: u128::from(burst) * 1000,
			last_ms: now_ms,
		}
	}

	fn refill(&mut self, now_ms: u64) {
		let dt = now_ms.saturating_sub(self.last_ms);
		self.last_ms = self.last_ms.max(now_ms);
		let add = u128::from(dt) * u128::from(self.per_sec);
		self.tokens_milli = (self.tokens_milli + add).min(u128::from(self.capacity) * 1000);
	}

	/// Takes `n` units if available.
	pub fn try_take(&mut self, n: u64, now_ms: u64) -> bool {
		self.refill(now_ms);
		let need = u128::from(n) * 1000;
		if self.tokens_milli >= need {
			self.tokens_milli -= need;
			true
		} else {
			false
		}
	}

	/// Whole units available now.
	pub fn available(&mut self, now_ms: u64) -> u64 {
		self.refill(now_ms);
		(self.tokens_milli / 1000) as u64
	}
}
