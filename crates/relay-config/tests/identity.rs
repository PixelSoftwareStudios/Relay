use std::sync::Arc;

use relay_config::fsx::WriterLock;
use relay_config::identity::{
	IdentityStore, InsecureFileStore, MemoryStore, Secret32, StoredIdentity, TrustDecision,
	TrustLevel, TrustStore, load_or_create, open_store,
};
use relay_config::prefs::{KeyStorage, Preferences};
use relay_config::{ConfigError, Notice, Paths};

fn identity(n: u8) -> StoredIdentity {
	StoredIdentity {
		secret: Secret32::new([n; 32]),
		public: [n.wrapping_add(1); 32],
	}
}

#[test]
fn secret_debug_is_redacted() {
	let text = format!("{:?}", identity(7));
	assert!(text.contains("redacted"));
	assert!(!text.contains("7, 7"));
}

#[test]
fn load_or_create_persists_once() {
	let store = MemoryStore::default();
	let first = load_or_create(&store, || identity(1)).unwrap();
	let second = load_or_create(&store, || identity(2)).unwrap();
	assert_eq!(first, second);
	store.delete().unwrap();
	assert!(store.load().unwrap().is_none());
}

#[test]
fn file_store_requires_explicit_setting() {
	let dir = tempfile::tempdir().unwrap();
	let err = InsecureFileStore::new(dir.path().join("k"), KeyStorage::OsCredentialStore);
	assert!(matches!(err, Err(ConfigError::InsecureStorageNotEnabled)));
}

#[test]
fn file_store_round_trips_with_private_mode_and_no_backup() {
	let dir = tempfile::tempdir().unwrap();
	let store = InsecureFileStore::new(dir.path().join("k.key"), KeyStorage::InsecureFile).unwrap();
	assert!(store.describe().contains("INSECURE"));
	store.save(&identity(3)).unwrap();
	store.save(&identity(4)).unwrap();
	assert_eq!(store.load().unwrap(), Some(identity(4)));
	assert!(!dir.path().join("k.key.bak").exists());
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let mode = std::fs::metadata(store.path())
			.unwrap()
			.permissions()
			.mode();
		assert_eq!(mode & 0o777, 0o600);
	}
	store.delete().unwrap();
	assert!(store.load().unwrap().is_none());
}

#[test]
fn malformed_key_file_is_an_error() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("k.key");
	std::fs::write(&path, b"short").unwrap();
	let store = InsecureFileStore::new(path, KeyStorage::InsecureFile).unwrap();
	assert!(matches!(
		store.load(),
		Err(ConfigError::IdentityMalformed(_))
	));
}

#[test]
fn open_store_honours_explicit_file_choice() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	let mut prefs = Preferences::default();
	prefs.identity.key_storage = KeyStorage::InsecureFile;
	let store = open_store(&prefs, &paths).unwrap();
	assert!(store.describe().contains("INSECURE"));
}

fn trust(dir: &tempfile::TempDir) -> (TrustStore, Vec<Notice>, Paths) {
	let paths = Paths::under(dir.path());
	let lock = Arc::new(WriterLock::acquire(&paths.writer_lock_file()).unwrap());
	let (store, notices) = TrustStore::open(paths.trust_file(), lock).unwrap();
	(store, notices, paths)
}

#[test]
fn trust_tofu_verify_remove_persist() {
	let dir = tempfile::tempdir().unwrap();
	let key = [9u8; 32];
	{
		let (mut store, notices, _) = trust(&dir);
		assert!(notices.is_empty());
		assert_eq!(store.lookup(&key), TrustDecision::Unknown);
		store.pin_tofu(&key, "Alice\u{7}", 100).unwrap();
		assert_eq!(store.entries()[0].display_name, "Alice");
		assert!(store.mark_verified(&key).unwrap());
		assert!(!store.mark_verified(&[1; 32]).unwrap());
	}
	let (mut store, _, _) = trust(&dir);
	assert_eq!(
		store.lookup(&key),
		TrustDecision::Known(TrustLevel::Verified)
	);
	// Re-pinning never downgrades a verified peer.
	store.pin_tofu(&key, "Mallory", 200).unwrap();
	assert_eq!(
		store.lookup(&key),
		TrustDecision::Known(TrustLevel::Verified)
	);
	assert!(store.remove(&key).unwrap());
	assert_eq!(store.lookup(&key), TrustDecision::Unknown);
}

#[test]
fn corrupt_trust_with_backup_recovers_and_notifies() {
	let dir = tempfile::tempdir().unwrap();
	let key = [5u8; 32];
	let paths = {
		let (mut store, _, paths) = trust(&dir);
		store.pin_tofu(&key, "A", 1).unwrap();
		store.pin_tofu(&[6; 32], "B", 2).unwrap(); // backup now holds A
		paths
	};
	std::fs::write(paths.trust_file(), b"\x00broken").unwrap();
	let (store, notices, _) = trust(&dir);
	assert_eq!(notices, vec![Notice::TrustRecoveredFromBackup]);
	assert!(matches!(store.lookup(&key), TrustDecision::Known(_)));
}

#[test]
fn corrupt_trust_without_backup_is_never_silent() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	std::fs::create_dir_all(paths.data_dir()).unwrap();
	std::fs::write(paths.trust_file(), b"???").unwrap();
	let (store, notices, _) = trust(&dir);
	assert!(store.entries().is_empty());
	match notices.as_slice() {
		[Notice::TrustStoreLost { preserved: Some(p) }] => assert!(p.exists()),
		other => panic!("unexpected {other:?}"),
	}
}

#[test]
fn trust_crash_mid_replace_missing_main_uses_backup() {
	let dir = tempfile::tempdir().unwrap();
	let key = [5u8; 32];
	let paths = {
		let (mut store, _, paths) = trust(&dir);
		store.pin_tofu(&key, "A", 1).unwrap();
		store.pin_tofu(&[6; 32], "B", 2).unwrap();
		paths
	};
	std::fs::remove_file(paths.trust_file()).unwrap();
	let (_, notices, _) = trust(&dir);
	assert_eq!(notices, vec![Notice::TrustRecoveredFromBackup]);
}
