mod support;

use relay_config::blocklist::{BlockedDevice, Blocklist, MAX_BLOCKED};
use support::storage;

fn key(n: u16) -> [u8; 32] {
	let mut k = [0u8; 32];
	k[..2].copy_from_slice(&n.to_be_bytes());
	k
}

#[test]
fn blocked_devices_survive_reopen_and_unblock_removes_them() {
	let dir = tempfile::tempdir().unwrap();
	{
		let s = storage(dir.path());
		let list = Blocklist::open(&s.database);
		assert!(list.list().unwrap().is_empty());
		list.add(&key(1), "Spam").unwrap();
		list.add(&key(2), "Other").unwrap();
		list.add(&key(1), "Spam again").unwrap();
	}
	let s = storage(dir.path());
	let list = Blocklist::open(&s.database);
	let found = list.list().unwrap();
	assert_eq!(found.len(), 2);
	assert!(found.contains(&BlockedDevice {
		fingerprint: key(1),
		display_name: "Spam again".into()
	}));
	assert!(list.remove(&key(1)).unwrap());
	assert!(!list.remove(&key(1)).unwrap());
	assert_eq!(list.list().unwrap().len(), 1);
}

#[test]
fn the_list_is_bounded_and_an_existing_entry_still_updates() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let list = Blocklist::open(&s.database);
	for n in 0..MAX_BLOCKED as u16 {
		list.add(&key(n), "x").unwrap();
	}
	assert!(list.add(&key(9_999), "late").is_err());
	list.add(&key(0), "renamed").unwrap();
	assert_eq!(list.list().unwrap().len(), MAX_BLOCKED);
}

#[test]
fn a_long_name_is_cut_on_a_character_boundary() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let list = Blocklist::open(&s.database);
	list.add(&key(1), &"é".repeat(300)).unwrap();
	let name = list.list().unwrap().remove(0).display_name;
	assert!(name.len() <= 128 && name.chars().all(|c| c == 'é'));
}
