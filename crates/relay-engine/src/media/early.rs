//! Negotiation that arrived before the local side was ready: bounded per peer
//! and aged out, so an unauthenticated-looking flood of `Ready` messages for
//! calls that never exist cannot pin memory or evict real entries forever.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use relay_core::{CallId, PeerId};

use super::state::RemoteReady;

/// How long an early message is kept.
pub const EARLY_TTL: Duration = Duration::from_secs(10);
/// Early entries one peer may hold.
pub const EARLY_PER_PEER: usize = 4;
/// Early entries overall.
pub const EARLY_TOTAL: usize = 16;

struct Entry {
	peer: PeerId,
	epoch: [u8; 16],
	ready: RemoteReady,
	at: Instant,
}

/// Remote `Ready` messages for calls not yet prepared locally.
#[derive(Default)]
pub struct EarlyReady(HashMap<CallId, Entry>);

impl EarlyReady {
	fn prune(&mut self, now: Instant) {
		self.0.retain(|_, e| now.duration_since(e.at) < EARLY_TTL);
	}

	/// Keeps the message unless the peer or the table is at its cap.
	pub fn insert(
		&mut self,
		call: CallId,
		peer: PeerId,
		epoch: [u8; 16],
		ready: RemoteReady,
		now: Instant,
	) -> bool {
		self.prune(now);
		let from_peer = self.0.values().filter(|e| e.peer == peer).count();
		if !self.0.contains_key(&call)
			&& (from_peer >= EARLY_PER_PEER || self.0.len() >= EARLY_TOTAL)
		{
			return false;
		}
		self.0.insert(
			call,
			Entry {
				peer,
				epoch,
				ready,
				at: now,
			},
		);
		true
	}

	/// Removes and returns a fresh entry for `call`.
	pub fn take(&mut self, call: &CallId, now: Instant) -> Option<([u8; 16], RemoteReady)> {
		self.prune(now);
		self.0.remove(call).map(|e| (e.epoch, e.ready))
	}

	/// Drops the entry without returning it.
	pub fn discard(&mut self, call: &CallId) {
		self.0.remove(call);
	}

	/// Entries currently held.
	pub fn len(&self) -> usize {
		self.0.len()
	}

	/// Whether nothing is held.
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}
