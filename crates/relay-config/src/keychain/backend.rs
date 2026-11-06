//! Where the one blob physically lives.

use std::sync::Mutex;

use relay_io::LockExt;
use zeroize::Zeroizing;

use crate::error::{ConfigError, Result};

/// Service name of the single credential-store item.
pub const KEYRING_SERVICE: &str = "relay-voice";
/// Account name of the single credential-store item.
pub const KEYRING_ACCOUNT: &str = "vault";

/// Raw storage for the one blob. Implementations need no locking of their own.
pub trait SecretBackend: Send + Sync {
	fn get(&self) -> Result<Option<Zeroizing<Vec<u8>>>>;
	fn set(&self, blob: &[u8]) -> Result<()>;
	/// Deleting a missing blob is not an error.
	fn delete(&self) -> Result<()>;
}

/// The OS credential store (Keychain, Credential Manager, Secret Service).
#[derive(Debug, Default)]
pub struct KeyringBackend;

impl KeyringBackend {
	fn entry() -> Result<keyring::Entry> {
		keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
			.map_err(|e| ConfigError::CredentialStore(e.to_string()))
	}
}

impl SecretBackend for KeyringBackend {
	fn get(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
		match Self::entry()?.get_secret() {
			Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
			Err(keyring::Error::NoEntry) => Ok(None),
			Err(e) => Err(ConfigError::CredentialStore(e.to_string())),
		}
	}

	fn set(&self, blob: &[u8]) -> Result<()> {
		Self::entry()?
			.set_secret(blob)
			.map_err(|e| ConfigError::CredentialStore(e.to_string()))
	}

	fn delete(&self) -> Result<()> {
		match Self::entry()?.delete_credential() {
			Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
			Err(e) => Err(ConfigError::CredentialStore(e.to_string())),
		}
	}
}

/// In-memory backend for tests and embedding.
#[derive(Debug, Default)]
pub struct MemoryBackend {
	blob: Mutex<Option<Vec<u8>>>,
}

impl SecretBackend for MemoryBackend {
	fn get(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
		Ok(self.blob.lock_unpoisoned().clone().map(Zeroizing::new))
	}

	fn set(&self, blob: &[u8]) -> Result<()> {
		*self.blob.lock_unpoisoned() = Some(blob.to_vec());
		Ok(())
	}

	fn delete(&self) -> Result<()> {
		*self.blob.lock_unpoisoned() = None;
		Ok(())
	}
}
