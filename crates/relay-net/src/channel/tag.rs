//! Blinded per-channel discovery tags and their mDNS TXT form.
//!
//! `tag = keyed_blake3(tag_key(channel_id), epoch || instance_id || fingerprint)[..8]`. Members recompute it;
//! outsiders without the channel id cannot recognise, link or enumerate it, and the tag differs
//! per device and per hour. A tag is unauthenticated (anyone who knows the id can forge one),
//! so a match is a hint, never proof of membership.

use relay_core::channel::{ChannelId, ChannelName, Policy};

use super::ids::{ct_eq, random, tag_key};
use super::limits::{
	LISTED_ADVERTISED_MAX, TAG_BYTES, TAG_EPOCH_SECS, TAG_SLOTS_LARGE, TAG_SLOTS_SMALL,
};
use crate::fixed_hex;
use crate::identity::Fingerprint;

const TAG_LABEL: &[u8] = b"tag";
const PSEUDONYM_DOMAIN: &str = "relay-net 2026 hidden instance id v1";
const TXT_EPOCH: &str = "ce";
const TXT_TAGS: &str = "ct";
const TXT_LISTED: &str = "chl";
/// Listed entries use keys `chl0`, `chl1`, ... because TXT keys are unique.
const TXT_VALUE_MAX: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag([u8; TAG_BYTES]);

impl Tag {
	pub const fn as_bytes(&self) -> &[u8; TAG_BYTES] {
		&self.0
	}
}

pub const fn epoch_of(unix_secs: u64) -> u64 {
	unix_secs / TAG_EPOCH_SECS
}

/// The tag a member of `id` advertises from `instance_id` during `epoch`.
///
/// The advertiser's device fingerprint is part of the input, so a member can only recognise a
/// tag for a fingerprint it already knows. A copied advertisement then yields the *advertiser's*
/// fingerprint, and dialling the copier's address with that fingerprint pinned fails before our
/// static key is revealed.
pub fn blind(id: &ChannelId, epoch: u64, instance_id: &[u8; 16], fingerprint: &Fingerprint) -> Tag {
	let mut hasher = blake3::Hasher::new_keyed(&tag_key(id));
	hasher.update(TAG_LABEL);
	hasher.update(&epoch.to_le_bytes());
	hasher.update(instance_id);
	hasher.update(fingerprint.as_bytes());
	let mut tag = [0u8; TAG_BYTES];
	tag.copy_from_slice(&hasher.finalize().as_bytes()[..TAG_BYTES]);
	Tag(tag)
}

/// Rotating pseudonymous mDNS instance id for hidden mode.
pub fn hidden_instance_id(instance_secret: &[u8; 32], epoch: u64) -> [u8; 16] {
	let mut hasher = blake3::Hasher::new_derive_key(PSEUDONYM_DOMAIN);
	hasher.update(instance_secret);
	hasher.update(&epoch.to_le_bytes());
	let mut id = [0u8; 16];
	id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
	id
}

/// A channel whose name is advertised in clear, by the owner's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedChannel {
	pub id: ChannelId,
	pub name: ChannelName,
}

/// What a device publishes about channels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelAdvert {
	pub epoch: u64,
	pub tags: Vec<Tag>,
	pub listed: Vec<ListedChannel>,
}

impl ChannelAdvert {
	/// Builds the advert for the given joined channels. Unlisted channels contribute only a
	/// blinded tag; the tag list is padded with random decoys to 4 or 8 slots and shuffled, so
	/// the channel count is only coarsely visible.
	pub fn build<'a>(
		channels: impl IntoIterator<Item = (&'a ChannelId, &'a ChannelName, &'a Policy)>,
		epoch: u64,
		instance_id: &[u8; 16],
		fingerprint: &Fingerprint,
	) -> Self {
		let mut tags = Vec::new();
		let mut listed = Vec::new();
		for (id, name, policy) in channels {
			if tags.len() < TAG_SLOTS_LARGE {
				tags.push(blind(id, epoch, instance_id, fingerprint));
			}
			if policy.listed && listed.len() < LISTED_ADVERTISED_MAX {
				listed.push(ListedChannel {
					id: *id,
					name: name.clone(),
				});
			}
		}
		let slots = if tags.len() <= TAG_SLOTS_SMALL {
			TAG_SLOTS_SMALL
		} else {
			TAG_SLOTS_LARGE
		};
		while tags.len() < slots {
			tags.push(Tag(random::<TAG_BYTES>()));
		}
		for i in (1..tags.len()).rev() {
			let j = usize::from(random::<1>()[0]) % (i + 1);
			tags.swap(i, j);
		}
		Self {
			epoch,
			tags,
			listed,
		}
	}

