//! One credential-store item per user holding every secret Relay keeps.
//!
//! Identity key and channel secrets live together in a single versioned [`Bundle`]
//! under `relay-voice` / `vault`, so the OS asks for access once instead of once per
//! secret. Every change is a locked read-modify-write: an in-process mutex, then the
//! cross-process [`ProcessLock`] on a lock file, so two processes of one user never
//! lose each other's updates.

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
	pub fn new(backend: Arc<dyn SecretBackend>, lock_file: impl Into<PathBuf>) -> Self {
		Self {
			backend,
			lock_file: lock_file.into(),
			local: Arc::default(),
		}
	}

	/// The OS credential store, sharing one in-process lock per lock file so the identity
	/// store and the channel vault of one process never contend through the file lock.
	pub fn os(lock_file: &Path) -> Arc<Self> {
		static LOCALS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
		let local = LOCALS
			.get_or_init(Mutex::default)
			.lock_unpoisoned()
			.entry(lock_file.to_path_buf())
			.or_default()
			.clone();
		Arc::new(Self {
			backend: Arc::new(KeyringBackend),
			lock_file: lock_file.to_path_buf(),
			local,
		})
	}

	fn read_bundle(&self) -> Result<Bundle> {
		match self.backend.get()? {
			Some(blob) => Bundle::decode(&blob),
			None => Ok(Bundle::default()),
		}
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
		Ok(f(&self.read_bundle()?))
	}

	/// Atomic read-modify-write. `f` returns whether it changed anything; an unchanged
	/// bundle is not written back, an emptied one removes the item.
	fn update<R>(&self, f: impl FnOnce(&mut Bundle) -> (bool, R)) -> Result<R> {
		let _local = self.local.lock_unpoisoned();
		let _file = self.acquire()?;
		let mut bundle = self.read_bundle()?;
		let (changed, out) = f(&mut bundle);
		if changed {
			if bundle.is_empty() {
				self.backend.delete()?;
			} else {
				self.backend.set(&bundle.encode()?)?;
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

	pub(crate) fn delete_identity(&self) -> Result<()> {
		self.update(|b| (b.identity.take().is_some(), ()))
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
