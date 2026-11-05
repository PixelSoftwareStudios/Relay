//! Blinded per-channel discovery tags and their mDNS TXT form.
//!
//! `tag = keyed_blake3(tag_key(channel_id), epoch || instance_id)[..8]`. Members recompute it;
//! outsiders without the channel id cannot recognise, link or enumerate it, and the tag differs
//! per device and per hour. A tag is unauthenticated (anyone who knows the id can forge one),
//! so a match is a hint, never proof of membership.

use relay_core::channel::{ChannelId, ChannelName, Policy};

use super::ids::{ct_eq, random, tag_key};
use super::limits::{
	LISTED_ADVERTISED_MAX, TAG_BYTES, TAG_EPOCH_SECS, TAG_SLOTS_LARGE, TAG_SLOTS_SMALL,
};

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
pub fn blind(id: &ChannelId, epoch: u64, instance_id: &[u8; 16]) -> Tag {
	let mut hasher = blake3::Hasher::new_keyed(&tag_key(id));
	hasher.update(TAG_LABEL);
	hasher.update(&epoch.to_le_bytes());
	hasher.update(instance_id);
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
	) -> Self {
		let mut tags = Vec::new();
		let mut listed = Vec::new();
		for (id, name, policy) in channels {
			if tags.len() < TAG_SLOTS_LARGE {
				tags.push(blind(id, epoch, instance_id));
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
			out.push((TXT_TAGS.to_owned(), hex(self.tags.iter().flat_map(|t| t.0))));
		}
		for (i, listed) in self.listed.iter().enumerate() {
			let value = format!(
				"{}:{}",
				hex(listed.id.as_bytes().iter().copied()),
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
				parsed_tags.push(Tag(unhex::<TAG_BYTES>(std::str::from_utf8(chunk).ok()?)?));
			}
		}
		let listed = listed
			.iter()
			.take(LISTED_ADVERTISED_MAX)
			.filter(|v| v.len() <= TXT_VALUE_MAX)
			.filter_map(|v| {
				let (id, name) = v.split_once(':')?;
				Some(ListedChannel {
					id: ChannelId::from_bytes(unhex::<16>(id)?),
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

	/// Channels among `local` that this advert probably belongs to. The advert's epoch must be
	/// within one period of `now_epoch`; only then is each local channel tested against every
	/// tag for epochs `epoch-1..=epoch+1`, in constant time per comparison.
	pub fn recognise(
		&self,
		local: &[ChannelId],
		instance_id: &[u8; 16],
		now_epoch: u64,
	) -> Vec<ChannelId> {
		if self.epoch.abs_diff(now_epoch) > 1 {
			return Vec::new();
		}
		let mut found = Vec::new();
		for id in local {
			let mut hit = false;
			for epoch in self.epoch.saturating_sub(1)..=self.epoch.saturating_add(1) {
				let expected = blind(id, epoch, instance_id);
				for tag in &self.tags {
					hit |= ct_eq(&expected.0, &tag.0);
				}
			}
			if hit {
				found.push(*id);
			}
		}
		found
	}
}

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
	bytes.into_iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
	if text.len() != N * 2 || !text.is_ascii() {
		return None;
	}
	let mut out = [0u8; N];
	for (i, byte) in out.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
	}
	Some(out)
}
