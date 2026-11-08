//! A database made by a build that had fewer tables is completed in place on open.

mod support;

use relay_config::favorites::Favorites;
use relay_config::identity::{TrustDecision, TrustLevel, TrustStore};
use support::{paths, storage};

fn key(n: u8) -> [u8; 32] {
	[n; 32]
}

#[test]
fn a_database_missing_tables_gains_them_and_keeps_its_trust() {
	let dir = tempfile::tempdir().unwrap();
	{
		let s = storage(dir.path());
		let mut trust = TrustStore::open(&s.database).unwrap();
		trust.pin_tofu(&key(1), "Laptop", 7).unwrap();
		trust.pin_tofu(&key(2), "Tablet", 8).unwrap();
	}
	// What an older build left behind: the same file without the newer tables.
	let conn = rusqlite::Connection::open(paths(dir.path()).database_file()).unwrap();
	conn.execute_batch("DROP TABLE favorites; DROP TABLE call_history;")
		.unwrap();
	drop(conn);

	let s = storage(dir.path());
	let mut trust = TrustStore::open(&s.database).unwrap();
	assert_eq!(trust.entries().len(), 2);
	assert_eq!(
		trust.lookup(&key(1)),
		TrustDecision::Known(TrustLevel::Tofu)
	);

	let favorites = Favorites::open(&s.database);
	assert!(favorites.list().unwrap().is_empty());
	favorites.set(&[key(1)]).unwrap();
	let entry = trust.replace_key(&key(1), &key(3)).unwrap();
	assert_eq!(entry.display_name, "Laptop");
	assert_eq!(favorites.list().unwrap(), vec![key(3)]);
	assert_eq!(trust.entries().len(), 2);
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
