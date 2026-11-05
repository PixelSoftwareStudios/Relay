use std::collections::BTreeMap;

use super::{CHANNELS_MAX, ChannelId, ChannelName, Policy};

/// The joined-channel limit was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("joined channel limit reached")]
pub struct LimitReached;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
	Owner,
	Admin,
	Member,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEntry {
	pub name: ChannelName,
	pub policy: Policy,
	pub role: Role,
}

/// The channels this device has joined. Bounded; deterministic iteration order.
#[derive(Debug, Clone, Default)]
pub struct Memberships {
	entries: BTreeMap<ChannelId, ChannelEntry>,
}

impl Memberships {
	pub fn new() -> Self {
		Self::default()
	}

	/// Adds or updates a channel. A new channel beyond the limit is refused.
	pub fn insert(&mut self, id: ChannelId, entry: ChannelEntry) -> Result<(), LimitReached> {
		if !self.entries.contains_key(&id) && self.entries.len() >= CHANNELS_MAX {
			return Err(LimitReached);
		}
		self.entries.insert(id, entry);
		Ok(())
	}

	pub fn remove(&mut self, id: &ChannelId) -> bool {
		self.entries.remove(id).is_some()
	}

	pub fn contains(&self, id: &ChannelId) -> bool {
		self.entries.contains_key(id)
	}

	pub fn get(&self, id: &ChannelId) -> Option<&ChannelEntry> {
		self.entries.get(id)
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub fn iter(&self) -> impl Iterator<Item = (&ChannelId, &ChannelEntry)> {
		self.entries.iter()
	}
}
