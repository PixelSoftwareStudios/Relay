//! Persistent trust behind relay-net's [`TrustStore`] trait.
//!
//! The handshake never writes here. Only [`TrustBook::pin`], reached from an
//! explicit user decision, persists a key.

use relay_io::LockExt;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use relay_config::identity::{TrustEntry, TrustLevel, TrustStore as ConfigStore};
use relay_net::discovery::name_skeleton;
use relay_net::identity::{
	Fingerprint, InMemoryTrustStore, PublicKey, TrustError, TrustRecord, TrustState, TrustStore,
};

const SEEN_KEYS_MAX: usize = 64;

/// A trust store that also remembers the public keys of recently
/// authenticated peers, because a decision must persist the key itself and
/// `TrustRecord` only carries its digest.
pub trait KeyedTrustStore: TrustStore + Send {
	/// Remembers an authenticated key so a later explicit `put` can persist it.
	fn observe(&mut self, key: &PublicKey);

	/// Every fingerprint the user has trusted (bounded by the store).
	fn fingerprints(&self) -> Vec<Fingerprint>;

	/// Atomically replaces the pinned key `old` by the authenticated key `new` as verified,
	/// carrying over the name and keeping the favorite. `new` must have been observed.
	fn replace(&mut self, old: &Fingerprint, new: &Fingerprint) -> Result<TrustRecord, TrustError>;
}

/// Shared handle used by link tasks and the manager. Never held across `.await`.
#[derive(Clone)]
pub struct SharedTrust(Arc<Mutex<dyn KeyedTrustStore>>);

impl SharedTrust {
	/// Wraps a store.
	pub fn new(store: impl KeyedTrustStore + 'static) -> Self {
		Self(Arc::new(Mutex::new(store)))
	}

	/// An in-memory store, for tests and ephemeral engines.
	pub fn in_memory() -> Self {
		Self::new(MemoryTrust::default())
	}

	/// Locks the store.
	pub fn lock(&self) -> MutexGuard<'_, dyn KeyedTrustStore + 'static> {
		self.0.lock_unpoisoned()
	}
}

/// Remembers recently seen keys in a bounded FIFO.
#[derive(Debug, Default)]
struct SeenKeys {
	keys: HashMap<Fingerprint, PublicKey>,
	order: VecDeque<Fingerprint>,
}

impl SeenKeys {
	fn insert(&mut self, key: &PublicKey) {
		let fingerprint = key.fingerprint();
		if self.keys.insert(fingerprint, *key).is_some() {
			return;
		}
		self.order.push_back(fingerprint);
		if self.order.len() > SEEN_KEYS_MAX
			&& let Some(oldest) = self.order.pop_front()
		{
			self.keys.remove(&oldest);
		}
	}

	fn get(&self, fingerprint: &Fingerprint) -> Option<PublicKey> {
		self.keys.get(fingerprint).copied()
	}
}

/// In-memory store with key observation.
#[derive(Debug, Default)]
pub struct MemoryTrust {
	store: InMemoryTrustStore,
	keys: HashSet<Fingerprint>,
}

impl TrustStore for MemoryTrust {
	fn get(&self, fingerprint: &Fingerprint) -> Option<TrustRecord> {
		self.store.get(fingerprint)
	}
	fn find_by_name(&self, display_name: &str) -> Vec<TrustRecord> {
		self.store.find_by_name(display_name)
	}
	fn put(&mut self, record: TrustRecord) -> Result<(), TrustError> {
		let fingerprint = record.fingerprint;
		self.store.put(record)?;
		self.keys.insert(fingerprint);
		Ok(())
	}
	fn remove(&mut self, fingerprint: &Fingerprint) -> Result<(), TrustError> {
		self.keys.remove(fingerprint);
		self.store.remove(fingerprint)
	}
}

impl KeyedTrustStore for MemoryTrust {
	fn observe(&mut self, _key: &PublicKey) {}

	fn fingerprints(&self) -> Vec<Fingerprint> {
		self.keys.iter().copied().collect()
	}

	fn replace(&mut self, old: &Fingerprint, new: &Fingerprint) -> Result<TrustRecord, TrustError> {
		let previous = self.store.get(old).ok_or(TrustError::Unknown)?;
		if self.store.get(new).is_some() {
			return Err(TrustError::Full);
		}
		let record = TrustRecord {
			fingerprint: *new,
			state: TrustState::Verified,
			..previous
		};
		self.store.remove(old)?;
		self.keys.remove(old);
		self.put(record.clone())?;
		Ok(record)
	}
}

