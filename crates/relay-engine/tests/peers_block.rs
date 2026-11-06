//! The block list: a blocked key is refused without a prompt or a listing, blocking a
//! contact forgets it, and unblocking lets the device be met again.

mod support_peers;

use std::time::Duration;

use relay_engine::peers::{Blocks, PeerOutput};
use relay_engine::service::PeerSelector;
use support_peers::{fast_config, linked_pair, peer, peer_with};
use tokio::time::{sleep, timeout};

async fn prompted(b: &mut support_peers::TestPeer, wait: Duration) -> bool {
	timeout(wait, async {
		loop {
			if let Some(PeerOutput::TrustRequired { .. }) = b.output.recv().await {
				return;
			}
		}
	})
	.await
	.is_ok()
}

#[tokio::test]
async fn a_blocked_device_gets_no_prompt_and_no_listing_until_unblocked() {
	let a = peer("alpha").await;
	let mut config = fast_config("bravo");
	config.blocks = Blocks::in_memory();
	let mut b = peer_with(
		config,
		std::sync::Arc::new(relay_net::identity::Identity::generate().unwrap()),
	)
	.await;
	b.handle.block(a.id()).unwrap();
	assert_eq!(b.handle.blocked().len(), 1);

	let _ = a.handle.dial(b.handle.local_addr(), None).await;
	assert!(
		!prompted(&mut b, Duration::from_millis(700)).await,
		"no prompt"
	);
	assert!(b.handle.peers().iter().all(|p| p.id != Some(a.id())));

	// Unblocking is forgetting the id.
	let gone = b.handle.forget(&PeerSelector::Id(a.id())).unwrap();
	assert_eq!(gone.id, a.id());
	assert!(b.handle.blocked().is_empty());
	a.handle.dial(b.handle.local_addr(), None).await.unwrap();
	assert!(prompted(&mut b, Duration::from_secs(3)).await, "met again");
}

#[tokio::test]
async fn blocking_a_contact_forgets_it_and_cuts_the_link() {
	let (a, b) = linked_pair().await;
	assert!(b.handle.trusted().contains(&a.identity.fingerprint()));
	b.handle.block(a.id()).unwrap();
	assert!(b.handle.trusted().is_empty(), "the contact is gone");
	timeout(Duration::from_secs(3), async {
		while b.handle.peers().iter().any(|p| p.id == Some(a.id())) {
			sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("the link is cut");
	// Unblocking does not make it a contact again.
	b.handle.forget(&PeerSelector::Id(a.id())).unwrap();
	assert!(b.handle.trusted().is_empty());
}

#[tokio::test]
async fn the_list_is_bounded() {
	let node = peer("self").await;
	for n in 0..relay_config::blocklist::MAX_BLOCKED {
		let mut id = [0u8; 32];
		id[..2].copy_from_slice(&(n as u16).to_be_bytes());
		node.handle
			.block(relay_core::PeerId::from_bytes(id))
			.unwrap();
	}
	assert!(
		node.handle
			.block(relay_core::PeerId::from_bytes([255; 32]))
			.is_err()
	);
}
