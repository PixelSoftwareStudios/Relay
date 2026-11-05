//! Pacing for an accept loop that hits errors such as fd exhaustion (`EMFILE`/`ENFILE`).

use std::time::Duration;

const FIRST: Duration = Duration::from_millis(20);
const MAX: Duration = Duration::from_secs(2);

/// Exponential delay between failed `accept` calls: 20 ms doubling up to 2 s. Sleep for the
/// returned delay after each error and call [`Self::reset`] after a success, so a persistent
/// error costs almost no CPU instead of spinning on the pending connection.
#[derive(Debug, Default)]
pub struct AcceptBackoff {
	failures: u32,
}

impl AcceptBackoff {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn on_error(&mut self) -> Duration {
		let delay = (FIRST * (1u32 << self.failures.min(7))).min(MAX);
		self.failures = self.failures.saturating_add(1);
		delay
	}

	pub fn reset(&mut self) {
		self.failures = 0;
	}
}
