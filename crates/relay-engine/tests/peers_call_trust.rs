//! Calling a device nobody trusted yet: the call works over the untrusted link, and the
//! key is pinned (as first use, never verified) only when the call is answered.

mod support_peers;

use std::time::Duration;

use relay_core::{CallId, PeerEvent, PeerMessage, SessionId};
use relay_engine::peers::PeerOutput;
use relay_net::identity::{Fingerprint, TrustState};
use support_peers::{Raw, TestPeer, expect, visible_peer};
use tokio::time::timeout;

const CALL: u128 = 0x1234;
const SESSION: u128 = 0x99;

fn invite() -> PeerMessage {
	PeerMessage::Invite {
		call: CallId::from_u128(CALL),
		session: SessionId::from_u128(SESSION),
		allow_parallel_same_peer: false,
	}
}

fn state_of(node: &TestPeer, other: &Fingerprint) -> Option<TrustState> {
	node.trust.lock().get(other).map(|r| r.state)
}

/// `a` met `b` (both visible): an untrusted link, announced to both domains.
async fn met() -> (TestPeer, TestPeer) {
	let (mut a, _sa) = visible_peer("alpha", false).await;
	let (mut b, _sb) = visible_peer("bravo", false).await;
	std::mem::forget(_sa);
	std::mem::forget(_sb);
	a.handle.dial(b.handle.local_addr(), None).await.unwrap();
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(_))).then_some(())
	})
	.await;
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(_))).then_some(())
	})
	.await;
	(a, b)
}

#[tokio::test]
async fn a_call_to_an_unknown_device_pins_both_keys_only_when_answered() {
	let (mut a, mut b) = met().await;
	a.handle.send(b.id(), invite()).unwrap();
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Invite { peer, .. }) if *peer == a.id())
			.then_some(())
	})
	.await;
	// Ringing is not trust.
	assert!(state_of(&a, &b.identity.fingerprint()).is_none());
	assert!(state_of(&b, &a.identity.fingerprint()).is_none());

	let accept = PeerMessage::Accept {
		call: CallId::from_u128(CALL),
		session: SessionId::from_u128(SESSION),
	};
	b.handle.send(a.id(), accept).unwrap();
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Accepted { .. })).then_some(())
	})
	.await;
	// Both sides hold the other's authenticated key as first-use trust, never verified.
	assert_eq!(
		state_of(&a, &b.identity.fingerprint()),
		Some(TrustState::UnverifiedFirstUse)
	);
	assert_eq!(
		state_of(&b, &a.identity.fingerprint()),
		Some(TrustState::UnverifiedFirstUse)
	);
	assert!(
		a.handle
			.peers()
			.iter()
			.any(|p| p.linked && p.id == Some(b.id()))
	);
}

#[tokio::test]
async fn a_declined_call_pins_nothing() {
	let (mut a, mut b) = met().await;
	a.handle.send(b.id(), invite()).unwrap();
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Invite { .. })).then_some(())
	})
	.await;
	let decline = PeerMessage::Decline {
		call: CallId::from_u128(CALL),
		session: SessionId::from_u128(SESSION),
		busy: false,
	};
	b.handle.send(a.id(), decline).unwrap();
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Declined { .. })).then_some(())
	})
	.await;
	assert!(a.handle.trusted().is_empty() && b.handle.trusted().is_empty());
}

#[tokio::test]
async fn a_cancelled_call_pins_nothing() {
	let (a, mut b) = met().await;
	a.handle.send(b.id(), invite()).unwrap();
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Invite { .. })).then_some(())
	})
	.await;
	let end = PeerMessage::End {
		call: CallId::from_u128(CALL),
		session: SessionId::from_u128(SESSION),
	};
	a.handle.send(b.id(), end).unwrap();
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::Ended { .. })).then_some(())
	})
	.await;
	assert!(a.handle.trusted().is_empty() && b.handle.trusted().is_empty());
}

#[tokio::test]
async fn a_private_device_takes_no_call_from_a_stranger() {
	let (a, _sa) = visible_peer("alpha", false).await;
	let (mut b, _sb) = visible_peer("bravo", true).await;
	a.handle.dial(b.handle.local_addr(), None).await.unwrap();
	a.handle.send(b.id(), invite()).unwrap();
	let rang = timeout(Duration::from_millis(700), async {
		loop {
			if let Some(PeerOutput::Core(PeerEvent::Invite { .. })) = b.output.recv().await {
				return;
			}
		}
	})
	.await
	.is_ok();
	assert!(!rang, "hidden means unreachable for strangers");
	assert!(b.handle.trusted().is_empty());
}

#[tokio::test]
async fn another_device_cannot_answer_for_the_callee() {
	let (mut a, _sa) = visible_peer("alpha", false).await;
	let (b, _sb) = visible_peer("bravo", false).await;
	a.handle.dial(b.handle.local_addr(), None).await.unwrap();
	a.handle.send(b.id(), invite()).unwrap();
	// An impostor with its own key connects to `a` and sends an Accept for that call.
	let mut raw = Raw::connect(a.handle.local_addr(), "impostor").await;
	let accept = relay_net::control::new_envelope(relay_net::proto::envelope::Body::Accept(
		relay_net::proto::Accept {
			call_id: CALL.to_be_bytes().to_vec(),
			selected_profile: relay_engine::peers::PROFILE.to_owned(),
			send_ssrc: 0,
			initial_stream_generation: 0,
		},
	));
	raw.sender.send(&accept).await.unwrap();
	let accepted = timeout(Duration::from_millis(700), async {
		loop {
			if let Some(PeerOutput::Core(PeerEvent::Accepted { .. })) = a.output.recv().await {
				return;
			}
		}
	})
	.await
	.is_ok();
	assert!(!accepted);
	assert!(a.handle.trusted().is_empty(), "nothing was pinned");
}

#[tokio::test]
async fn an_answer_after_the_carrying_link_is_gone_pins_nothing() {
	let (mut a, _sa) = visible_peer("alpha", false).await;
	let mut raw = Raw::connect(a.handle.local_addr(), "caller").await;
	let invite = relay_net::control::new_envelope(relay_net::proto::envelope::Body::Invite(
		relay_net::proto::Invite {
			call_id: CALL.to_be_bytes().to_vec(),
			offered_profiles: vec![relay_engine::peers::PROFILE.to_owned()],
			ring_timeout_ms: 30_000,
			allow_parallel_same_peer: false,
		},
	));
	raw.sender.send(&invite).await.unwrap();
	let caller = expect(&mut a.output, |o| match o {
		PeerOutput::Core(PeerEvent::Invite { peer, .. }) => Some(*peer),
		_ => None,
	})
	.await;
	// The link that carried the invitation goes away before the user answers.
	let key = raw.identity.fingerprint();
	drop(raw);
	tokio::time::sleep(Duration::from_millis(300)).await;
	let accept = PeerMessage::Accept {
		call: CallId::from_u128(CALL),
		session: SessionId::from_u128(SESSION),
	};
	a.handle.send(caller, accept).unwrap();
	tokio::time::sleep(Duration::from_millis(300)).await;
	assert!(
		state_of(&a, &key).is_none(),
		"no pin without the carrying link"
	);
}
