//! Key changes the engine has seen and the user has not yet settled.
//!
//! A device that presents a different key under a trusted name is refused, and the refusal
//! is remembered here: offending key -> the pinned key record it would replace. Shells show
//! the contact as "key changed" from this, and "forget" and "replace" accept the offending
//! key because that is the only key the contact card knows. Nothing here is trusted: an
//! entry only says what was refused. It lives in memory and is rebuilt by the next attempt.

use relay_io::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use relay_core::PeerId;
use relay_net::identity::Fingerprint;

/// Most distinct offending keys remembered; the oldest goes first.
const MAX_ENTRIES: usize = 32;

/// One refused key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChange {
	/// The pinned key it would replace.
	pub previous: Fingerprint,
	/// The name stored with the pinned key.
	pub previous_name: String,
	/// The name the offending device presented (untrusted).
	pub name: String,
	/// When it was last refused.
	pub at: SystemTime,
}

/// What clients may know about a refused key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChangeInfo {
	/// The pinned key digest.
	pub previous: PeerId,
	/// The name stored with the pinned key.
	pub previous_name: String,
	/// When the new key was last refused.
	pub at: SystemTime,
}

/// The shared map, cloned into the handle and the manager.
#[derive(Clone, Default)]
pub struct KeyChanges(Arc<Mutex<HashMap<Fingerprint, KeyChange>>>);

impl KeyChanges {
	/// Remembers `offending`. Any other offending key for the same pinned key is dropped:
	/// only the newest counts.
	pub(super) fn note(&self, offending: Fingerprint, change: KeyChange) {
		let mut map = self.0.lock_unpoisoned();
		map.retain(|key, other| other.previous != change.previous || *key == offending);
		map.insert(offending, change);
		while map.len() > MAX_ENTRIES {
			let Some(oldest) = map.iter().min_by_key(|(_, c)| c.at).map(|(k, _)| *k) else {
				break;
			};
			map.remove(&oldest);
		}
	}

	/// The entry for an offending key.
	pub fn get(&self, offending: &Fingerprint) -> Option<KeyChange> {
		self.0.lock_unpoisoned().get(offending).cloned()
	}

	/// Every entry.
	pub fn all(&self) -> Vec<(Fingerprint, KeyChange)> {
		let map = self.0.lock_unpoisoned();
		map.iter().map(|(k, c)| (*k, c.clone())).collect()
	}

	/// Drops the entry for an offending key.
	pub(super) fn remove(&self, offending: &Fingerprint) {
		self.0.lock_unpoisoned().remove(offending);
	}

	/// Drops every entry that names `previous`; returns the offending keys that went.
	pub(super) fn clear_previous(&self, previous: &Fingerprint) -> Vec<Fingerprint> {
		let mut map = self.0.lock_unpoisoned();
		let gone: Vec<Fingerprint> = map
			.iter()
			.filter(|(_, c)| &c.previous == previous)
			.map(|(k, _)| *k)
			.collect();
		for key in &gone {
			map.remove(key);
		}
		gone
	}
}
