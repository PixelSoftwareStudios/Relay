//! One credential-store item per user holding every secret Relay keeps.
//!
//! Identity key and channel secrets live together in a single versioned [`Bundle`]
//! under `relay-voice` / `vault`, so the OS asks for access once instead of once per
//! secret. Every change is a locked read-modify-write: an in-process mutex, then the
//! cross-process [`ProcessLock`] on a lock file, so two processes of one user never
//! lose each other's updates.
//!
//! The daemon is the only process that opens the OS store ([`Keychain::os`]). It reads the
//! item once and keeps the decoded bundle in memory for the life of the process, so the
//! operating system prompts at most once per start, and never twice at the same time:
//! readers queue behind the one read that is in flight. A failed read is not cached, so a
//! retry asks the store again.

mod backend;
mod bundle;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use relay_io::LockExt;
use relay_io::lock::ProcessLock;
use zeroize::Zeroizing;

pub use backend::{KEYRING_ACCOUNT, KEYRING_SERVICE, KeyringBackend, MemoryBackend, SecretBackend};
use bundle::{Bundle, SecretBytes};

use crate::error::{ConfigError, Result};

const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(2);

/// Locked access to the secret bundle. Cheap to clone via `Arc`.
pub struct Keychain {
	backend: Arc<dyn SecretBackend>,
	lock_file: PathBuf,
	local: Arc<Mutex<()>>,
	/// `Some` when the decoded bundle is kept in memory after the first read.
	cache: Option<Mutex<Option<Bundle>>>,
}

impl std::fmt::Debug for Keychain {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Keychain")
			.field("lock_file", &self.lock_file)
			.finish_non_exhaustive()
	}
}

impl Keychain {
	/// Independent in-process lock; processes (and tests) are kept apart by the lock file.
	/// Every read goes to the backend.
	pub fn new(backend: Arc<dyn SecretBackend>, lock_file: impl Into<PathBuf>) -> Self {
		Self {
			backend,
			lock_file: lock_file.into(),
			local: Arc::default(),
			cache: None,
		}
	}

	/// Like [`Keychain::new`] but the bundle is read from the backend once and then served
	/// from memory; writes go through to the backend and update the copy. Only for a
	/// process that is the single owner of the item (the daemon).
	pub fn cached(backend: Arc<dyn SecretBackend>, lock_file: impl Into<PathBuf>) -> Self {
		Self {
			cache: Some(Mutex::new(None)),
			..Self::new(backend, lock_file)
		}
	}

	/// The OS credential store. One cached instance per lock file and process, shared by the
	/// identity store and the channel vault: they contend through neither the file lock nor
	/// a second prompt.
	pub fn os(lock_file: &Path) -> Arc<Self> {
		static SHARED: OnceLock<Mutex<HashMap<PathBuf, Arc<Keychain>>>> = OnceLock::new();
		SHARED
			.get_or_init(Mutex::default)
			.lock_unpoisoned()
			.entry(lock_file.to_path_buf())
			.or_insert_with(|| Arc::new(Self::cached(Arc::new(KeyringBackend), lock_file)))
			.clone()
	}

	fn fetch(&self) -> Result<Bundle> {
		match self.backend.get()? {
			Some(blob) => Bundle::decode(&blob),
			None => Ok(Bundle::default()),
		}
	}

	/// Runs `f` on the current bundle. With a cache the backend is asked only while the copy
	/// is empty, and the cache lock is held across that read so there is one read in flight.
	fn with_bundle<R>(&self, f: impl FnOnce(&Bundle) -> R) -> Result<R> {
		let Some(cache) = &self.cache else {
			return Ok(f(&self.fetch()?));
		};
		let mut held = cache.lock_unpoisoned();
		if held.is_none() {
			*held = Some(self.fetch()?);
		}
		Ok(f(held.as_ref().unwrap_or(&Bundle::default())))
	}

	fn current(&self) -> Result<Bundle> {
		self.with_bundle(Bundle::clone)
	}

	fn acquire(&self) -> Result<ProcessLock> {
		let start = Instant::now();
		loop {
			match ProcessLock::acquire(&self.lock_file) {
				Ok(lock) => return Ok(lock),
				Err(relay_io::IoError::Locked(path)) if start.elapsed() >= LOCK_WAIT => {
					return Err(ConfigError::Locked(path));
				}
				Err(relay_io::IoError::Locked(_)) => std::thread::sleep(LOCK_POLL),
				Err(e) => return Err(e.into()),
			}
		}
	}

	/// Reads one value out of the bundle.
	fn read<R>(&self, f: impl FnOnce(&Bundle) -> R) -> Result<R> {
		self.with_bundle(f)
	}

	/// Atomic read-modify-write. `f` returns whether it changed anything; an unchanged
	/// bundle is not written back, an emptied one removes the item.
	fn update<R>(&self, f: impl FnOnce(&mut Bundle) -> (bool, R)) -> Result<R> {
		let _local = self.local.lock_unpoisoned();
		let _file = self.acquire()?;
		let mut bundle = self.current()?;
		let (changed, out) = f(&mut bundle);
		if changed {
			if bundle.is_empty() {
				self.backend.delete()?;
			} else {
				self.backend.set(&bundle.encode()?)?;
			}
			if let Some(cache) = &self.cache {
				*cache.lock_unpoisoned() = Some(bundle);
			}
		}
		Ok(out)
	}

	pub(crate) fn identity(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
		self.read(|b| {
			b.identity
				.as_ref()
				.map(|s| Zeroizing::new(s.bytes().to_vec()))
		})
	}

	pub(crate) fn set_identity(&self, bytes: &[u8]) -> Result<()> {
		self.update(|b| {
			b.identity = Some(SecretBytes::new(bytes));
			(true, ())
		})
	}

	/// Stores the identity only if the item holds none: `Ok(None)` when it was stored, the
	/// existing bytes otherwise. One locked read-modify-write, so two starts cannot both win.
	pub(crate) fn create_identity(&self, bytes: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>> {
		self.update(|b| match &b.identity {
			Some(existing) => (false, Some(Zeroizing::new(existing.bytes().to_vec()))),
			None => {
				b.identity = Some(SecretBytes::new(bytes));
				(true, None)
			}
		})
	}

	/// Removes the identity. A stored bundle that cannot be read at all (corrupt, or from a
	/// newer version) is removed whole: this is the deliberate reset, the only way out.
	pub(crate) fn delete_identity(&self) -> Result<()> {
		match self.update(|b| (b.identity.take().is_some(), ())) {
			Err(ConfigError::BundleUnreadable(_)) => {
				let _local = self.local.lock_unpoisoned();
				let _file = self.acquire()?;
				self.backend.delete()?;
				if let Some(cache) = &self.cache {
					*cache.lock_unpoisoned() = None;
				}
				Ok(())
			}
			other => other,
		}
	}

	pub(crate) fn channel_secret(&self, key: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
		self.read(|b| {
			b.channels
				.get(key)
				.map(|s| Zeroizing::new(s.bytes().to_vec()))
		})
	}

	pub(crate) fn set_channel_secret(&self, key: String, bytes: &[u8]) -> Result<()> {
		self.update(|b| {
			b.channels.insert(key, SecretBytes::new(bytes));
			(true, ())
		})
	}

	pub(crate) fn delete_channel_secret(&self, key: &str) -> Result<()> {
		self.update(|b| (b.channels.remove(key).is_some(), ()))
	}
}