	/// TXT entries to add to the mDNS record.
	pub fn to_txt(&self) -> Vec<(String, String)> {
		let mut out = vec![(TXT_EPOCH.to_owned(), self.epoch.to_string())];
		if !self.tags.is_empty() {
			out.push((
				TXT_TAGS.to_owned(),
				hex::encode(self.tags.iter().flat_map(|t| t.0).collect::<Vec<u8>>()),
			));
		}
		for (i, listed) in self.listed.iter().enumerate() {
			let value = format!(
				"{}:{}",
				hex::encode(listed.id.as_bytes()),
				listed.name.as_str()
			);
			if value.len() <= TXT_VALUE_MAX {
				out.push((format!("{TXT_LISTED}{i}"), value));
			}
		}
		out
	}

	/// Parses the raw TXT map of a discovered peer (`DiscoveredPeer::txt`).
	pub fn from_map(txt: &std::collections::BTreeMap<String, String>) -> Option<Self> {
		let listed: Vec<&str> = (0..LISTED_ADVERTISED_MAX)
			.filter_map(|i| txt.get(&format!("{TXT_LISTED}{i}")).map(String::as_str))
			.collect();
		Self::from_txt(
			txt.get(TXT_EPOCH).map(String::as_str),
			txt.get(TXT_TAGS).map(String::as_str),
			&listed,
		)
	}

	/// Parses untrusted TXT values. `listed` holds every `chl` value. Malformed pieces are
	/// dropped, never trusted, and the result is bounded.
	pub fn from_txt(epoch: Option<&str>, tags: Option<&str>, listed: &[&str]) -> Option<Self> {
		let epoch: u64 = epoch?.parse().ok()?;
		let mut parsed_tags = Vec::new();
		if let Some(text) = tags {
			if text.len() > TAG_SLOTS_LARGE * TAG_BYTES * 2 || text.len() % (TAG_BYTES * 2) != 0 {
				return None;
			}
			for chunk in text.as_bytes().chunks(TAG_BYTES * 2) {
				parsed_tags.push(Tag(fixed_hex::decode::<TAG_BYTES>(
					std::str::from_utf8(chunk).ok()?,
				)?));
			}
		}
		let listed = listed
			.iter()
			.take(LISTED_ADVERTISED_MAX)
			.filter(|v| v.len() <= TXT_VALUE_MAX)
			.filter_map(|v| {
				let (id, name) = v.split_once(':')?;
				Some(ListedChannel {
					id: ChannelId::from_bytes(fixed_hex::decode::<16>(id)?),
					name: ChannelName::new(name).ok()?,
				})
			})
			.collect();
		Some(Self {
			epoch,
			tags: parsed_tags,
			listed,
		})
	}

	/// Members this advert probably belongs to: for each channel in `local` and each already
	/// known (`known`) device fingerprint, whether a tag matches. The advert's epoch must be
	/// within one period of `now_epoch`; then epochs `epoch-1..=epoch+1` are tested, in constant
	/// time per comparison. The result carries the fingerprint the advertiser must present, to be
	/// passed to the handshake as the expected key. A device not in `known` is never recognised,
	/// so a replayed tag can only ever lead to a pinned (and therefore failing) dial.
	pub fn recognise(
		&self,
		local: &[ChannelId],
		instance_id: &[u8; 16],
		known: &[Fingerprint],
		now_epoch: u64,
	) -> Vec<Recognised> {
		if self.epoch.abs_diff(now_epoch) > 1 {
			return Vec::new();
		}
		let mut found = Vec::new();
		for id in local {
			for fingerprint in known {
				let mut hit = false;
				for epoch in self.epoch.saturating_sub(1)..=self.epoch.saturating_add(1) {
					let expected = blind(id, epoch, instance_id, fingerprint);
					for tag in &self.tags {
						hit |= ct_eq(&expected.0, &tag.0);
					}
				}
				if hit {
					found.push(Recognised {
						channel: *id,
						fingerprint: *fingerprint,
					});
				}
			}
		}
		found
	}
}

/// A recognised advertisement: the channel it hints at and the device it claims to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recognised {
	pub channel: ChannelId,
	pub fingerprint: Fingerprint,
}
