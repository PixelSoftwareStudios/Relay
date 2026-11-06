use std::sync::Arc;

use relay_config::WriterLock;
use relay_config::db::Database;
use relay_config::identity::TrustStore as ConfigStore;
use relay_engine::peers::{ConfigTrust, KeyedTrustStore};
use relay_net::identity::{
	Identity, PeerTrust, TrustError, TrustRecord, TrustState, TrustStore, evaluate_peer,
};

fn open(dir: &std::path::Path) -> ConfigTrust {
	let lock = Arc::new(WriterLock::acquire(&dir.join("writer.lock")).unwrap());
	let opened = Database::open(dir.join("relay.db"), lock).unwrap();
	assert!(opened.notices.is_empty());
	ConfigTrust::new(ConfigStore::open(&opened.database).unwrap())
}

fn record(identity: &Identity, name: &str, state: TrustState) -> TrustRecord {
	TrustRecord {
		fingerprint: identity.fingerprint(),
		state,
		display_name: name.to_owned(),
		trusted_at: std::time::SystemTime::now(),
	}
}

#[test]
fn nothing_is_persisted_by_observation_only() {
	let dir = tempfile::tempdir().unwrap();
	let peer = Identity::generate().unwrap();
	let mut trust = open(dir.path());
	trust.observe(peer.public());
	assert_eq!(
		evaluate_peer(&trust, &peer.fingerprint(), "p"),
		PeerTrust::New
	);
	drop(trust);
	assert!(open(dir.path()).get(&peer.fingerprint()).is_none());
}

#[test]
fn an_explicit_put_persists_and_survives_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let peer = Identity::generate().unwrap();
	let mut trust = open(dir.path());
	trust.observe(peer.public());
	trust
		.put(record(&peer, "kitchen", TrustState::UnverifiedFirstUse))
		.unwrap();
	drop(trust);

	let mut trust = open(dir.path());
	assert_eq!(
		evaluate_peer(&trust, &peer.fingerprint(), "x"),
		PeerTrust::UnverifiedFirstUse
	);
	// Upgrading to verified needs the key again in this process.
	assert_eq!(
		trust.put(record(&peer, "kitchen", TrustState::Verified)),
		Err(TrustError::Unknown)
	);
	trust.observe(peer.public());
	trust
		.put(record(&peer, "kitchen", TrustState::Verified))
		.unwrap();
	assert_eq!(
		evaluate_peer(&trust, &peer.fingerprint(), "x"),
		PeerTrust::Verified
	);
}

#[test]
fn a_digest_alone_cannot_be_pinned() {
	let dir = tempfile::tempdir().unwrap();
	let peer = Identity::generate().unwrap();
	let mut trust = open(dir.path());
	assert_eq!(
		trust.put(record(&peer, "ghost", TrustState::UnverifiedFirstUse)),
		Err(TrustError::Unknown)
	);
}

#[test]
fn a_new_key_under_a_pinned_name_is_a_key_change_and_remove_works() {
	let dir = tempfile::tempdir().unwrap();
	let (old, new) = (Identity::generate().unwrap(), Identity::generate().unwrap());
	let mut trust = open(dir.path());
	trust.observe(old.public());
	trust
		.put(record(&old, "desk", TrustState::UnverifiedFirstUse))
		.unwrap();
	assert!(matches!(
		evaluate_peer(&trust, &new.fingerprint(), "desk"),
		PeerTrust::KeyChanged { .. }
	));
	trust.remove(&old.fingerprint()).unwrap();
	assert_eq!(trust.remove(&old.fingerprint()), Err(TrustError::Unknown));
	assert_eq!(
		evaluate_peer(&trust, &new.fingerprint(), "desk"),
		PeerTrust::New
	);
}

#[test]
fn a_database_fault_is_never_reported_as_a_full_store() {
	let dir = tempfile::tempdir().unwrap();
	let lock = Arc::new(WriterLock::acquire(&dir.path().join("writer.lock")).unwrap());
	let opened = Database::open(dir.path().join("relay.db"), lock).unwrap();
	let mut trust = ConfigTrust::new(ConfigStore::open(&opened.database).unwrap());
	let (old, new) = (Identity::generate().unwrap(), Identity::generate().unwrap());
	for peer in [&old, &new] {
		trust.observe(peer.public());
	}
	trust
		.put(record(&old, "laptop", TrustState::UnverifiedFirstUse))
		.unwrap();
	opened
		.database
		.with(|c| Ok(c.execute_batch("DROP TABLE favorites")?))
		.unwrap();
	let error = trust
		.replace(&old.fingerprint(), &new.fingerprint())
		.unwrap_err();
	assert!(
		matches!(&error, TrustError::Storage(detail) if detail.contains("favorites")),
		"{error:?}"
	);
	assert!(!error.to_string().contains("limit"));
}

#[test]
fn replacing_with_an_already_trusted_key_says_so() {
	let dir = tempfile::tempdir().unwrap();
	let (old, new) = (Identity::generate().unwrap(), Identity::generate().unwrap());
	let mut trust = open(dir.path());
	for peer in [&old, &new] {
		trust.observe(peer.public());
		trust
			.put(record(peer, "x", TrustState::UnverifiedFirstUse))
			.unwrap();
	}
	assert_eq!(
		trust.replace(&old.fingerprint(), &new.fingerprint()),
		Err(TrustError::Exists)
	);
}
