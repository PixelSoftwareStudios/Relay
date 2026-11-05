//! Bound on authenticated-but-untrusted links. Minting identities is free, so a completed
//! handshake proves nothing about the sender's resources: keep few, per source, and evict the
//! oldest instead of growing.

use std::collections::VecDeque;
use std::net::IpAddr;

use crate::limits::{UNTRUSTED_LINKS_MAX, UNTRUSTED_LINKS_PER_SOURCE};

/// Tracks which untrusted links exist so the owner can close the evicted one. `K` identifies a
/// link (for example its fingerprint or an internal id).
#[derive(Debug)]
pub struct UntrustedLinks<K> {
	order: VecDeque<(K, IpAddr)>,
	max: usize,
	per_source: usize,
}

impl<K: PartialEq + Copy> UntrustedLinks<K> {
	pub fn new() -> Self {
		Self::with_limits(UNTRUSTED_LINKS_MAX, UNTRUSTED_LINKS_PER_SOURCE)
	}

	pub fn with_limits(max: usize, per_source: usize) -> Self {
		Self {
			order: VecDeque::new(),
			max,
			per_source,
		}
	}

	/// Registers a new untrusted link. Returns the links to close to stay within bounds: a
	/// source over its share loses its own oldest link, and a full table loses the oldest
	/// overall. The new link itself is never refused, so an attacker only evicts its own.
	pub fn admit(&mut self, key: K, source: IpAddr) -> Vec<K> {
		let mut evict = Vec::new();
		self.order.retain(|(k, _)| *k != key);
		while self.order.iter().filter(|(_, s)| *s == source).count() >= self.per_source {
			let Some(i) = self.order.iter().position(|(_, s)| *s == source) else {
				break;
			};
			evict.extend(self.order.remove(i).map(|(k, _)| k));
		}
		while self.order.len() >= self.max {
			evict.extend(self.order.pop_front().map(|(k, _)| k));
		}
		self.order.push_back((key, source));
		evict
	}

	/// The link was trusted, rejected or closed.
	pub fn release(&mut self, key: &K) {
		self.order.retain(|(k, _)| k != key);
	}

	pub fn len(&self) -> usize {
		self.order.len()
	}

	pub fn is_empty(&self) -> bool {
		self.order.is_empty()
	}
}

impl<K: PartialEq + Copy> Default for UntrustedLinks<K> {
	fn default() -> Self {
		Self::new()
	}
}
