//! A new identity is generated only when the store says none exists.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_config::identity::{
	IdentityStore, InsecureFileStore, KeyringStore, Secret32, StoredIdentity, load_or_create,
};
use relay_config::keychain::{Keychain, MemoryBackend, SecretBackend};
use relay_config::prefs::KeyStorage;
use zeroize::Zeroizing;

fn identity(n: u8) -> StoredIdentity {
	StoredIdentity {
		secret: Secret32::new([n; 32]),
		public: [n.wrapping_add(1); 32],
	}
}

/// A keyring whose reads can be made to fail the way a denied prompt does.
#[derive(Default)]
struct Flaky {
	inner: MemoryBackend,
	fail_reads: Mutex<Option<String>>,
	writes: AtomicUsize,
}

impl SecretBackend for Flaky {
	fn get(&self) -> relay_config::Result<Option<Zeroizing<Vec<u8>>>> {
		if let Some(why) = self.fail_reads.lock().unwrap().clone() {
			return Err(relay_config::ConfigError::CredentialStore(why));
		}
		self.inner.get()
	}
	fn set(&self, blob: &[u8]) -> relay_config::Result<()> {
		self.writes.fetch_add(1, Ordering::SeqCst);
		self.inner.set(blob)
	}
	fn delete(&self) -> relay_config::Result<()> {
		self.writes.fetch_add(1, Ordering::SeqCst);
		self.inner.delete()
	}
}

fn store(backend: &Arc<Flaky>, dir: &std::path::Path) -> KeyringStore {
	KeyringStore::new(Arc::new(Keychain::new(backend.clone(), dir.join("k.lock"))))
}

#[test]
fn a_failed_read_generates_nothing_and_writes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Flaky::default());
	let keyring = store(&backend, dir.path());
	keyring.save(&identity(5)).unwrap();
	let before = backend.inner.get().unwrap().unwrap().to_vec();
	let writes = backend.writes.load(Ordering::SeqCst);

	*backend.fail_reads.lock().unwrap() = Some("user denied".into());
	let generated = AtomicUsize::new(0);
	let result = load_or_create(&keyring, || {
		generated.fetch_add(1, Ordering::SeqCst);
		Ok(identity(9))
	});
	assert!(result.is_err());
	assert_eq!(generated.load(Ordering::SeqCst), 0);
	assert_eq!(backend.writes.load(Ordering::SeqCst), writes);
	assert_eq!(backend.inner.get().unwrap().unwrap().to_vec(), before);

	*backend.fail_reads.lock().unwrap() = None;
	assert_eq!(keyring.load().unwrap(), Some(identity(5)));
}

#[test]
fn missing_generates_exactly_once() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Flaky::default());
	let keyring = store(&backend, dir.path());
	let generated = AtomicUsize::new(0);
	let make = || {
		generated.fetch_add(1, Ordering::SeqCst);
		Ok(identity(7))
	};
	let (first, created) = load_or_create(&keyring, make).unwrap();
	assert!(created);
	let (second, created) = load_or_create(&keyring, make).unwrap();
	assert!(!created);
	assert_eq!(first, second);
	assert_eq!(generated.load(Ordering::SeqCst), 1);
}

#[test]
fn a_corrupt_bundle_is_refused_and_left_alone() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Flaky::default());
	backend.inner.set(b"{ not a bundle").unwrap();
	let keyring = store(&backend, dir.path());
	assert!(load_or_create(&keyring, || Ok(identity(1))).is_err());
	assert!(keyring.create(&identity(1)).is_err());
	assert_eq!(
		backend.inner.get().unwrap().unwrap().as_slice(),
		b"{ not a bundle"
	);

	backend
		.inner
		.set(br#"{"version":99,"identity":null,"channels":{}}"#)
		.unwrap();
	assert!(load_or_create(&keyring, || Ok(identity(1))).is_err());
}

#[test]
fn concurrent_first_starts_create_one_identity() {
	let dir = tempfile::tempdir().unwrap();
	let backend = Arc::new(Flaky::default());
	let lock = dir.path().join("k.lock");
	let threads: Vec<_> = (1..=6u8)
		.map(|n| {
			// Separate handles, as separate processes would have: only the file lock joins them.
			let keyring = KeyringStore::new(Arc::new(Keychain::new(backend.clone(), &lock)));
			std::thread::spawn(move || load_or_create(&keyring, || Ok(identity(n))).unwrap().0)
		})
		.collect();
	let found: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
	assert!(found.windows(2).all(|pair| pair[0] == pair[1]));
	assert_eq!(backend.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn the_file_store_creates_only_if_absent() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("identity.key");
	let file = InsecureFileStore::new(&path, KeyStorage::InsecureFile).unwrap();
	assert_eq!(file.create(&identity(1)).unwrap(), identity(1));
	assert_eq!(file.create(&identity(2)).unwrap(), identity(1));
	assert_eq!(file.load().unwrap(), Some(identity(1)));
}
