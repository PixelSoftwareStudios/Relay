//! Forgetting a stored trust decision by id, fingerprint prefix or name.

mod support_peers;

use std::time::SystemTime;

use relay_core::PeerEvent;
use relay_engine::peers::{PeerOutput, peer_id};
use relay_engine::service::{PeerSelector, PortError};
use relay_net::identity::{Identity, TrustRecord, TrustState};
use support_peers::{expect, linked_pair, peer};

fn record(name: &str) -> (Identity, TrustRecord) {
	let key = Identity::generate().unwrap();
	let record = TrustRecord {
		fingerprint: key.fingerprint(),
		state: TrustState::UnverifiedFirstUse,
		display_name: name.to_owned(),
		trusted_at: SystemTime::now(),
	};
	(key, record)
}

fn query(text: &str) -> PeerSelector {
	PeerSelector::Query(text.to_owned())
}

#[tokio::test]
async fn names_and_fingerprint_prefixes_pick_exactly_one_peer() {
	let node = peer("self").await;
	let (office, office_record) = record("Office");
	let (_, den_a) = record("Den");
	let (_, den_b) = record("den");
	let (laptop, laptop_record) = record("Laptop");
	for r in [&office_record, &den_a, &den_b, &laptop_record] {
		node.trust.lock().put(r.clone()).unwrap();
	}

	let ambiguous = node.handle.forget(&query("DEN")).unwrap_err();
	assert!(matches!(&ambiguous, PortError::Ambiguous(why) if why.contains("2 peers")));
	assert_eq!(
		node.handle.trusted().len(),
		4,
		"an ambiguous request changes nothing"
	);

	let gone = node.handle.forget(&query(" office ")).unwrap();
	assert_eq!(
		(gone.id, gone.name.as_str()),
		(peer_id(&office.fingerprint()), "Office")
	);
	assert_eq!(node.handle.trusted().len(), 3);

	// The short form shown in lists works, with its separators and case.
	let short = laptop.fingerprint().short();
	let gone = node.handle.forget(&query(&short[..14])).unwrap();
	assert_eq!(gone.name, "Laptop");

	assert!(matches!(
		node.handle.forget(&query("office")),
		Err(PortError::NotFound(_))
	));
	assert!(matches!(
		node.handle.forget(&query("")),
		Err(PortError::Invalid(_))
	));
	assert!(matches!(
		node.handle.forget(&query("abc123")),
		Err(PortError::NotFound(_))
	));
	let unknown = PeerSelector::Id(peer_id(&Identity::generate().unwrap().fingerprint()));
	assert!(matches!(
		node.handle.forget(&unknown),
		Err(PortError::NotFound(_))
	));

	// A full-length hex fingerprint selects by itself.
	let both = hex::encode(den_a.fingerprint.as_bytes());
	let by_prefix = node.handle.forget(&query(&both[..8])).unwrap();
	assert_eq!(by_prefix.id, peer_id(&den_a.fingerprint));
	assert_eq!(node.handle.trusted().len(), 1);
}

#[tokio::test]
async fn forgetting_a_linked_peer_cuts_its_link_and_trust() {
	let (mut a, b) = linked_pair().await;
	assert_eq!(a.handle.trusted().len(), 1);
	let gone = a.handle.forget(&PeerSelector::Id(b.id())).unwrap();
	assert_eq!(gone.id, b.id());
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkDown(id)) if *id == b.id()).then_some(())
	})
	.await;
	assert!(a.handle.trusted().is_empty());
	assert!(a.handle.peers().iter().all(|p| !p.linked));
}
