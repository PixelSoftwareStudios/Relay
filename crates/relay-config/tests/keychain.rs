use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_config::identity::{IdentityStore, KeyringStore, Secret32, StoredIdentity};
use relay_config::keychain::{Keychain, MemoryBackend, SecretBackend};
use relay_config::vault::{KeyringVault, SecretVault};
use zeroize::Zeroizing;

/// A fake keyring whose reads are slow, so an unlocked read-modify-write loses updates.
#[derive(Default)]
struct SlowBackend {
	inner: MemoryBackend,
	writes: Mutex<usize>,
	deletes: Mutex<usize>,
}

impl SecretBackend for SlowBackend {
	fn get(&self) -> relay_config::Result<Option<Zeroizing<Vec<u8>>>> {
		let out = self.inner.get();
		std::thread::sleep(Duration::from_millis(1));
		out
	}

	fn set(&self, blob: &[u8]) -> relay_config::Result<()> {
		*self.writes.lock().unwrap() += 1;
		self.inner.set(blob)
	}

	fn delete(&self) -> relay_config::Result<()> {
		*self.deletes.lock().unwrap() += 1;
		self.inner.delete()
	}
}

fn identity(n: u8) -> StoredIdentity {
	StoredIdentity {
		secret: Secret32::new([n; 32]),
		public: [n.wrapping_add(1); 32],
	}
}

fn pair(backend: &Arc<SlowBackend>, lock: &std::path::Path) -> (KeyringStore, KeyringVault) {
	let keychain = Arc::new(Keychain::new(backend.clone(), lock));
	(
		KeyringStore::new(keychain.clone()),
		KeyringVault::new(keychain),
	)
}

#[test]
fn identity_and_secrets_share_one_blob_and_roundtrip() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(SlowBackend::default());
	let (store, vault) = pair(&backend, &dir.path().join("k.lock"));
	store.save(&identity(3)).unwrap();
	vault.put(&[1; 16], "owner", b"key-bytes").unwrap();
	vault.put(&[2; 16], "password.1", b"pw").unwrap();

	assert_eq!(store.load().unwrap(), Some(identity(3)));
	assert_eq!(
		&**vault.get(&[1; 16], "owner").unwrap().unwrap(),
		b"key-bytes"
	);
	let blob = backend.inner.get().unwrap().unwrap();
	let text = String::from_utf8(blob.to_vec()).unwrap();
	assert!(text.contains("\"version\":1"));

	vault.delete(&[1; 16], "owner").unwrap();
	assert!(vault.get(&[1; 16], "owner").unwrap().is_none());
	assert!(vault.get(&[2; 16], "password.1").unwrap().is_some());
	assert_eq!(store.load().unwrap(), Some(identity(3)));
}

#[test]
fn emptying_the_bundle_removes_the_item_and_noops_do_not_write() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(SlowBackend::default());
	let (store, vault) = pair(&backend, &dir.path().join("k.lock"));
	vault.delete(&[1; 16], "missing").unwrap();
	store.delete().unwrap();
	assert_eq!(*backend.writes.lock().unwrap(), 0);
	store.save(&identity(1)).unwrap();
	store.delete().unwrap();
	assert_eq!(*backend.deletes.lock().unwrap(), 1);
	assert!(backend.inner.get().unwrap().is_none());
}

#[test]
fn concurrent_writers_never_lose_updates() {
	let dir = tempfile::tempdir().unwrap();
	let lock = dir.path().join("k.lock");
	let backend = Arc::new(SlowBackend::default());
	const THREADS: u8 = 8;
	const PER: u8 = 6;
	std::thread::scope(|scope| {
		for t in 0..THREADS {
			let backend = backend.clone();
			let lock = lock.clone();
			scope.spawn(move || {
				// Separate Keychain per thread: only the file lock keeps them apart,
				// exactly as two processes would be.
				let (store, vault) = pair(&backend, &lock);
				for i in 0..PER {
					vault.put(&[t; 16], &format!("slot{i}"), &[t, i]).unwrap();
				}
				store.save(&identity(t)).unwrap();
			});
		}
	});
	let (store, vault) = pair(&backend, &lock);
	for t in 0..THREADS {
		for i in 0..PER {
			let got = vault.get(&[t; 16], &format!("slot{i}")).unwrap().unwrap();
			assert_eq!(&**got, &[t, i], "lost update for thread {t} slot {i}");
		}
	}
	assert!(store.load().unwrap().is_some());
}

#[test]
fn shared_in_process_lock_is_used_for_threads_on_one_keychain() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(SlowBackend::default());
	let keychain = Arc::new(Keychain::new(backend, dir.path().join("k.lock")));
	std::thread::scope(|scope| {
		for t in 0..8u8 {
			let vault = KeyringVault::new(keychain.clone());
			scope.spawn(move || vault.put(&[t; 16], "s", &[t]).unwrap());
		}
	});
	let vault = KeyringVault::new(keychain);
	for t in 0..8u8 {
		assert!(vault.get(&[t; 16], "s").unwrap().is_some());
	}
}

#[test]
fn unknown_version_and_garbage_are_refused() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(SlowBackend::default());
	let (store, vault) = pair(&backend, &dir.path().join("k.lock"));
	backend.inner.set(br#"{"version":2}"#).unwrap();
	assert!(store.load().is_err());
	assert!(vault.put(&[1; 16], "s", b"x").is_err());
	backend.inner.set(b"not json").unwrap();
	assert!(store.load().is_err());
}

#[test]
fn bad_slot_names_are_rejected() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(SlowBackend::default());
	let (_store, vault) = pair(&backend, &dir.path().join("k.lock"));
	assert!(vault.put(&[1; 16], "Bad/Slot", b"x").is_err());
}

#[test]
fn keychain_debug_does_not_leak() {
	let dir = tempfile::tempdir().unwrap();
	let keychain = Keychain::new(
		Arc::new(MemoryBackend::default()),
		dir.path().join("k.lock"),
	);
	assert!(!format!("{keychain:?}").contains("secret"));
}
