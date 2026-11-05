//! Ack and bounded-retry bookkeeping for the application layer.

use std::collections::BTreeMap;

use crate::limits::{ACK_TIMEOUT_MS, MAX_RETRIES};

#[derive(Clone, Copy, Debug)]
struct Pending {
	attempts: u32,
	deadline_ms: u64,
}

/// What the owner must do after [`Outbox::poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryAction {
	/// Send the message with this sequence again.
	Resend(u64),
	/// Retries exhausted: mark the message failed.
	GiveUp(u64),
}

/// Unacked local messages.
#[derive(Debug, Default)]
pub struct Outbox {
	pending: BTreeMap<u64, Pending>,
}

impl Outbox {
	/// Starts the ack timer for `seq`.
	pub fn sent(&mut self, seq: u64, now_ms: u64) {
		self.pending.insert(
			seq,
			Pending {
				attempts: 0,
				deadline_ms: now_ms + ACK_TIMEOUT_MS,
			},
		);
	}

	/// Stops tracking `seq`; true when it was pending.
	pub fn acked(&mut self, seq: u64) -> bool {
		self.pending.remove(&seq).is_some()
	}

	/// Pending count.
	pub fn len(&self) -> usize {
		self.pending.len()
	}

	/// True when nothing is pending.
	pub fn is_empty(&self) -> bool {
		self.pending.is_empty()
	}

	/// Earliest deadline, so the engine can schedule a wakeup.
	pub fn next_deadline(&self) -> Option<u64> {
		self.pending.values().map(|p| p.deadline_ms).min()
	}

	/// Due retries (timeout doubles each attempt) and give-ups, in sequence order.
	pub fn poll(&mut self, now_ms: u64) -> Vec<RetryAction> {
		let mut out = Vec::new();
		let mut done = Vec::new();
		for (&seq, p) in &mut self.pending {
			if now_ms < p.deadline_ms {
				continue;
			}
			if p.attempts >= MAX_RETRIES {
				done.push(seq);
				out.push(RetryAction::GiveUp(seq));
			} else {
				p.attempts += 1;
				p.deadline_ms = now_ms + (ACK_TIMEOUT_MS << p.attempts);
				out.push(RetryAction::Resend(seq));
			}
		}
		for s in done {
			self.pending.remove(&s);
		}
		out
	}

	/// Drops everything (session end).
	pub fn clear(&mut self) {
		self.pending.clear();
	}
}
