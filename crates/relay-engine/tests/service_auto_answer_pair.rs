#![cfg(unix)]
//! Auto-answer on two real engines: only a verified contact can be switched on, a call from
//! it is answered by the called engine in the default (push-to-talk) mode, and blocking the
//! contact clears the flag.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_pair;

use relay_api::RelayClient;
use relay_api::pb::{
	CallState, DecidePeerTrustRequest, Peer, StartCallRequest, TransmitMode, TrustState,
	start_call_request::Target,
};
use service_support::*;
use support_pair::{Side, paired, peer_where};

async fn decide(cli: &mut RelayClient, op: u8, peer: &Peer, to: TrustState, verified: bool) {
	let request = DecidePeerTrustRequest {
		context: Some(ctx(cli, op)),
		peer_id: peer.peer_id.clone(),
		expected_public_key_sha256: peer.peer_id.clone(),
		requested_trust: to as i32,
		out_of_band_verified: verified,
	};
	let done = cli
		.raw()
		.decide_peer_trust(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_applied(&done);
}

async fn verify(a: &mut Side, b_on_a: &Peer) -> Peer {
	decide(&mut a.cli, 10, b_on_a, TrustState::Verified, true).await;
	eventually!(
		peer_where(&mut a.cli, |p| {
			p.peer_id == b_on_a.peer_id && p.trust == TrustState::Verified as i32
		})
		.await
	)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_verified_contact_can_be_switched_on() {
	let (mut a, b, b_on_a, _) = paired().await;
	let refused = a
		.cli
		.set_peer_auto_answer(&b_on_a.peer_id, true)
		.await
		.unwrap();
	assert_failed(&refused, "INVALID_REQUEST");
	assert!(
		refused
			.problem
			.as_ref()
			.unwrap()
			.message
			.contains("verified"),
		"{refused:?}"
	);
	let unknown = a.cli.set_peer_auto_answer(&[3; 32], true).await.unwrap();
	assert_failed(&unknown, "INVALID_REQUEST");
	let verified = verify(&mut a, &b_on_a).await;
	assert!(!verified.auto_answer, "off until the user turns it on");
	assert_applied(
		&a.cli
			.set_peer_auto_answer(&b_on_a.peer_id, true)
			.await
			.unwrap(),
	);
	let on = peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id)
		.await
		.unwrap();
	assert!(on.auto_answer);
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_called_engine_answers_in_the_default_push_to_talk_mode() {
	let (mut a, mut b, b_on_a, a_on_b) = paired().await;
	verify(&mut a, &b_on_a).await;
	assert_applied(
		&a.cli
			.set_peer_auto_answer(&b_on_a.peer_id, true)
			.await
			.unwrap(),
	);
	let request = StartCallRequest {
		context: Some(ctx(&b.cli, 20)),
		target: Some(Target::PeerId(a_on_b.peer_id.clone())),
		allow_parallel_same_peer: false,
	};
	let call = b
		.cli
		.raw()
		.start_call(request)
		.await
		.expect("call")
		.into_inner()
		.call_id;
	let on_a = eventually!(
		call_in(&mut a.cli, &call)
			.await
			.filter(|c| c.auto_answered && c.state != CallState::IncomingRinging as i32)
	);
	assert_eq!(
		on_a.transmit.expect("transmit").mode,
		TransmitMode::PushToTalk as i32,
		"the default call mic state, not an open mic"
	);
	let live = eventually!(
		call_in(&mut a.cli, &call)
			.await
			.filter(|c| c.state == CallState::Connected as i32)
	);
	assert!(!live.transmit.unwrap().effective_transmitting);
	// The caller sees an ordinary call that was picked up.
	let on_b = call_in(&mut b.cli, &call).await.expect("call on b");
	assert!(!on_b.auto_answered);
	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocking_the_contact_clears_the_flag() {
	let (mut a, b, b_on_a, _) = paired().await;
	verify(&mut a, &b_on_a).await;
	assert_applied(
		&a.cli
			.set_peer_auto_answer(&b_on_a.peer_id, true)
			.await
			.unwrap(),
	);
	decide(&mut a.cli, 30, &b_on_a, TrustState::Blocked, false).await;
	let blocked = eventually!(
		peer_where(&mut a.cli, |p| {
			p.peer_id == b_on_a.peer_id && p.trust == TrustState::Blocked as i32
		})
		.await
	);
	assert!(!blocked.auto_answer);
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}