/// Adapter over the persistent `relay-config` trust file.
pub struct ConfigTrust {
	store: ConfigStore,
	seen: SeenKeys,
}

impl ConfigTrust {
	/// Wraps an opened store.
	pub fn new(store: ConfigStore) -> Self {
		Self {
			store,
			seen: SeenKeys::default(),
		}
	}

	fn record(entry: &TrustEntry) -> Option<TrustRecord> {
		let key: [u8; 32] = hex::decode(&entry.public_key).ok()?.try_into().ok()?;
		Some(TrustRecord {
			fingerprint: PublicKey::from_bytes(key).fingerprint(),
			state: match entry.level {
				TrustLevel::Tofu => TrustState::UnverifiedFirstUse,
				TrustLevel::Verified => TrustState::Verified,
			},
			display_name: entry.display_name.clone(),
			trusted_at: UNIX_EPOCH + Duration::from_secs(entry.first_seen),
		})
	}

	fn key_of(&self, fingerprint: &Fingerprint) -> Option<[u8; 32]> {
		let entry = self
			.store
			.entries()
			.iter()
			.find(|e| Self::record(e).is_some_and(|r| &r.fingerprint == fingerprint))?;
		hex::decode(&entry.public_key).ok()?.try_into().ok()
	}

	fn records(&self) -> impl Iterator<Item = TrustRecord> + '_ {
		self.store.entries().iter().filter_map(Self::record)
	}
}

impl TrustStore for ConfigTrust {
	fn get(&self, fingerprint: &Fingerprint) -> Option<TrustRecord> {
		self.records().find(|r| &r.fingerprint == fingerprint)
	}

	fn find_by_name(&self, display_name: &str) -> Vec<TrustRecord> {
		// Compared on the confusable skeleton, so a lookalike name still raises KeyChanged.
		let wanted = name_skeleton(display_name);
		self.records()
			.filter(|r| name_skeleton(&r.display_name) == wanted)
			.collect()
	}

	/// Persists an explicit decision. Requires that the key was authenticated
	/// in this process (`observe`); a digest alone cannot be pinned.
	fn put(&mut self, record: TrustRecord) -> Result<(), TrustError> {
		let key = self
			.seen
			.get(&record.fingerprint)
			.ok_or(TrustError::Unknown)?;
		let bytes = key.as_bytes();
		let now = record
			.trusted_at
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_secs());
		let existing = self.get(&record.fingerprint);
		if existing.is_none() {
			self.store
				.pin_tofu(bytes, &record.display_name, now)
				.map_err(|_| TrustError::Full)?;
		}
		if record.state == TrustState::Verified {
			self.store
				.mark_verified(bytes)
				.map_err(|_| TrustError::Full)?;
		}
		Ok(())
	}

	fn remove(&mut self, fingerprint: &Fingerprint) -> Result<(), TrustError> {
		let entry = self
			.store
			.entries()
			.iter()
			.find(|e| Self::record(e).is_some_and(|r| &r.fingerprint == fingerprint))
			.ok_or(TrustError::Unknown)?;
		let key: [u8; 32] = hex::decode(&entry.public_key)
			.ok()
			.and_then(|bytes| bytes.try_into().ok())
			.ok_or(TrustError::Unknown)?;
		self.store.remove(&key).map_err(|_| TrustError::Unknown)?;
		Ok(())
	}
}

impl KeyedTrustStore for ConfigTrust {
	fn observe(&mut self, key: &PublicKey) {
		self.seen.insert(key);
	}

	fn fingerprints(&self) -> Vec<Fingerprint> {
		self.records().map(|r| r.fingerprint).collect()
	}

	fn replace(&mut self, old: &Fingerprint, new: &Fingerprint) -> Result<TrustRecord, TrustError> {
		let old_key = self.key_of(old).ok_or(TrustError::Unknown)?;
		let new_key = self.seen.get(new).ok_or(TrustError::Unknown)?;
		let entry = self
			.store
			.replace_key(&old_key, new_key.as_bytes())
			.map_err(|_| TrustError::Full)?;
		Self::record(&entry).ok_or(TrustError::Unknown)
	}
}

/// Current wall time for a new trust record.
pub fn now() -> SystemTime {
	relay_io::clock::system_time()
}
