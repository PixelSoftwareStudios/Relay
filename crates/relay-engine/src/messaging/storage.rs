//! Live `limits.*` for the messaging actor: transcript budgets and transfer concurrency.

use super::hub::Hub;
use super::sink::MessagingLimits;

/// What the transcripts hold now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessagingUsage {
	/// Bytes held by ended transcripts (counted against the retained budget).
	pub retained_bytes: u64,
	/// Bytes of the largest transcript of a call still running.
	pub largest_live_bytes: u64,
}

impl Hub {
	/// Puts new limits into force. Ended transcripts over the new retained budget are removed
	/// at once, oldest first. A transcript over a smaller session budget keeps what it has and
	/// accepts nothing new until it shrinks. A transfer limit applies to the next transfer.
	pub fn set_limits(&mut self, limits: MessagingLimits) {
		self.env.limits = limits;
		self.history.set_session_limit(limits.session_bytes);
		for old in self.history.set_budget(limits.retained_bytes) {
			self.forget(old, "retained history budget");
		}
	}

	/// The memory the transcripts use.
	pub fn usage(&self) -> MessagingUsage {
		MessagingUsage {
			retained_bytes: self.history.retained_bytes(),
			largest_live_bytes: self.history.largest_live_bytes(),
		}
	}
}
