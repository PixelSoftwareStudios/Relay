//! The daemon's keychain handle reads the OS item once per process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_config::identity::{IdentityStore, KeyringStore, Secret32, StoredIdentity};
use relay_config::keychain::{Keychain, MemoryBackend, SecretBackend};
use relay_config::vault::{KeyringVault, SecretVault};
use zeroize::Zeroizing;

/// Counts reads and lets one read take a while (the system prompt).
#[derive(Default)]
struct Counting {
	inner: MemoryBackend,
	gets: AtomicUsize,
	active: Mutex<usize>,
	overlapped: AtomicUsize,
	fail_next: Mutex<bool>,
}

impl SecretBackend for Counting {
	fn get(&self) -> relay_config::Result<Option<Zeroizing<Vec<u8>>>> {
		self.gets.fetch_add(1, Ordering::SeqCst);
		{
			let mut active = self.active.lock().unwrap();
			*active += 1;
			if *active > 1 {
				self.overlapped.fetch_add(1, Ordering::SeqCst);
			}
		}
		std::thread::sleep(Duration::from_millis(40));
		*self.active.lock().unwrap() -= 1;
		if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
			return Err(relay_config::ConfigError::CredentialStore("denied".into()));
		}
		self.inner.get()
	}

	fn set(&self, blob: &[u8]) -> relay_config::Result<()> {
		self.inner.set(blob)
	}

	fn delete(&self) -> relay_config::Result<()> {
		self.inner.delete()
	}
}

fn identity() -> StoredIdentity {
	StoredIdentity {
		secret: Secret32::new([3; 32]),
		public: [4; 32],
	}
}

#[test]
fn identity_and_channel_secrets_cost_one_read() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Counting::default());
	let keychain = Arc::new(Keychain::cached(backend.clone(), dir.path().join("k.lock")));
	let store = KeyringStore::new(keychain.clone());
	let vault = KeyringVault::new(keychain);
	store.save(&identity()).unwrap();
	vault.put(&[1; 16], "owner", b"key").unwrap();
	for _ in 0..5 {
		assert_eq!(store.load().unwrap(), Some(identity()));
		assert_eq!(
			vault.get(&[1; 16], "owner").unwrap().unwrap().as_slice(),
			b"key"
		);
	}
	assert_eq!(backend.gets.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_first_reads_share_one_prompt() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Counting::default());
	let keychain = Arc::new(Keychain::cached(backend.clone(), dir.path().join("k.lock")));
	let threads: Vec<_> = (0..4)
		.map(|_| {
			let store = KeyringStore::new(keychain.clone());
			std::thread::spawn(move || store.load().unwrap())
		})
		.collect();
	for thread in threads {
		thread.join().unwrap();
	}
	assert_eq!(backend.gets.load(Ordering::SeqCst), 1);
	assert_eq!(backend.overlapped.load(Ordering::SeqCst), 0);
}

#[test]
fn a_denied_read_is_not_cached_and_the_retry_asks_again() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Counting::default());
	*backend.fail_next.lock().unwrap() = true;
	let keychain = Arc::new(Keychain::cached(backend.clone(), dir.path().join("k.lock")));
	let store = KeyringStore::new(keychain);
	assert!(store.load().is_err());
	assert_eq!(store.load().unwrap(), None);
	assert_eq!(backend.gets.load(Ordering::SeqCst), 2);
}

#[test]
fn the_os_handle_is_one_per_lock_file() {
	let dir = tempfile::tempdir().unwrap();
	let lock = dir.path().join("k.lock");
	assert!(Arc::ptr_eq(&Keychain::os(&lock), &Keychain::os(&lock)));
}
