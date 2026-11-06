#![cfg(unix)]
//! Calling a device that is not a contact yet, on two real engines: the call rings, the
//! keys are pinned (as first use) only when it is answered, a decline pins nothing, and a
//! blocked device cannot ring at all until it is unblocked.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::{
	AcquireControllerRequest, CallState, DecidePeerTrustRequest, StartCallRequest, TrustState,
	start_call_request::Target,
};
use service_support::*;
use support_channels::*;

async fn trust_of(side: &mut Side, peer: &[u8]) -> Option<i32> {
	side.cli
		.snapshot()
		.await
		.expect("snapshot")
		.peers
		.iter()
		.find(|p| p.peer_id == peer)
		.map(|p| p.trust)
}

/// Both engines have met (untrusted links) without any trust decision.
async fn met() -> (Side, Side) {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	let addr = b.daemon.listen_addr().expect("listens");
	let request = StartCallRequest {
		context: Some(ctx(&a.cli, 1)),
		target: Some(Target::ManualLocalAddress(addr.to_string())),
		allow_parallel_same_peer: false,
	};
	let _ = a.cli.raw().start_call(request).await;
	let (a_id, b_id) = (id_of(&a), id_of(&b));
	eventually!(
		trust_of(&mut a, &b_id)
			.await
			.filter(|t| *t == TrustState::Unknown as i32)
	);
	eventually!(
		trust_of(&mut b, &a_id)
			.await
			.filter(|t| *t == TrustState::Unknown as i32)
	);
	(a, b)
}

async fn decide(side: &mut Side, op: u8, peer: &[u8], to: TrustState) -> relay_api::pb::Operation {
	let request = DecidePeerTrustRequest {
		context: Some(ctx(&side.cli, op)),
		peer_id: peer.to_vec(),
		expected_public_key_sha256: peer.to_vec(),
		requested_trust: to as i32,
		out_of_band_verified: false,
	};
	side.cli
		.raw()
		.decide_peer_trust(request)
		.await
		.expect("rpc")
		.into_inner()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn answering_a_call_from_an_unknown_device_trusts_it_as_first_use() {
	let (mut a, mut b) = met().await;
	let (a_id, b_id) = (id_of(&a), id_of(&b));
	let call = ring(&mut a, &b, 10).await;
	// Ringing alone trusts nobody.
	eventually!(
		call_in(&mut b.cli, &call)
			.await
			.filter(|c| c.state == CallState::IncomingRinging as i32)
	);
	assert_eq!(
		trust_of(&mut b, &a_id).await,
		Some(TrustState::Unknown as i32)
	);
	answer(&mut b.cli, &call, 20).await;
	let first_use = TrustState::TofuUnverified as i32;
	eventually!(trust_of(&mut a, &b_id).await.filter(|t| *t == first_use));
	eventually!(trust_of(&mut b, &a_id).await.filter(|t| *t == first_use));
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declining_a_call_from_an_unknown_device_trusts_nobody() {
	let (mut a, mut b) = met().await;
	let (a_id, b_id) = (id_of(&a), id_of(&b));
	let call = ring(&mut a, &b, 10).await;
	eventually!(
		call_in(&mut b.cli, &call)
			.await
			.filter(|c| c.state == CallState::IncomingRinging as i32)
	);
	let context = ctx(&b.cli, 20);
	let lease = b
		.cli
		.raw()
		.acquire_controller(AcquireControllerRequest {
			context: Some(context),
			call_id: call.clone(),
			request_explicit_takeover: false,
		})
		.await
		.expect("lease")
		.into_inner();
	let decline = mutation(&b.cli, 21, &call, lease.generation);
	assert_applied(
		&b.cli
			.raw()
			.decline_call(decline)
			.await
			.expect("decline")
			.into_inner(),
	);
	eventually!(
		call_in(&mut a.cli, &call)
			.await
			.filter(|c| c.state != CallState::OutgoingRinging as i32)
	);
	settle(300).await;
	assert_eq!(
		trust_of(&mut a, &b_id).await,
		Some(TrustState::Unknown as i32)
	);
	assert_eq!(
		trust_of(&mut b, &a_id).await,
		Some(TrustState::Unknown as i32)
	);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_blocked_device_cannot_ring_until_it_is_unblocked() {
	let (mut a, mut b) = met().await;
	let (a_id, _b_id) = (id_of(&a), id_of(&b));
	assert_applied(&decide(&mut b, 30, &a_id, TrustState::Blocked).await);
	eventually!(
		trust_of(&mut b, &a_id)
			.await
			.filter(|t| *t == TrustState::Blocked as i32)
	);
	let call = ring(&mut a, &b, 10).await;
	settle(800).await;
	assert!(
		call_in(&mut b.cli, &call).await.is_none(),
		"no ring on the blocked side"
	);
	// Unblocking forgets the entry; the device is simply unknown again.
	assert_applied(&decide(&mut b, 31, &a_id, TrustState::Unknown).await);
	eventually!(match trust_of(&mut b, &a_id).await {
		None => Some(()),
		Some(t) if t != TrustState::Blocked as i32 => Some(()),
		_ => None,
	});
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
