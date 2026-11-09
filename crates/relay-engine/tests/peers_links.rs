mod support_peers;

use std::time::Duration;

use relay_core::{CallId, PeerEvent, PeerMessage, SessionId};
use relay_engine::peers::{PeerOutput, peer_id};
use relay_net::identity::{TrustRecord, TrustState};
use support_peers::{expect, fast_config, linked_pair, peer, peer_with};
use tokio::time::{sleep, timeout};

fn is_link_up(output: &PeerOutput) -> Option<()> {
	matches!(output, PeerOutput::Core(PeerEvent::LinkUp(_))).then_some(())
}

fn is_link_down(output: &PeerOutput) -> Option<()> {
	matches!(output, PeerOutput::Core(PeerEvent::LinkDown(_))).then_some(())
}

fn pre_trust(on: &support_peers::TestPeer, other: &support_peers::TestPeer, name: &str) {
	on.trust
		.lock()
		.put(TrustRecord {
			fingerprint: other.identity.fingerprint(),
			state: TrustState::UnverifiedFirstUse,
			display_name: name.to_owned(),
			trusted_at: std::time::SystemTime::now(),
		})
		.unwrap();
}

#[tokio::test]
async fn first_contact_holds_the_link_until_an_explicit_pin() {
	let a = peer("alpha").await;
	let mut b = peer("bravo").await;
	let dialled = a.handle.dial(b.handle.local_addr(), None).await.unwrap();
	assert_eq!(dialled, b.id());
	let named = expect(&mut b.output, |o| match o {
		PeerOutput::TrustRequired { name, peer, .. } => Some((name.clone(), *peer)),
		_ => None,
	})
	.await;
	assert_eq!(named, ("alpha".to_owned(), a.id()));
	// Nothing was persisted by the handshake.
	assert!(b.trust.lock().get(&a.identity.fingerprint()).is_none());
	// Signaling from the untrusted side never reaches the domain.
	a.handle
		.send(
			b.id(),
			PeerMessage::End {
				call: CallId::from_u128(1),
				session: SessionId::from_u128(2),
			},
		)
		.unwrap();
	assert!(
		timeout(Duration::from_millis(300), b.output.recv())
			.await
			.is_err()
	);
	b.handle
		.pin(a.id(), TrustState::UnverifiedFirstUse)
		.await
		.unwrap();
	expect(&mut b.output, is_link_up).await;
	assert!(b.trust.lock().get(&a.identity.fingerprint()).is_some());
}

#[tokio::test]
async fn call_messages_cross_a_trusted_link_with_session_mapping() {
	let (a, mut b) = linked_pair().await;
	let (call, session) = (CallId::from_u128(9), SessionId::from_u128(10));
	a.handle
		.send(
			b.id(),
			PeerMessage::Invite {
				call,
				session,
				allow_parallel_same_peer: false,
			},
		)
		.unwrap();
	let seen = expect(&mut b.output, |o| match o {
		PeerOutput::Core(PeerEvent::Invite { call, peer, .. }) => Some((*call, *peer)),
		_ => None,
	})
	.await;
	assert_eq!(seen, (call, a.id()));
}

#[tokio::test]
async fn messages_sent_before_the_link_exists_are_delivered_after_it_comes_up() {
	let mut a = peer("alpha").await;
	let mut b = peer("bravo").await;
	pre_trust(&a, &b, "bravo");
	pre_trust(&b, &a, "alpha");
	a.handle.add_address(b.id(), b.handle.local_addr()).unwrap();
	a.handle.ensure_link(b.id()).unwrap();
	let call = CallId::from_u128(77);
	a.handle
		.send(
			b.id(),
			PeerMessage::Invite {
				call,
				session: SessionId::from_u128(1),
				allow_parallel_same_peer: false,
			},
		)
		.unwrap();
	expect(&mut a.output, is_link_up).await;
	expect(&mut b.output, |o| match o {
		PeerOutput::Core(PeerEvent::Invite { call: c, .. }) => (*c == call).then_some(()),
		_ => None,
	})
	.await;
}

#[tokio::test]
async fn simultaneous_dial_converges_on_one_link_without_a_spurious_down() {
	let mut a = peer("alpha").await;
	let mut b = peer("bravo").await;
	pre_trust(&a, &b, "bravo");
	pre_trust(&b, &a, "alpha");
	a.handle.add_address(b.id(), b.handle.local_addr()).unwrap();
	b.handle.add_address(a.id(), a.handle.local_addr()).unwrap();
	a.handle.ensure_link(b.id()).unwrap();
	b.handle.ensure_link(a.id()).unwrap();
	expect(&mut a.output, is_link_up).await;
	expect(&mut b.output, is_link_up).await;
	sleep(Duration::from_millis(300)).await;
	// Traffic works in both directions and no side reported the peer down.
	let (call, session) = (CallId::from_u128(4), SessionId::from_u128(5));
	a.handle
		.send(b.id(), PeerMessage::End { call, session })
		.unwrap();
	b.handle
		.send(
			a.id(),
			PeerMessage::Invite {
				call: CallId::from_u128(8),
				session,
				allow_parallel_same_peer: false,
			},
		)
		.unwrap();
	expect(&mut a.output, |o| match o {
		PeerOutput::Core(PeerEvent::Invite { .. }) => Some(()),
		PeerOutput::Core(PeerEvent::LinkDown(_)) => panic!("spurious LinkDown on a"),
		_ => None,
	})
	.await;
	while let Ok(item) = b.output.try_recv() {
		assert!(is_link_down(&item).is_none(), "spurious LinkDown on b");
	}
}

