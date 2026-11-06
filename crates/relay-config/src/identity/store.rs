//! Identity secret storage behind a trait: OS credential store, explicit file, memory.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use relay_io::fs::{AtomicWrite, backup_path, read_if_exists, remove_if_exists};

use super::secret::StoredIdentity;
use crate::error::{ConfigError, Result};
use crate::keychain::Keychain;
use crate::paths::Paths;
use crate::prefs::{KeyStorage, Preferences};

/// Persistence for the one local identity.
pub trait IdentityStore: Send + Sync {
	/// Honest, user-displayable description of where the key lives.
	fn describe(&self) -> &'static str;
	/// `Ok(Some)` is found, `Ok(None)` is the store's definitive answer that no identity
	/// exists, and `Err` is anything else (denied, locked, interrupted, unreadable,
	/// malformed). Only `Ok(None)` may lead to a new identity; see [`IdentityStore::create`].
	fn load(&self) -> Result<Option<StoredIdentity>>;
	/// Replaces whatever is stored. Only a deliberate reset uses this.
	fn save(&self, identity: &StoredIdentity) -> Result<()>;
	fn delete(&self) -> Result<()>;
	/// Stores `identity` only if none exists and returns the identity that is stored
	/// afterwards: `identity` itself, or the one another process created first. Never
	/// overwrites. Stores that several processes can reach override this with an atomic
	/// create; the default suits a store only this process writes.
	fn create(&self, identity: &StoredIdentity) -> Result<StoredIdentity> {
		match self.load()? {
			Some(existing) => Ok(existing),
			None => {
				self.save(identity)?;
				Ok(identity.clone())
			}
		}
	}
}

/// Loads the stored identity, or creates one with `generate` when the store says none
/// exists. A failing store is an error: nothing is generated and nothing is written.
/// `created` tells whether this call made the identity.
pub fn load_or_create(
	store: &dyn IdentityStore,
	generate: impl FnOnce() -> Result<StoredIdentity>,
) -> Result<(StoredIdentity, bool)> {
	if let Some(existing) = store.load()? {
		return Ok((existing, false));
	}
	let fresh = generate()?;
	let stored = store.create(&fresh)?;
	let created = stored == fresh;
	Ok((stored, created))
}

/// Opens the store the preferences ask for. There is no automatic fallback:
/// a failing OS credential store surfaces its error to the user.
pub fn open_store(prefs: &Preferences, paths: &Paths) -> Result<Box<dyn IdentityStore>> {
	match prefs.identity.key_storage {
		KeyStorage::OsCredentialStore => Ok(Box::new(KeyringStore::new(Keychain::os(
			&paths.keychain_lock_file(),
		)))),
		KeyStorage::InsecureFile => Ok(Box::new(InsecureFileStore::new(
			paths.identity_file(),
			prefs.identity.key_storage,
		)?)),
	}
}

/// OS credential store (Keychain, Credential Manager, Secret Service), sharing the one
/// per-user secret item with the channel vault.
#[derive(Debug)]
pub struct KeyringStore {
	keychain: Arc<Keychain>,
}

impl KeyringStore {
	pub fn new(keychain: Arc<Keychain>) -> Self {
		Self { keychain }
	}
}

impl IdentityStore for KeyringStore {
	fn describe(&self) -> &'static str {
		"OS credential store"
	}

	fn load(&self) -> Result<Option<StoredIdentity>> {
		match self.keychain.identity()? {
			Some(bytes) => StoredIdentity::decode(&bytes).map(Some),
			None => Ok(None),
		}
	}

	fn save(&self, identity: &StoredIdentity) -> Result<()> {
		self.keychain.set_identity(&identity.encode())
	}

	fn delete(&self) -> Result<()> {
		self.keychain.delete_identity()
	}

	fn create(&self, identity: &StoredIdentity) -> Result<StoredIdentity> {
		match self.keychain.create_identity(&identity.encode())? {
			Some(existing) => StoredIdentity::decode(&existing),
			None => Ok(identity.clone()),
		}
	}
}

/// Unencrypted key file with mode 0600. Weaker than the OS store and only
/// constructible when the user explicitly chose [`KeyStorage::InsecureFile`].
#[derive(Debug)]
pub struct InsecureFileStore {
	path: PathBuf,
}

impl InsecureFileStore {
	pub fn new(path: impl Into<PathBuf>, setting: KeyStorage) -> Result<Self> {
		if setting != KeyStorage::InsecureFile {
			return Err(ConfigError::InsecureStorageNotEnabled);
		}
		Ok(Self { path: path.into() })
	}

	pub fn path(&self) -> &Path {
		&self.path
	}
}

impl IdentityStore for InsecureFileStore {
	fn describe(&self) -> &'static str {
		"INSECURE unencrypted file (0600) chosen by the user"
	}

	fn load(&self) -> Result<Option<StoredIdentity>> {
		match read_if_exists(&self.path)? {
			Some(bytes) => {
				let bytes = zeroize::Zeroizing::new(bytes);
				StoredIdentity::decode(&bytes).map(Some)
			}
			None => Ok(None),
		}
	}

	fn save(&self, identity: &StoredIdentity) -> Result<()> {
		AtomicWrite::new(&self.path)
			.private(true)
			.backup(true)
			.write(&identity.encode())?;
		// Keep no plaintext backup copy of a private key.
		let _ = remove_if_exists(&backup_path(&self.path));
		Ok(())
	}

	fn delete(&self) -> Result<()> {
		Ok(remove_if_exists(&self.path)?)
	}

	fn create(&self, identity: &StoredIdentity) -> Result<StoredIdentity> {
		// Stage the whole file, then link it into place: the link fails when a file
		// appeared meanwhile, and nobody ever sees a half-written key.
		let staged = AtomicWrite::new(&self.path)
			.private(true)
			.stage(&identity.encode())?;
		let linked = std::fs::hard_link(staged.temp_path(), &self.path);
		let _ = remove_if_exists(staged.temp_path());
		match linked {
			Ok(()) => Ok(identity.clone()),
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
				self.load()?.ok_or(ConfigError::IdentityMissing)
			}
			Err(e) => Err(ConfigError::Io {
				path: self.path.clone(),
				source: e,
			}),
		}
	}
}

/// In-memory store for tests and embedding.
#[derive(Debug, Default)]
pub struct MemoryStore {
	slot: Mutex<Option<StoredIdentity>>,
}

impl IdentityStore for MemoryStore {
	fn describe(&self) -> &'static str {
		"in-memory (not persistent)"
	}

	fn load(&self) -> Result<Option<StoredIdentity>> {
		Ok(self.slot.lock().expect("memory store poisoned").clone())
	}

	fn save(&self, identity: &StoredIdentity) -> Result<()> {
		*self.slot.lock().expect("memory store poisoned") = Some(identity.clone());
		Ok(())
	}

	fn delete(&self) -> Result<()> {
		*self.slot.lock().expect("memory store poisoned") = None;
		Ok(())
	}

	fn create(&self, identity: &StoredIdentity) -> Result<StoredIdentity> {
		let mut slot = self.slot.lock().expect("memory store poisoned");
		Ok(slot.get_or_insert_with(|| identity.clone()).clone())
	}
}
