//! Identity and channel secrets sealed by the Android Keystore.
//!
//! The Keystore key (AES-256-GCM, non-exportable) lives in Kotlin. Rust hands
//! the secret to [`HostBridge::seal`], gets ciphertext back and writes that to
//! an owner-only file in the app's private `filesDir`. Rust never sees the
//! Keystore key; Kotlin never sees where the files are. The associated data
//! names the slot, so a copied file does not open as a different secret.

use std::path::PathBuf;
use std::sync::Arc;

use relay_config::error::{ConfigError, Result};
use relay_config::identity::{IdentityStore, Secret32, StoredIdentity};
use relay_config::vault::{SecretVault, validate_slot};
use relay_io::fs::{
	AtomicWrite, backup_path, ensure_private_dir, read_if_exists, remove_if_exists,
};
use zeroize::Zeroizing;

use crate::bridge::HostBridge;

/// Encrypts and decrypts one value. The real one is the Keystore; tests use a fake.
pub trait Sealer: Send + Sync {
	fn seal(&self, context: &str, plain: &[u8]) -> Result<Vec<u8>>;
	fn open(&self, context: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// [`Sealer`] backed by the Kotlin Keystore wrapper.
pub struct KeystoreSealer(pub Arc<dyn HostBridge>);

impl Sealer for KeystoreSealer {
	fn seal(&self, context: &str, plain: &[u8]) -> Result<Vec<u8>> {
		self.0
			.seal(context.to_owned(), plain.to_vec())
			.map_err(|e| ConfigError::CredentialStore(e.to_string()))
	}

	fn open(&self, context: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
		self.0
			.open(context.to_owned(), sealed.to_vec())
			.map(Zeroizing::new)
			.map_err(|e| ConfigError::CredentialStore(e.to_string()))
	}
}

fn write_private(path: &PathBuf, bytes: &[u8]) -> Result<()> {
	AtomicWrite::new(path).private(true).write(bytes)?;
	// The sealed file has no backup copy: a stale one would outlive a deletion.
	let _ = remove_if_exists(&backup_path(path));
	Ok(())
}

const IDENTITY_CONTEXT: &str = "identity";

/// The device identity: `secret || public` sealed into one file.
pub struct SealedIdentityStore {
	path: PathBuf,
	sealer: Arc<dyn Sealer>,
}

impl SealedIdentityStore {
	pub fn new(path: PathBuf, sealer: Arc<dyn Sealer>) -> Self {
		Self { path, sealer }
	}
}

impl IdentityStore for SealedIdentityStore {
	fn describe(&self) -> &'static str {
		"Android Keystore (AES-GCM key, sealed file in app storage)"
	}

	fn load(&self) -> Result<Option<StoredIdentity>> {
		let Some(sealed) = read_if_exists(&self.path)? else {
			return Ok(None);
		};
		let plain = self.sealer.open(IDENTITY_CONTEXT, &sealed)?;
		let bytes: &[u8; 64] = plain.as_slice().try_into().map_err(|_| {
			ConfigError::CredentialStore("sealed identity has the wrong size".into())
		})?;
		let (secret, public) = bytes.split_at(32);
		Ok(Some(StoredIdentity {
			secret: Secret32::new(secret.try_into().expect("32 bytes")),
			public: public.try_into().expect("32 bytes"),
		}))
	}

	fn save(&self, identity: &StoredIdentity) -> Result<()> {
		let mut plain = Zeroizing::new(Vec::with_capacity(64));
		plain.extend_from_slice(identity.secret.expose());
		plain.extend_from_slice(&identity.public);
		let sealed = self.sealer.seal(IDENTITY_CONTEXT, &plain)?;
		write_private(&self.path, &sealed)
	}

	fn delete(&self) -> Result<()> {
		Ok(remove_if_exists(&self.path)?)
	}
}

/// Channel secrets: one sealed file per `(channel, slot)`.
pub struct SealedVault {
	dir: PathBuf,
	sealer: Arc<dyn Sealer>,
}

impl SealedVault {
	pub fn new(dir: PathBuf, sealer: Arc<dyn Sealer>) -> Self {
		Self { dir, sealer }
	}

