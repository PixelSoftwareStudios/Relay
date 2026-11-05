mod support;

use relay_config::identity::{TrustDecision, TrustLevel, TrustStore};
use relay_config::{Notice, Paths, Storage};
use support::{paths, storage};

#[test]
fn tofu_verify_remove_persist_across_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let key = [9u8; 32];
	{
		let s = storage(dir.path());
		let mut store = TrustStore::open(&s.database).unwrap();
		assert_eq!(store.lookup(&key), TrustDecision::Unknown);
		store.pin_tofu(&key, "Alice\u{7}", 100).unwrap();
		assert_eq!(store.entries()[0].display_name, "Alice");
		assert!(store.mark_verified(&key).unwrap());
		assert!(!store.mark_verified(&[1; 32]).unwrap());
	}
	let s = storage(dir.path());
	let mut store = TrustStore::open(&s.database).unwrap();
	assert_eq!(
		store.lookup(&key),
		TrustDecision::Known(TrustLevel::Verified)
	);
	assert_eq!(store.entries()[0].first_seen, 100);
	// Re-pinning never downgrades a verified peer.
	store.pin_tofu(&key, "Mallory", 200).unwrap();
	assert_eq!(
		store.lookup(&key),
		TrustDecision::Known(TrustLevel::Verified)
	);
	assert!(store.remove(&key).unwrap());
	assert!(!store.remove(&key).unwrap());
	assert_eq!(store.lookup(&key), TrustDecision::Unknown);
}

#[test]
fn corrupt_database_recovers_pinned_keys_from_backup_with_notice() {
	let dir = tempfile::tempdir().unwrap();
	let key = [5u8; 32];
	{
		let s = storage(dir.path());
		TrustStore::open(&s.database)
			.unwrap()
			.pin_tofu(&key, "A", 1)
			.unwrap();
	}
	let p: Paths = paths(dir.path());
	std::fs::write(p.database_file(), b"\x00broken").unwrap();
	let s = Storage::open(&p).unwrap();
	assert!(s.notices.contains(&Notice::TrustRecoveredFromBackup));
	let store = TrustStore::open(&s.database).unwrap();
	assert!(matches!(store.lookup(&key), TrustDecision::Known(_)));
}

#[test]
fn lost_trust_is_never_silent() {
	let dir = tempfile::tempdir().unwrap();
	let p = paths(dir.path());
	std::fs::create_dir_all(p.data_dir()).unwrap();
	std::fs::write(p.database_file(), b"???").unwrap();
	let s = Storage::open(&p).unwrap();
	assert!(
		s.notices
			.iter()
			.any(|n| matches!(n, Notice::TrustStoreLost { preserved: Some(p) } if p.exists()))
	);
	assert!(TrustStore::open(&s.database).unwrap().entries().is_empty());
}
