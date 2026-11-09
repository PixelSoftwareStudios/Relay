//! Listed channels other devices advertise right now.
//!
//! A listed channel's name and id travel in clear in the mDNS record by the creator's choice.
//! This module remembers, per name, which id was seen, which devices (by advertised
//! instance id) said it and when, so the engine can show "channels on your network" and
//! resolve a join by name.
//!
//! Rules:
//!
//! - An entry lives while some device advertised it within [`LISTED_TTL`]; a device that
//!   falls silent stops counting after that long, and an entry with no device left is gone.
//! - A name seen with two different ids is *ambiguous* (a spoof or a clash). It is shown as
//!   unavailable and joining by that name is refused: it is never bound to the first id.
//!   It stays ambiguous while any device still advertises it.
//! - A name equal to a channel this device already belongs to is treated the same way, so a
//!   copy of your own channel's name cannot be offered as a join.
//! - A channel this device already belongs to is never offered.
//! - At most [`LISTED_SEEN_MAX`] names and [`DEVICES_PER_NAME`] devices per name are kept.
//! - Receiving is independent of the privacy mode: a hidden device lists what it hears.

use std::collections::BTreeMap;
use std::time::Duration;

use relay_api::pb::NearbyChannel;
use relay_core::channel::ChannelId;

use super::state::State;

/// Listed channel names remembered from adverts.
pub(super) const LISTED_SEEN_MAX: usize = 32;
/// Advertising devices counted per name.
pub(super) const DEVICES_PER_NAME: usize = 64;
/// A listed channel nobody has advertised for this long is no longer nearby.
pub const LISTED_TTL: Duration = Duration::from_secs(120);

/// One listed name: the id it stands for (`None` once two ids claimed it) and who said so.
pub(super) struct Listed {
	pub id: Option<ChannelId>,
	/// Advertising device (instance id) to the mono time of its last advert.
	pub sources: BTreeMap<[u8; 16], Duration>,
}

impl Listed {
	fn live(&self, mono: Duration) -> usize {
		self.sources.values().filter(|at| fresh(**at, mono)).count()
	}
}

fn fresh(at: Duration, mono: Duration) -> bool {
	mono.saturating_sub(at) < LISTED_TTL
}

/// What a name can be joined as.
pub(super) enum Resolved {
	Id(ChannelId),
	Ambiguous,
	Unknown,
}

impl State {
	/// Records one listed channel heard from `device`; true when what is shown changed.
	pub fn note_listed(
		&mut self,
		name: &str,
		id: ChannelId,
		device: [u8; 16],
		mono: Duration,
	) -> bool {
		if self.memberships.contains(&id) {
			return false;
		}
		let mut changed = self.prune_listed(mono);
		if !self.listed_seen.contains_key(name) && self.listed_seen.len() >= LISTED_SEEN_MAX {
			return changed;
		}
		let entry = self
			.listed_seen
			.entry(name.to_owned())
			.or_insert_with(|| Listed {
				id: Some(id),
				sources: BTreeMap::new(),
			});
		if entry.sources.is_empty() {
			changed = true;
		}
		if entry.id.is_some_and(|first| first != id) {
			entry.id = None;
			changed = true;
		}
		if let Some(at) = entry.sources.get_mut(&device) {
			// A device that was silent past the TTL counts again.
			changed |= !fresh(*at, mono);
			*at = mono;
		} else if entry.sources.len() < DEVICES_PER_NAME {
			entry.sources.insert(device, mono);
			changed = true;
		}
		changed
	}

	/// Drops devices and names not heard within the TTL; true when anything went.
	pub fn prune_listed(&mut self, mono: Duration) -> bool {
		let before = self.listed_seen.len();
		let mut changed = false;
		for entry in self.listed_seen.values_mut() {
			let had = entry.sources.len();
			entry.sources.retain(|_, at| fresh(*at, mono));
			changed |= entry.sources.len() != had;
		}
		self.listed_seen
			.retain(|_, entry| !entry.sources.is_empty());
		changed || before != self.listed_seen.len()
	}

	/// The id a listed name may be joined as.
	pub fn resolve_listed(&self, name: &str, mono: Duration) -> Resolved {
		let Some(entry) = self.listed_seen.get(name) else {
			return Resolved::Unknown;
		};
		if entry.live(mono) == 0 {
			return Resolved::Unknown;
		}
		match entry.id {
			Some(id) if !self.memberships.contains(&id) && !self.joined_name(name) => {
				Resolved::Id(id)
			}
			Some(id) if self.memberships.contains(&id) => Resolved::Unknown,
			_ => Resolved::Ambiguous,
		}
	}

	fn joined_name(&self, name: &str) -> bool {
		self.records.iter().any(|r| r.name.as_str() == name)
	}

	/// What the API shows: fresh names this device is not in, sorted by name.
	pub fn nearby(&self, mono: Duration) -> Vec<NearbyChannel> {
		self.listed_seen
			.iter()
			.filter_map(|(name, entry)| {
				let devices = entry.live(mono);
				if devices == 0 {
					return None;
				}
				let ambiguous = match self.resolve_listed(name, mono) {
					Resolved::Id(_) => false,
					Resolved::Ambiguous => true,
					Resolved::Unknown => return None,
				};
				Some(NearbyChannel {
					name: name.clone(),
					devices: u32::try_from(devices).unwrap_or(u32::MAX),
					ambiguous,
				})
			})
			.collect()
	}
}
