//! Global retained-history budget across sessions.

use std::collections::HashMap;
use std::sync::Arc;

use crate::env::Clock;
use crate::limits::GLOBAL_RETAINED_BYTES;
use crate::message::{PeerFingerprint, Permissions, SessionId};
use crate::store::{SessionInfo, SessionStore};

/// Owns every session transcript in the daemon.
///
/// Active sessions are never evicted. Ended transcripts stay readable until the user clears
/// them, a new session with the same peer starts, the global budget or the ended-call count evicts them (oldest ended
/// first), or the daemon exits.
pub struct HistoryManager {
	sessions: HashMap<SessionId, SessionStore>,
	/// Ended sessions, oldest first.
	ended: Vec<SessionId>,
	budget: u64,
	/// Most ended transcripts kept.
	max_ended: usize,
	clock: Arc<dyn Clock>,
}

impl HistoryManager {
	/// Standard 256 MiB retained budget.
	pub fn new(clock: Arc<dyn Clock>) -> Self {
		Self::with_budget(clock, GLOBAL_RETAINED_BYTES)
	}

	/// Custom retained budget.
	pub fn with_budget(clock: Arc<dyn Clock>, budget: u64) -> Self {
		Self {
			sessions: HashMap::new(),
			ended: Vec::new(),
			budget,
			max_ended: usize::MAX,
			clock,
		}
	}

	/// Starts a session. Any ended transcript with the same peer is dropped; their ids are returned.
	pub fn start_session(&mut self, info: SessionInfo, perms: Permissions) -> Vec<SessionId> {
		self.start_with(SessionStore::new(info, perms, self.clock.clone()))
	}

	/// Starts a session from a prepared store (custom per-session limit).
	pub fn start_with(&mut self, store: SessionStore) -> Vec<SessionId> {
		let peer: PeerFingerprint = store.info().peer.fingerprint;
		let replaced: Vec<SessionId> = self
			.ended
			.iter()
			.copied()
			.filter(|id| {
				self.sessions
					.get(id)
					.is_some_and(|s| s.info().peer.fingerprint == peer)
			})
			.collect();
		for id in &replaced {
			self.remove(*id);
		}
		self.sessions.insert(store.info().id, store);
		replaced
	}

	/// Ends a session and enforces the retained budget and call count. Returns evicted session ids.
	pub fn end_session(&mut self, id: SessionId) -> Vec<SessionId> {
		let Some(s) = self.sessions.get_mut(&id) else {
			return Vec::new();
		};
		if s.is_ended() {
			return Vec::new();
		}
		s.end();
		self.ended.push(id);
		self.enforce()
	}

	fn enforce(&mut self) -> Vec<SessionId> {
		let mut evicted = Vec::new();
		while self.ended.len() > self.max_ended {
			let oldest = self.ended[0];
			self.remove(oldest);
			evicted.push(oldest);
		}
		while self.retained_bytes() > self.budget && !self.ended.is_empty() {
			let oldest = self.ended[0];
			self.remove(oldest);
			evicted.push(oldest);
		}
		evicted
	}

	fn remove(&mut self, id: SessionId) {
		self.sessions.remove(&id);
		self.ended.retain(|e| *e != id);
	}

	/// Changes the retained budget and evicts the oldest ended transcripts that no longer fit.
	/// Returns the evicted ids.
	pub fn set_budget(&mut self, budget: u64) -> Vec<SessionId> {
		self.budget = budget;
		self.enforce()
	}

	/// Changes how many ended transcripts are kept (`0` keeps none) and evicts the oldest
	/// surplus. Returns the evicted ids.
	pub fn set_max_ended(&mut self, max: usize) -> Vec<SessionId> {
		self.max_ended = max;
		self.enforce()
	}

	/// Changes the per-session budget of every transcript, live or ended. Nothing is deleted;
	/// a transcript over its new budget accepts nothing new until it shrinks.
	pub fn set_session_limit(&mut self, limit: u64) {
		for session in self.sessions.values_mut() {
			session.set_limit_bytes(limit);
		}
	}

	/// The largest transcript of a session that has not ended, in bytes.
	pub fn largest_live_bytes(&self) -> u64 {
		self.sessions
			.values()
			.filter(|s| !s.is_ended())
			.map(SessionStore::used_bytes)
			.max()
			.unwrap_or(0)
	}

	/// Bytes held by ended transcripts.
	pub fn retained_bytes(&self) -> u64 {
		self.ended
			.iter()
			.filter_map(|id| self.sessions.get(id))
			.map(SessionStore::used_bytes)
			.sum()
	}

	/// A session.
	pub fn get(&self, id: SessionId) -> Option<&SessionStore> {
		self.sessions.get(&id)
	}

	/// A session, mutably.
	pub fn get_mut(&mut self, id: SessionId) -> Option<&mut SessionStore> {
		self.sessions.get_mut(&id)
	}

	/// Ended session ids, oldest first.
	pub fn ended_ids(&self) -> &[SessionId] {
		&self.ended
	}

	/// User clears one transcript. An active session keeps running with an empty transcript;
	/// an ended one is removed entirely.
	pub fn clear(&mut self, id: SessionId) {
		match self.sessions.get_mut(&id) {
			Some(s) if s.is_ended() => self.remove(id),
			Some(s) => s.clear(),
			None => {}
		}
	}

	/// Removes every ended transcript.
	pub fn clear_ended(&mut self) {
		for id in std::mem::take(&mut self.ended) {
			self.sessions.remove(&id);
		}
	}

	/// Re-checks the budget after a transcript grew (a late preview, for example).
	pub fn rebalance(&mut self) -> Vec<SessionId> {
		self.enforce()
	}
}
