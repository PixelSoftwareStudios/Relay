//! Reconnect delay: exponential growth with equal jitter, capped.

use std::time::Duration;

/// Backoff bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffConfig {
	/// Delay ceiling of the first attempt.
	pub base: Duration,
	/// Delay ceiling of every later attempt.
	pub max: Duration,
}

impl Default for BackoffConfig {
	fn default() -> Self {
		Self {
			base: Duration::from_millis(250),
			max: Duration::from_secs(30),
		}
	}
}

impl BackoffConfig {
	/// The delay before retry number `attempt` (0 = first retry).
	///
	/// `jitter` in `0.0..=1.0` picks a point in the upper half of the window,
	/// so two peers that failed together do not retry in lockstep.
	pub fn delay(&self, attempt: u32, jitter: f64) -> Duration {
		let window = self
			.base
			.saturating_mul(1u32.checked_shl(attempt.min(20)).unwrap_or(u32::MAX))
			.min(self.max);
		let jitter = if jitter.is_finite() {
			jitter.clamp(0.0, 1.0)
		} else {
			1.0
		};
		window.mul_f64(0.5 + 0.5 * jitter)
	}
}
