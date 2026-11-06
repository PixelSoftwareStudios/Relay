//! Re-verifying a contact whose key changed: the user compares the new key's fingerprint on
//! the other device, and only an exact match replaces the pinned key.

use std::net::SocketAddr;

use relay_core::PeerId;
use relay_net::identity::Fingerprint;

use super::handle::{Command, PeerHandle};
use super::key_changes::KeyChangeInfo;
use super::manager::Actor;
use super::{fingerprint, peer_id};
use crate::service::ports::{PortError, ReplacedPeer};

/// Digits of a full fingerprint.
const FULL_DIGITS: usize = 64;

/// The hex digits of `text` with spaces, dashes and colons ignored and case folded, or `None`
/// when anything else is in it.
pub fn normalize_fingerprint(text: &str) -> Option<String> {
	let digits: String = text
		.chars()
		.filter(|c| !matches!(c, ' ' | '-' | ':' | '\t'))
		.map(|c| c.to_ascii_lowercase())
		.collect();
	(digits.chars().all(|c| c.is_ascii_hexdigit())).then_some(digits)
}

impl PeerHandle {
	/// What is known about a refused key, if `peer` is one.
	pub fn key_change(&self, peer: PeerId) -> Option<KeyChangeInfo> {
		let change = self.key_changes.get(&fingerprint(peer))?;
		Some(KeyChangeInfo {
			previous: peer_id(&change.previous),
			previous_name: change.previous_name,
			at: change.at,
		})
	}

	/// Replaces the pinned key behind the refused key `offending` by that key, as verified.
	/// This needs all of: a remembered key change for `offending`, the user's statement that
	/// they compared, and the typed or scanned fingerprint equal to the full fingerprint of
	/// `offending` (comparing proves the other device shows the same key). Otherwise nothing
	/// changes. The name and the favorite place carry over; the manager then dials the device
	/// at `via` and the addresses it already knew.
	pub fn replace_pinned_key(
		&self,
		offending: PeerId,
		expected_fingerprint: &str,
		verified: bool,
		via: Vec<SocketAddr>,
	) -> Result<ReplacedPeer, PortError> {
		let key = fingerprint(offending);
		let Some(change) = self.key_changes.get(&key) else {
			return Err(PortError::NotFound(
				"that device has no key change to verify".to_owned(),
			));
		};
		if !verified {
			return Err(PortError::Invalid(
				"compare the fingerprint on the other device first".to_owned(),
			));
		}
		let typed = normalize_fingerprint(expected_fingerprint)
			.filter(|digits| digits.len() == FULL_DIGITS)
			.ok_or_else(|| PortError::Invalid("enter the full fingerprint".to_owned()))?;
		if typed != hex::encode(key.as_bytes()) {
			return Err(PortError::Invalid(
				"that fingerprint is not the new key of this device".to_owned(),
			));
		}
		let record = self
			.trust
			.lock()
			.replace(&change.previous, &key)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		self.key_changes.clear_previous(&change.previous);
		self.key_changes.remove(&key);
		self.try_send(Command::Replaced {
			old: change.previous,
			new: key,
			via,
		})
		.map_err(|e| PortError::Unavailable(e.to_string()))?;
		Ok(ReplacedPeer {
			id: offending,
			previous: peer_id(&change.previous),
			name: record.display_name,
		})
	}
}

impl Actor {
	/// The pinned key is gone and its successor is trusted: cut the old link, carry the
	/// addresses over and dial the new key.
	pub(super) async fn replaced(
		&mut self,
		old: Fingerprint,
		new: Fingerprint,
		via: Vec<SocketAddr>,
	) {
		self.wanted.remove(&old);
		self.pending.remove(&old);
		if let Some(id) = self.canonical.get(&old).copied() {
			self.drop_link(id).await;
		}
		for addr in self
			.addresses
			.remove(&old)
			.unwrap_or_default()
			.into_iter()
			.chain(via)
		{
			self.remember_address(new, addr);
		}
		self.wanted.entry(new).or_default();
		self.try_dial(new);
	}
}
