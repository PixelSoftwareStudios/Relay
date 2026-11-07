//! Trusted contacts stay in the peer list while they are neither linked nor discovered.

mod support_peers;

use std::time::{Duration, SystemTime};

use relay_engine::peers::peer_id;
use relay_engine::service::PeerSelector;
use relay_net::identity::{Identity, TrustRecord, TrustState};
use support_peers::peer;

async fn listed(node: &support_peers::TestPeer, id: relay_core::PeerId) -> Option<bool> {
	for _ in 0..100 {
		if let Some(found) = node.handle.peers().iter().find(|p| p.id == Some(id)) {
			return Some(found.callable);
		}
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
	None
}

#[tokio::test]
async fn a_trusted_contact_that_is_offline_is_still_listed_with_its_name() {
	let node = peer("self").await;
	let other = Identity::generate().unwrap();
	node.trust
		.lock()
		.put(TrustRecord {
			fingerprint: other.fingerprint(),
			state: TrustState::Verified,
			display_name: "Office".to_owned(),
			trusted_at: SystemTime::now(),
		})
		.unwrap();
	let id = peer_id(&other.fingerprint());
	assert_eq!(listed(&node, id).await, Some(false), "listed, not callable");
	let found = node.handle.peers();
	let found = found.iter().find(|p| p.id == Some(id)).unwrap();
	assert_eq!(found.name, "Office");
	assert_eq!(found.trust, Some(TrustState::Verified));
	assert!(!found.linked);
}

#[tokio::test]
async fn forgetting_an_offline_contact_removes_it() {
	let node = peer("self").await;
	let other = Identity::generate().unwrap();
	node.trust
		.lock()
		.put(TrustRecord {
			fingerprint: other.fingerprint(),
			state: TrustState::Verified,
			display_name: "Office".to_owned(),
			trusted_at: SystemTime::now(),
		})
		.unwrap();
	let id = peer_id(&other.fingerprint());
	assert!(listed(&node, id).await.is_some());
	node.handle.forget(&PeerSelector::Id(id)).unwrap();
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert!(node.handle.peers().iter().all(|p| p.id != Some(id)));
}
