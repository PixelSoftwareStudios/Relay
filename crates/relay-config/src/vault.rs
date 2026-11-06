//! Secret storage for material that must never be written to SQLite: channel signing
//! keys and password or invite credentials.
//!
//! A vault maps `(channel id, slot)` to secret bytes. The default is the OS credential
//! store; a 0600 file store exists only when the user explicitly chose
//! [`KeyStorage::InsecureFile`], exactly like the identity key.

use relay_io::LockExt;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use relay_io::fs::{
	AtomicWrite, backup_path, ensure_private_dir, read_if_exists, remove_if_exists,
};
use zeroize::Zeroizing;

use crate::error::{ConfigError, Result};
use crate::keychain::Keychain;
use crate::paths::Paths;
use crate::prefs::{KeyStorage, Preferences};

const SLOT_MAX_CHARS: usize = 64;

/// Checks that a slot name is short and filename-safe (`a-z 0-9 . _ -`).
pub fn validate_slot(slot: &str) -> Result<()> {
	let ok = !slot.is_empty()
		&& slot.len() <= SLOT_MAX_CHARS
		&& slot
			.bytes()
			.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
	if ok {
		Ok(())
	} else {
		Err(ConfigError::InvalidPreference(format!(
			"bad secret slot name `{slot}`"
		)))
	}
}

/// Persistence for small secrets keyed by `(channel id, slot)`.
pub trait SecretVault: Send + Sync {
	/// Honest, user-displayable description of where secrets live.
	fn describe(&self) -> &'static str;
	fn get(&self, channel: &[u8; 16], slot: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
	fn put(&self, channel: &[u8; 16], slot: &str, secret: &[u8]) -> Result<()>;
	/// Deleting a missing secret is not an error.
	fn delete(&self, channel: &[u8; 16], slot: &str) -> Result<()>;
}

/// Opens the vault the preferences ask for; there is no automatic fallback.
pub fn open_vault(prefs: &Preferences, paths: &Paths) -> Result<Arc<dyn SecretVault>> {
	match prefs.identity.key_storage {
		KeyStorage::OsCredentialStore => Ok(Arc::new(KeyringVault::new(Keychain::os(
			&paths.keychain_lock_file(),
		)))),
		KeyStorage::InsecureFile => Ok(Arc::new(FileVault::new(
			paths.channel_secrets_dir(),
			prefs.identity.key_storage,
		)?)),
	}
}

/// OS credential store (Keychain, Credential Manager, Secret Service), sharing the one
/// per-user secret item with the identity key.
#[derive(Debug)]
pub struct KeyringVault {
	keychain: Arc<Keychain>,
}

impl KeyringVault {
	pub fn new(keychain: Arc<Keychain>) -> Self {
		Self { keychain }
	}

	fn key(channel: &[u8; 16], slot: &str) -> Result<String> {
		validate_slot(slot)?;
		Ok(format!("{}/{slot}", hex::encode(channel)))
	}
}

impl SecretVault for KeyringVault {
	fn describe(&self) -> &'static str {
		"OS credential store"
	}

	fn get(&self, channel: &[u8; 16], slot: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
		self.keychain.channel_secret(&Self::key(channel, slot)?)
	}

	fn put(&self, channel: &[u8; 16], slot: &str, secret: &[u8]) -> Result<()> {
		self.keychain
			.set_channel_secret(Self::key(channel, slot)?, secret)
	}

	fn delete(&self, channel: &[u8; 16], slot: &str) -> Result<()> {
		self.keychain
			.delete_channel_secret(&Self::key(channel, slot)?)
	}
}

/// One 0600 file per secret. Weaker than the OS store; constructible only when the
/// user explicitly chose [`KeyStorage::InsecureFile`].
#[derive(Debug)]
pub struct FileVault {
	dir: PathBuf,
}

impl FileVault {
	pub fn new(dir: impl Into<PathBuf>, setting: KeyStorage) -> Result<Self> {
		if setting != KeyStorage::InsecureFile {
			return Err(ConfigError::InsecureStorageNotEnabled);
		}
		Ok(Self { dir: dir.into() })
	}

	fn file(&self, channel: &[u8; 16], slot: &str) -> Result<PathBuf> {
		validate_slot(slot)?;
		Ok(self.dir.join(format!("{}.{slot}", hex::encode(channel))))
	}
}

impl SecretVault for FileVault {
	fn describe(&self) -> &'static str {
		"INSECURE unencrypted files (0600) chosen by the user"
	}

	fn get(&self, channel: &[u8; 16], slot: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
		Ok(read_if_exists(&self.file(channel, slot)?)?.map(Zeroizing::new))
	}

	fn put(&self, channel: &[u8; 16], slot: &str, secret: &[u8]) -> Result<()> {
		ensure_private_dir(&self.dir)?;
		let path = self.file(channel, slot)?;
		AtomicWrite::new(&path)
			.private(true)
			.backup(true)
			.write(secret)?;
		// No plaintext backup copy of a secret.
		let _ = remove_if_exists(&backup_path(&path));
		Ok(())
	}

	fn delete(&self, channel: &[u8; 16], slot: &str) -> Result<()> {
		let path = self.file(channel, slot)?;
		Ok(remove_if_exists(&path)?)
	}
}

type Secrets = BTreeMap<([u8; 16], String), Vec<u8>>;

/// In-memory vault for tests and embedding.
#[derive(Debug, Default)]
pub struct MemoryVault {
	secrets: Mutex<Secrets>,
}

impl MemoryVault {
	fn map(&self) -> std::sync::MutexGuard<'_, Secrets> {
		self.secrets.lock_unpoisoned()
	}

	/// Number of stored secrets (tests assert nothing is orphaned).
	pub fn len(&self) -> usize {
		self.map().len()
	}

	pub fn is_empty(&self) -> bool {
		self.map().is_empty()
	}
}

impl SecretVault for MemoryVault {
	fn describe(&self) -> &'static str {
		"in-memory (not persistent)"
	}

	fn get(&self, channel: &[u8; 16], slot: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
		Ok(self
			.map()
			.get(&(*channel, slot.to_owned()))
			.map(|v| Zeroizing::new(v.clone())))
	}

	fn put(&self, channel: &[u8; 16], slot: &str, secret: &[u8]) -> Result<()> {
		validate_slot(slot)?;
		self.map()
			.insert((*channel, slot.to_owned()), secret.to_vec());
		Ok(())
	}

	fn delete(&self, channel: &[u8; 16], slot: &str) -> Result<()> {
		self.map().remove(&(*channel, slot.to_owned()));
		Ok(())
	}
}
