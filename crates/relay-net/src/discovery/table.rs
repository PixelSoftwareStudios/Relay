use std::collections::HashMap;

use super::{DiscoveredPeer, DiscoveryError};
use crate::limits::DISCOVERY_ENTRIES_MAX;

/// Bounded table of currently visible candidates, keyed by service instance (not by name).
/// Visibility is separate from trust: an entry here implies nothing about pairing.
#[derive(Debug, Default)]
pub struct DiscoveryTable {
	entries: HashMap<String, DiscoveredPeer>,
}

impl DiscoveryTable {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn upsert(&mut self, peer: DiscoveredPeer) -> Result<(), DiscoveryError> {
		if !self.entries.contains_key(&peer.fullname) && self.entries.len() >= DISCOVERY_ENTRIES_MAX
		{
			return Err(DiscoveryError::TableFull);
		}
		self.entries.insert(peer.fullname.clone(), peer);
		Ok(())
	}

	pub fn remove(&mut self, fullname: &str) -> Option<DiscoveredPeer> {
		self.entries.remove(fullname)
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub fn iter(&self) -> impl Iterator<Item = &DiscoveredPeer> {
		self.entries.values()
	}
}
