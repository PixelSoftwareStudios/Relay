//! The call-to-session table: which peer owns which call, bounded per peer.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use relay_core::{CallId, PeerId, SessionId};

const SESSIONS_MAX: usize = 256;
/// Calls one peer may have in the table; its oldest go first.
const SESSIONS_PER_PEER: usize = 32;
/// Inbound invitations one peer may send per window.
const INVITES_PER_WINDOW: u32 = 16;
/// Media-ready messages one peer may send per window.
const READY_PER_WINDOW: u32 = 16;
const WINDOW: Duration = Duration::from_secs(10);
/// Peers whose rates are remembered.
const RATES_MAX: usize = 256;

/// Fixed-window counter.
#[derive(Debug, Clone, Copy)]
struct Rate {
	began: Instant,
	invites: u32,
	ready: u32,
}

/// Call-to-session table. Every entry records the peer the call was exchanged with;
/// later messages about the call count only from that peer. Capacity is per peer,
/// and a full table evicts from the peer holding the most, so one peer cannot push
/// out the calls other peers have.
#[derive(Debug, Default)]
pub struct SessionTable {
	map: HashMap<CallId, (SessionId, PeerId)>,
	order: VecDeque<CallId>,
	rates: HashMap<PeerId, Rate>,
}

impl SessionTable {
	/// Records a call. False if another peer already owns that call id.
	pub(super) fn remember(&mut self, call: CallId, session: SessionId, peer: PeerId) -> bool {
		match self.map.get_mut(&call) {
			Some((_, owner)) if *owner != peer => return false,
			Some(entry) => {
				entry.0 = session;
				return true;
			}
			None => {}
		}
		if self.count_of(peer) >= SESSIONS_PER_PEER {
			self.evict_oldest_of(peer);
		}
		if self.map.len() >= SESSIONS_MAX {
			let busiest = self.busiest();
			self.evict_oldest_of(busiest);
		}
		self.map.insert(call, (session, peer));
		self.order.push_back(call);
		true
	}

	fn count_of(&self, peer: PeerId) -> usize {
		self.map.values().filter(|(_, p)| *p == peer).count()
	}

	fn busiest(&self) -> PeerId {
		let mut counts: HashMap<PeerId, usize> = HashMap::new();
		for (_, peer) in self.map.values() {
			*counts.entry(*peer).or_default() += 1;
		}
		counts
			.into_iter()
			.max_by_key(|(_, n)| *n)
			.map(|(peer, _)| peer)
			.unwrap_or_else(|| PeerId::from_bytes([0; 32]))
	}

	fn evict_oldest_of(&mut self, peer: PeerId) {
		let oldest = self
			.order
			.iter()
			.position(|call| self.map.get(call).is_some_and(|(_, p)| *p == peer));
		if let Some(call) = oldest.and_then(|i| self.order.remove(i)) {
			self.map.remove(&call);
		}
	}

	/// The session recorded for a call.
	pub fn get(&self, call: CallId) -> Option<SessionId> {
		self.map.get(&call).map(|(session, _)| *session)
	}

	/// The session of a call, only when `peer` owns it.
	pub fn get_for(&self, call: CallId, peer: PeerId) -> Option<SessionId> {
		self.map
			.get(&call)
			.filter(|(_, owner)| *owner == peer)
			.map(|(session, _)| *session)
	}

	/// The peer a call was exchanged with.
	pub fn peer_of(&self, call: CallId) -> Option<PeerId> {
		self.map.get(&call).map(|(_, peer)| *peer)
	}

	fn spend(&mut self, peer: PeerId, ready: bool) -> bool {
		let now = Instant::now();
		if self.rates.len() >= RATES_MAX && !self.rates.contains_key(&peer) {
			self.rates
				.retain(|_, r| now.saturating_duration_since(r.began) < WINDOW);
		}
		let rate = self.rates.entry(peer).or_insert(Rate {
			began: now,
			invites: 0,
			ready: 0,
		});
		if now.saturating_duration_since(rate.began) >= WINDOW {
			*rate = Rate {
				began: now,
				invites: 0,
				ready: 0,
			};
		}
		let (used, cap) = if ready {
			(&mut rate.ready, READY_PER_WINDOW)
		} else {
			(&mut rate.invites, INVITES_PER_WINDOW)
		};
		*used = used.saturating_add(1);
		*used <= cap
	}

	/// Counts an inbound invitation from `peer`; false once it is over its rate.
	pub(super) fn admit_invite(&mut self, peer: PeerId) -> bool {
		self.spend(peer, false)
	}

	/// Counts an inbound media-ready from `peer`; false once it is over its rate.
	pub(super) fn admit_ready(&mut self, peer: PeerId) -> bool {
		self.spend(peer, true)
	}
}
