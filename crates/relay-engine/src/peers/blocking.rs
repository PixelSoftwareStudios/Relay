//! The block list: keys whose links are refused without a word.
//!
//! A blocked device is identified by its key digest, so it cannot come back under another
//! name. Its links are closed as soon as the handshake has shown the key: nothing is
//! announced to the user and no trust prompt is raised. The list is bounded and, in the
//! daemon, persisted in `relay-config`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use relay_config::blocklist::{Blocklist, MAX_BLOCKED};
use relay_io::LockExt;
use relay_net::identity::Fingerprint;

use crate::service::ports::PortError;

/// Where the block list lives between runs.
pub trait BlockStore: Send + Sync {
	/// Every stored entry.
	fn load(&self) -> Vec<(Fingerprint, String)>;
	/// Stores or renames an entry.
	fn add(&self, peer: &Fingerprint, name: &str) -> Result<(), String>;
	/// Deletes an entry.
	fn remove(&self, peer: &Fingerprint) -> Result<(), String>;
}

/// The `blocked` table of the configuration database.
pub struct ConfigBlocks(pub Blocklist);

impl BlockStore for ConfigBlocks {
	fn load(&self) -> Vec<(Fingerprint, String)> {
		self.0
			.list()
			.unwrap_or_default()
			.into_iter()
			.map(|d| (Fingerprint::from_bytes(d.fingerprint), d.display_name))
			.collect()
	}

	fn add(&self, peer: &Fingerprint, name: &str) -> Result<(), String> {
		self.0.add(peer.as_bytes(), name).map_err(|e| e.to_string())
	}

	fn remove(&self, peer: &Fingerprint) -> Result<(), String> {
		self.0
			.remove(peer.as_bytes())
			.map(drop)
			.map_err(|e| e.to_string())
	}
}

/// The shared, bounded set of blocked keys.
#[derive(Clone, Default)]
pub struct Blocks {
	entries: Arc<Mutex<HashMap<Fingerprint, String>>>,
	store: Option<Arc<dyn BlockStore>>,
}

impl Blocks {
	/// A list kept in memory only (tests, hosts without storage).
	pub fn in_memory() -> Self {
		Self::default()
	}

	/// A list loaded from, and written through to, `store`.
	pub fn persistent(store: Arc<dyn BlockStore>) -> Self {
		let entries = store.load().into_iter().collect();
		Self {
			entries: Arc::new(Mutex::new(entries)),
			store: Some(store),
		}
	}

	/// True when `peer` is blocked.
	pub fn contains(&self, peer: &Fingerprint) -> bool {
		self.entries.lock_unpoisoned().contains_key(peer)
	}

	/// The name a blocked device had when it was blocked.
	pub fn name_of(&self, peer: &Fingerprint) -> Option<String> {
		self.entries.lock_unpoisoned().get(peer).cloned()
	}

	/// Every blocked key with its name.
	pub fn entries(&self) -> Vec<(Fingerprint, String)> {
		self.entries
			.lock_unpoisoned()
			.iter()
			.map(|(k, v)| (*k, v.clone()))
			.collect()
	}

	/// Blocks `peer`. Refused, with nothing changed, when the list is full or cannot be
	/// stored.
	pub fn block(&self, peer: Fingerprint, name: &str) -> Result<(), PortError> {
		let mut entries = self.entries.lock_unpoisoned();
		if !entries.contains_key(&peer) && entries.len() >= MAX_BLOCKED {
			return Err(PortError::Invalid(format!(
				"blocked limit reached: {MAX_BLOCKED}"
			)));
		}
		if let Some(store) = &self.store {
			store
				.add(&peer, name)
				.map_err(|e| PortError::Unavailable(format!("could not save the device: {e}")))?;
		}
		entries.insert(peer, name.to_owned());
		Ok(())
	}

	/// Unblocks `peer`; true when it was blocked.
	pub fn unblock(&self, peer: &Fingerprint) -> Result<bool, PortError> {
		let mut entries = self.entries.lock_unpoisoned();
		if !entries.contains_key(peer) {
			return Ok(false);
		}
		if let Some(store) = &self.store {
			store
				.remove(peer)
				.map_err(|e| PortError::Unavailable(format!("could not save the device: {e}")))?;
		}
		entries.remove(peer);
		Ok(true)
	}
}

impl std::fmt::Debug for Blocks {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Blocks({})", self.entries.lock_unpoisoned().len())
	}
}
