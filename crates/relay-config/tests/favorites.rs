mod support;

use relay_config::ConfigError;
use relay_config::favorites::{DUPLICATE, FULL, Favorites, MAX_FAVORITES, NOT_TRUSTED};
use relay_config::identity::TrustStore;
use support::storage;

fn key(n: u8) -> [u8; 32] {
	[n; 32]
}

fn reason(error: ConfigError) -> String {
	error.to_string()
}

fn trusted(dir: &std::path::Path, count: u8) -> (relay_config::Storage, TrustStore) {
	let s = storage(dir);
	let mut trust = TrustStore::open(&s.database).unwrap();
	for n in 1..=count {
		trust
			.pin_tofu(&key(n), &format!("Peer {n}"), u64::from(n))
			.unwrap();
	}
	(s, trust)
}

#[test]
fn the_list_keeps_its_order_across_reopen() {
	let dir = tempfile::tempdir().unwrap();
	{
		let (s, _) = trusted(dir.path(), 4);
		let favorites = Favorites::open(&s.database);
		assert!(favorites.list().unwrap().is_empty());
		favorites.set(&[key(3), key(1), key(4)]).unwrap();
	}
	let s = storage(dir.path());
	assert_eq!(
		Favorites::open(&s.database).list().unwrap(),
		vec![key(3), key(1), key(4)]
	);
}

#[test]
fn setting_again_reorders_and_removes() {
	let dir = tempfile::tempdir().unwrap();
	let (s, _) = trusted(dir.path(), 3);
	let favorites = Favorites::open(&s.database);
	favorites.set(&[key(1), key(2), key(3)]).unwrap();
	favorites.set(&[key(3), key(1)]).unwrap();
	assert_eq!(favorites.list().unwrap(), vec![key(3), key(1)]);
	favorites.set(&[]).unwrap();
	assert!(favorites.list().unwrap().is_empty());
}

#[test]
fn at_most_six_and_a_refusal_changes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	let (s, _) = trusted(dir.path(), 7);
	let favorites = Favorites::open(&s.database);
	let six: Vec<_> = (1..=6).map(key).collect();
	assert_eq!(six.len(), MAX_FAVORITES);
	favorites.set(&six).unwrap();
	let seven: Vec<_> = (1..=7).map(key).collect();
	assert_eq!(reason(favorites.set(&seven).unwrap_err()), FULL);
	assert_eq!(favorites.list().unwrap(), six);
}

#[test]
fn only_trusted_peers_and_no_duplicates() {
	let dir = tempfile::tempdir().unwrap();
	let (s, _) = trusted(dir.path(), 2);
	let favorites = Favorites::open(&s.database);
	favorites.set(&[key(1)]).unwrap();
	assert_eq!(
		reason(favorites.set(&[key(1), key(9)]).unwrap_err()),
		NOT_TRUSTED
	);
	assert_eq!(
		reason(favorites.set(&[key(2), key(2)]).unwrap_err()),
		DUPLICATE
	);
	assert_eq!(favorites.list().unwrap(), vec![key(1)]);
}

#[test]
fn removing_trust_removes_the_favorite_and_keeps_the_order() {
	let dir = tempfile::tempdir().unwrap();
	let (s, mut trust) = trusted(dir.path(), 4);
	let favorites = Favorites::open(&s.database);
	favorites.set(&[key(4), key(2), key(1), key(3)]).unwrap();
	assert!(trust.remove(&key(2)).unwrap());
	assert_eq!(favorites.list().unwrap(), vec![key(4), key(1), key(3)]);
	// The freed slot can be used again, at the end.
	favorites.set(&[key(4), key(1), key(3)]).unwrap();
	assert!(trust.remove(&key(4)).unwrap());
	assert_eq!(favorites.list().unwrap(), vec![key(1), key(3)]);
}
