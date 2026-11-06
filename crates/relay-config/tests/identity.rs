use relay_config::identity::{
	IdentityStore, InsecureFileStore, MemoryStore, Secret32, StoredIdentity, load_or_create,
	open_store,
};
use relay_config::prefs::{KeyStorage, Preferences};
use relay_config::{ConfigError, Paths};

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
	let (first, made) = load_or_create(&store, || Ok(identity(1))).unwrap();
	let (second, made_again) = load_or_create(&store, || Ok(identity(2))).unwrap();
	assert!(made && !made_again);
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
