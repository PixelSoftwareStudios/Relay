//! Opening an existing database again changes nothing.

mod support;

use relay_config::identity::TrustStore;
use support::storage;

fn key(n: u8) -> [u8; 32] {
	[n; 32]
}

#[test]
fn opening_twice_changes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	for _ in 0..3 {
		let s = storage(dir.path());
		let mut trust = TrustStore::open(&s.database).unwrap();
		trust.pin_tofu(&key(1), "Laptop", 7).unwrap();
		assert_eq!(trust.entries().len(), 1);
	}
}
