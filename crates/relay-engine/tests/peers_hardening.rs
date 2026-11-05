//! Review F-04 (untrusted link cap), F-09 (decision key survives identity churn),
//! F-06/A4 (look-alike names), F-24 (claimed names are display data).

mod support_peers;

use std::sync::Arc;
use std::time::Duration;

use relay_config::WriterLock;
use relay_config::db::Database;
use relay_config::identity::TrustStore as ConfigStore;
use relay_engine::peers::{
	ConfigTrust, KeyedTrustStore, PeerOutput, SharedTrust, disambiguated, spawn,
};
use relay_net::identity::{Identity, TrustRecord, TrustState, TrustStore};
use support_peers::{Raw, expect, fast_config, peer};

fn config_trust(dir: &std::path::Path) -> ConfigTrust {
	let lock = Arc::new(WriterLock::acquire(&dir.join("writer.lock")).unwrap());
	let opened = Database::open(dir.join("relay.db"), lock).unwrap();
	ConfigTrust::new(ConfigStore::open(&opened.database).unwrap())
}

#[tokio::test]
async fn one_host_cannot_hold_more_than_its_share_of_untrusted_links() {
	let victim = peer("victim").await;
	let mut raws = Vec::new();
	for i in 0..8 {
		raws.push(Raw::connect(victim.handle.local_addr(), &format!("stranger {i}")).await);
	}
	tokio::time::sleep(Duration::from_millis(150)).await;
	let held = victim
		.handle
		.peers()
		.iter()
		.filter(|p| p.id.is_some())
		.count();
	assert!(
		(1..=2).contains(&held),
		"{held} untrusted links held from one source (cap is 2)"
	);
}

#[tokio::test]
async fn a_pending_decision_survives_churn_of_the_key_cache() {
	let dir = tempfile::tempdir().unwrap();
	let identity = Arc::new(Identity::generate().unwrap());
	let trust = SharedTrust::new(config_trust(dir.path()));
	let (handle, mut output) = spawn(fast_config("host"), identity, trust.clone(), [3; 16])
		.await
		.unwrap();
	let newcomer = peer("newcomer").await;
	let dial = newcomer.handle.dial(handle.local_addr(), None);
	let (_, ()) = tokio::join!(async { dial.await.unwrap() }, async {
		expect(&mut output, |o| {
			matches!(o, PeerOutput::TrustRequired { .. }).then_some(())
		})
		.await;
	});

	// Far more identities than the key cache holds are observed before the user answers.
	for _ in 0..200 {
		trust.lock().observe(Identity::generate().unwrap().public());
	}

	handle
		.pin(newcomer.id(), TrustState::UnverifiedFirstUse)
		.await
		.expect("the held link carries the key, so the decision still persists");
	assert!(trust.lock().get(&newcomer.identity.fingerprint()).is_some());
}

#[test]
fn trust_lookup_by_name_compares_confusable_skeletons() {
	let dir = tempfile::tempdir().unwrap();
	let alice = Identity::generate().unwrap();
	let mut trust = config_trust(dir.path());
	trust.observe(alice.public());
	trust
		.put(TrustRecord {
			fingerprint: alice.fingerprint(),
			state: TrustState::Verified,
			display_name: "Alice".to_owned(),
			trusted_at: std::time::SystemTime::now(),
		})
		.unwrap();
	// Cyrillic A, and a differently cased spelling.
	assert_eq!(trust.find_by_name("\u{410}lice").len(), 1);
	assert_eq!(trust.find_by_name("ALICE").len(), 1);
	assert!(trust.find_by_name("Bob").is_empty());
}

#[test]
fn a_name_another_key_also_claims_is_shown_with_the_fingerprint() {
	let (mine, other) = (Identity::generate().unwrap(), Identity::generate().unwrap());
	let (a, b) = (mine.fingerprint(), other.fingerprint());
	assert_eq!(disambiguated("Alice", &a, &[a]), "Alice");
	assert_eq!(disambiguated("Alice", &a, &[]), "Alice");
	let shown = disambiguated("Alice", &b, &[a]);
	assert!(
		shown.starts_with("Alice (") && shown.contains(&b.short()),
		"{shown}"
	);
}