#[tokio::test]
async fn a_different_key_under_a_trusted_name_is_refused() {
	let mut b = peer("bravo").await;
	let genuine = peer("alpha").await;
	pre_trust(&b, &genuine, "alpha");
	let impostor = peer("alpha").await;
	let result = impostor.handle.dial(b.handle.local_addr(), None).await;
	let _ = result;
	let previous = expect(&mut b.output, |o| match o {
		PeerOutput::KeyChanged { previous, peer, .. } => Some((*previous, *peer)),
		PeerOutput::Core(PeerEvent::LinkUp(_)) => panic!("impostor must not link up"),
		_ => None,
	})
	.await;
	assert_eq!(previous, (genuine.id(), impostor.id()));
	assert!(
		b.trust
			.lock()
			.get(&impostor.identity.fingerprint())
			.is_none()
	);
}

#[tokio::test]
async fn keepalives_hold_an_idle_link_open_past_the_idle_timeout() {
	let (mut a, mut b) = linked_pair().await;
	sleep(Duration::from_millis(1500)).await;
	while let Ok(item) = a.output.try_recv() {
		assert!(is_link_down(&item).is_none());
	}
	while let Ok(item) = b.output.try_recv() {
		assert!(is_link_down(&item).is_none());
	}
	assert!(a.handle.peers().iter().any(|p| p.linked));
}

#[tokio::test]
async fn link_loss_reports_down_and_reconnects_with_backoff() {
	let mut a = peer("alpha").await;
	let b = peer("bravo").await;
	pre_trust(&a, &b, "bravo");
	pre_trust(&b, &a, "alpha");
	let b_addr = b.handle.local_addr();
	let b_identity = b.identity.clone();
	a.handle.add_address(b.id(), b_addr).unwrap();
	a.handle.ensure_link(b.id()).unwrap();
	expect(&mut a.output, is_link_up).await;

	b.handle.shutdown();
	drop(b);
	expect(&mut a.output, is_link_down).await;

	// The peer comes back on the same port with the same identity.
	let mut config = fast_config("bravo");
	config.listen = b_addr;
	let mut revived = None;
	for _ in 0..50 {
		if let Ok(Ok(p)) = tokio::spawn(peer_with(config.clone(), b_identity.clone()))
			.await
			.map(Ok::<_, ()>)
		{
			revived = Some(p);
			break;
		}
		sleep(Duration::from_millis(50)).await;
	}
	let revived = revived.expect("rebind");
	pre_trust(&revived, &a, "alpha");
	expect(&mut a.output, is_link_up).await;
	assert_eq!(peer_id(&b_identity.fingerprint()), revived.id());
}

/// Resolves when the link ends (the far side closed it).
async fn closed(raw: &mut support_peers::Raw) {
	while raw.receiver.recv().await.is_ok() {}
}

/// A link replaced through the election stays open for a while: the peer may still be
/// completing the new handshake, and a link that vanished first would read as the peer going
/// away (a live call would be interrupted).
#[tokio::test]
async fn a_superseded_link_is_not_closed_before_the_peer_has_the_new_one() {
	use relay_net::identity::Identity;
	let a = peer("alpha").await;
	let addr = a.handle.local_addr();
	// Two links dialled by the same device tie on the dialler; the higher random epoch
	// wins, so the replacement happens about every other attempt.
	for attempt in 1..=40u8 {
		let device = || Identity::from_secret_bytes([attempt; 32]).unwrap();
		let mut first = support_peers::Raw::connect_as(device(), addr, "device").await;
		let mut second = support_peers::Raw::connect_as(device(), addr, "device").await;
		let outcome = timeout(Duration::from_millis(400), async {
			tokio::select! {
				() = closed(&mut first) => 1,
				() = closed(&mut second) => 2,
			}
		})
		.await;
		match outcome {
			// The new link lost the election: the old one carries on and the new one closes.
			Ok(2) => continue,
			// The old link was cut at once, ahead of the new one being settled.
			Ok(_) => panic!("the superseded link was closed immediately"),
			// The replacement happened and the old link is still held open.
			Err(_) => return,
		}
	}
	panic!("no replacement happened in 40 attempts");
}
