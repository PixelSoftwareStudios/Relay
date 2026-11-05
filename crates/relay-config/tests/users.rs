mod support;

use relay_config::avatar::AvatarCache;
use relay_config::identity::TrustStore;
use relay_config::prefs::PreferenceStore;
use support::storage;

#[test]
fn two_os_users_never_see_each_others_data() {
	let alice_home = tempfile::tempdir().unwrap();
	let bob_home = tempfile::tempdir().unwrap();
	let alice = storage(alice_home.path());
	let bob = storage(bob_home.path());
	assert_ne!(alice.database.path(), bob.database.path());

	let mut prefs = PreferenceStore::open(&alice.database)
		.unwrap()
		.load()
		.unwrap()
		.preferences;
	prefs.profile.display_name = "Alice".into();
	PreferenceStore::open(&alice.database)
		.unwrap()
		.save(&prefs)
		.unwrap();
	TrustStore::open(&alice.database)
		.unwrap()
		.pin_tofu(&[1; 32], "Alice's friend", 1)
		.unwrap();
	AvatarCache::new(&alice.database)
		.set_local(&[3; 32])
		.unwrap();

	let bob_prefs = PreferenceStore::open(&bob.database)
		.unwrap()
		.load()
		.unwrap()
		.preferences;
	assert_ne!(bob_prefs.profile.display_name, "Alice");
	assert!(
		TrustStore::open(&bob.database)
			.unwrap()
			.entries()
			.is_empty()
	);
	assert_eq!(AvatarCache::new(&bob.database).size().unwrap(), 0);

	// Bob's writes do not leak into Alice's database either.
	TrustStore::open(&bob.database)
		.unwrap()
		.pin_tofu(&[2; 32], "Bob's friend", 1)
		.unwrap();
	let alice_trust = TrustStore::open(&alice.database).unwrap();
	assert_eq!(alice_trust.entries().len(), 1);
	assert_eq!(alice_trust.entries()[0].display_name, "Alice's friend");
}

#[cfg(unix)]
mod unix_permissions {
	use std::fs;
	use std::os::unix::fs::PermissionsExt;

	use relay_config::{Paths, Storage};

	fn mode(path: &std::path::Path) -> u32 {
		fs::metadata(path).unwrap().permissions().mode() & 0o777
	}

	#[test]
	fn files_and_dirs_are_owner_only() {
		let dir = tempfile::tempdir().unwrap();
		let paths = Paths::under(dir.path());
		let s = Storage::open(&paths).unwrap();
		s.database.refresh_backup(true);
		assert_eq!(mode(paths.data_dir()), 0o700);
		assert_eq!(mode(paths.config_dir()), 0o700);
		assert_eq!(mode(&paths.database_file()), 0o600);
		assert_eq!(mode(&paths.writer_lock_file()), 0o600);
		let mut bak = paths.database_file().into_os_string();
		bak.push(".bak");
		assert_eq!(mode(bak.as_ref()), 0o600);
		for suffix in ["-wal", "-shm"] {
			let mut side = paths.database_file().into_os_string();
			side.push(suffix);
			let side = std::path::PathBuf::from(side);
			if side.exists() {
				assert_eq!(mode(&side), 0o600, "{suffix}");
			}
		}
	}

	#[test]
	fn loose_permissions_are_tightened_on_open() {
		let dir = tempfile::tempdir().unwrap();
		let paths = Paths::under(dir.path());
		drop(Storage::open(&paths).unwrap());
		fs::set_permissions(paths.data_dir(), fs::Permissions::from_mode(0o755)).unwrap();
		fs::set_permissions(paths.database_file(), fs::Permissions::from_mode(0o644)).unwrap();
		drop(Storage::open(&paths).unwrap());
		assert_eq!(mode(paths.data_dir()), 0o700);
		assert_eq!(mode(&paths.database_file()), 0o600);
	}
}
