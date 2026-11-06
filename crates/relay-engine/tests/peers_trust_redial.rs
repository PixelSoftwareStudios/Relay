//! Trusting a device whose probe link is gone: the decision dials it again with its key
//! pinned, never pins another key, and reports a failure instead of swallowing it.

mod support_peers;

use std::time::Duration;

use relay_core::PeerEvent;
use relay_engine::peers::{PeerError, PeerOutput, peer_id};
use relay_net::identity::{Identity, TrustState};
use support_peers::{TestPeer, expect, peer};
use tokio::time::{sleep, timeout};

/// `b` met `a` (untrusted link on both sides), then `a` let go of it: `b` holds no link.
async fn met_then_dropped() -> (TestPeer, TestPeer) {
	let mut a = peer("alpha").await;
	let b = peer("bravo").await;
	let found = b.handle.dial(a.handle.local_addr(), None).await.unwrap();
	assert_eq!(found, a.id());
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::TrustRequired { .. }).then_some(())
	})
	.await;
	// `a` trusts `b`, then forgets it: that closes the link and leaves `a` listening.
	a.handle
		.pin(b.id(), TrustState::UnverifiedFirstUse)
		.await
		.unwrap();
	a.handle
		.forget(&relay_engine::service::PeerSelector::Id(b.id()))
		.unwrap();
	timeout(Duration::from_secs(5), async {
		while b.handle.peers().iter().any(|p| p.id == Some(a.id())) {
			sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("the link goes away");
	assert!(b.handle.trusted().is_empty());
	(a, b)
}

#[tokio::test]
async fn trusting_without_a_link_dials_the_device_again_and_pins_its_key() {
	let (a, mut b) = met_then_dropped().await;
	assert!(matches!(
		b.handle.pin(a.id(), TrustState::UnverifiedFirstUse).await,
		Err(PeerError::UnknownKey)
	));
	b.handle.add_address(a.id(), a.handle.local_addr()).unwrap();
	b.handle
		.pin_reaching(a.id(), TrustState::UnverifiedFirstUse)
		.await
		.expect("redial then pin");
	assert!(b.handle.trusted().contains(&a.identity.fingerprint()));
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(p)) if *p == a.id()).then_some(())
	})
	.await;
}

#[tokio::test]
async fn another_key_at_the_address_is_never_trusted() {
	let (_a, b) = met_then_dropped().await;
	let impostor = peer("impostor").await;
	let wanted = peer_id(&Identity::generate().unwrap().fingerprint());
	b.handle
		.add_address(wanted, impostor.handle.local_addr())
		.unwrap();
	let refused = b
		.handle
		.pin_reaching(wanted, TrustState::UnverifiedFirstUse)
		.await;
	assert!(
		matches!(refused, Err(PeerError::KeyMismatch)),
		"{refused:?}"
	);
	assert!(b.handle.trusted().is_empty());
	assert!(
		!b.handle
			.trusted()
			.contains(&impostor.identity.fingerprint())
	);
}

#[tokio::test]
async fn an_unreachable_device_is_an_error() {
	let (a, b) = met_then_dropped().await;
	let none = b
		.handle
		.pin_reaching(a.id(), TrustState::UnverifiedFirstUse)
		.await;
	assert!(matches!(none, Err(PeerError::UnknownKey)), "{none:?}");
	let closed = {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		listener.local_addr().unwrap()
	};
	b.handle.add_address(a.id(), closed).unwrap();
	let failed = b
		.handle
		.pin_reaching(a.id(), TrustState::UnverifiedFirstUse)
		.await;
	assert!(failed.is_err(), "{failed:?}");
	assert!(b.handle.trusted().is_empty());
}
