use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use super::{DiscoveredPeer, DiscoveryError};
use crate::limits::{DISCOVERY_ENTRIES_MAX, DISCOVERY_PER_SOURCE_MAX};

#[derive(Debug)]
struct Entry {
	peer: DiscoveredPeer,
	seen: Instant,
}

/// Bounded table of currently visible candidates, keyed by service instance (not by name).
/// Visibility is separate from trust: an entry here implies nothing about pairing.
///
/// Announcing many instance names cannot silence discovery: a source address holds at most
/// [`DISCOVERY_PER_SOURCE_MAX`] entries (its own oldest is replaced), and when the table is
/// full the oldest entry of the busiest source is evicted instead of refusing a newcomer.
/// Evicted names are available from [`Self::take_evicted`] so the owner can drop state tied to
/// them. [`Self::expire`] ages entries out without relying on a goodbye packet.
#[derive(Debug, Default)]
pub struct DiscoveryTable {
	entries: HashMap<String, Entry>,
	evicted: Vec<String>,
}

fn source(peer: &DiscoveredPeer) -> Option<IpAddr> {
	peer.addresses.first().map(|a| a.ip())
}

impl DiscoveryTable {
	pub fn new() -> Self {
		Self::default()
	}

	/// Inserts or refreshes a candidate. Fails only for a record without any address.
	pub fn upsert(&mut self, peer: DiscoveredPeer) -> Result<(), DiscoveryError> {
		let src = source(&peer).ok_or(DiscoveryError::Advertisement("record has no address"))?;
		if !self.entries.contains_key(&peer.fullname) {
			let held = self.of_source(src);
			if held.len() >= DISCOVERY_PER_SOURCE_MAX {
				self.evict_oldest_of(&held);
			}
			if self.entries.len() >= DISCOVERY_ENTRIES_MAX {
				let busiest = self.busiest_source();
				let held = self.of_source(busiest);
				self.evict_oldest_of(&held);
			}
		}
		self.entries.insert(
			peer.fullname.clone(),
			Entry {
				peer,
				seen: Instant::now(),
			},
		);
		Ok(())
	}

	fn of_source(&self, src: IpAddr) -> Vec<String> {
		self.entries
			.iter()
			.filter(|(_, e)| source(&e.peer) == Some(src))
			.map(|(k, _)| k.clone())
			.collect()
	}

	fn busiest_source(&self) -> IpAddr {
		let mut counts: HashMap<IpAddr, usize> = HashMap::new();
		for e in self.entries.values() {
			if let Some(s) = source(&e.peer) {
				*counts.entry(s).or_default() += 1;
			}
		}
		counts
			.into_iter()
			.max_by_key(|(_, n)| *n)
			.map(|(s, _)| s)
			.unwrap_or(IpAddr::from([0, 0, 0, 0]))
	}

	fn evict_oldest_of(&mut self, names: &[String]) {
		let oldest = names
			.iter()
			.filter_map(|n| self.entries.get(n).map(|e| (n, e.seen)))
			.min_by_key(|(_, seen)| *seen)
			.map(|(n, _)| n.clone());
		if let Some(name) = oldest {
			self.entries.remove(&name);
			self.evicted.push(name);
		}
	}

	/// Names evicted since the last call.
	pub fn take_evicted(&mut self) -> Vec<String> {
		std::mem::take(&mut self.evicted)
	}

	/// Removes entries not refreshed within `ttl`; returns their names.
	pub fn expire(&mut self, ttl: Duration) -> Vec<String> {
		let now = Instant::now();
		let stale: Vec<String> = self
			.entries
			.iter()
			.filter(|(_, e)| now.saturating_duration_since(e.seen) >= ttl)
			.map(|(k, _)| k.clone())
			.collect();
		for name in &stale {
			self.entries.remove(name);
		}
		stale
	}

	pub fn remove(&mut self, fullname: &str) -> Option<DiscoveredPeer> {
		self.entries.remove(fullname).map(|e| e.peer)
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub fn iter(&self) -> impl Iterator<Item = &DiscoveredPeer> {
		self.entries.values().map(|e| &e.peer)
	}
}