	fn name(channel: &[u8; 16], slot: &str) -> Result<String> {
		validate_slot(slot)?;
		Ok(format!("{}.{slot}", hex::encode(channel)))
	}
}

impl SecretVault for SealedVault {
	fn describe(&self) -> &'static str {
		"Android Keystore (AES-GCM key, sealed files in app storage)"
	}

	fn get(&self, channel: &[u8; 16], slot: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
		let name = Self::name(channel, slot)?;
		match read_if_exists(&self.dir.join(&name))? {
			Some(sealed) => self.sealer.open(&name, &sealed).map(Some),
			None => Ok(None),
		}
	}

	fn put(&self, channel: &[u8; 16], slot: &str, secret: &[u8]) -> Result<()> {
		let name = Self::name(channel, slot)?;
		ensure_private_dir(&self.dir)?;
		let sealed = self.sealer.seal(&name, secret)?;
		write_private(&self.dir.join(name), &sealed)
	}

	fn delete(&self, channel: &[u8; 16], slot: &str) -> Result<()> {
		Ok(remove_if_exists(
			&self.dir.join(Self::name(channel, slot)?),
		)?)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Reversible and context-bound, like AES-GCM with associated data, without a key.
	struct Fake;

	impl Sealer for Fake {
		fn seal(&self, context: &str, plain: &[u8]) -> Result<Vec<u8>> {
			let mut out = context.as_bytes().to_vec();
			out.push(b'|');
			out.extend(plain.iter().map(|b| b ^ 0x5a));
			Ok(out)
		}

		fn open(&self, context: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
			let prefix = format!("{context}|");
			let body = sealed
				.strip_prefix(prefix.as_bytes())
				.ok_or_else(|| ConfigError::CredentialStore("wrong context".into()))?;
			Ok(Zeroizing::new(body.iter().map(|b| b ^ 0x5a).collect()))
		}
	}

	fn identity() -> StoredIdentity {
		StoredIdentity {
			secret: Secret32::new([7; 32]),
			public: [9; 32],
		}
	}

	#[test]
	fn identity_round_trips_and_is_not_stored_in_clear() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("identity.sealed");
		let store = SealedIdentityStore::new(path.clone(), Arc::new(Fake));
		assert!(store.load().unwrap().is_none());
		store.save(&identity()).unwrap();
		let on_disk = std::fs::read(&path).unwrap();
		assert!(
			!on_disk.windows(32).any(|w| w == [7u8; 32]),
			"secret must not be in clear"
		);
		let back = store.load().unwrap().expect("stored");
		assert_eq!(back.secret.expose(), &[7u8; 32]);
		assert_eq!(back.public, [9u8; 32]);
		store.delete().unwrap();
		assert!(store.load().unwrap().is_none());
	}

	#[cfg(unix)]
	#[test]
	fn sealed_files_are_owner_only() {
		use std::os::unix::fs::PermissionsExt;
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("identity.sealed");
		SealedIdentityStore::new(path.clone(), Arc::new(Fake))
			.save(&identity())
			.unwrap();
		let mode = std::fs::metadata(path).unwrap().permissions().mode();
		assert_eq!(mode & 0o077, 0, "group and other must have no access");
	}

	#[test]
	fn vault_binds_secrets_to_their_slot() {
		let dir = tempfile::tempdir().unwrap();
		let vault = SealedVault::new(dir.path().join("v"), Arc::new(Fake));
		let channel = [3u8; 16];
		vault.put(&channel, "signing", b"top secret").unwrap();
		assert_eq!(
			&**vault.get(&channel, "signing").unwrap().unwrap(),
			b"top secret"
		);
		assert!(vault.get(&channel, "password").unwrap().is_none());
		// A file moved to another slot does not open there.
		let from = dir
			.path()
			.join("v")
			.join(format!("{}.signing", hex::encode(channel)));
		let to = dir
			.path()
			.join("v")
			.join(format!("{}.password", hex::encode(channel)));
		std::fs::copy(from, to).unwrap();
		assert!(vault.get(&channel, "password").is_err());
		vault.delete(&channel, "signing").unwrap();
		assert!(vault.get(&channel, "signing").unwrap().is_none());
	}
}
