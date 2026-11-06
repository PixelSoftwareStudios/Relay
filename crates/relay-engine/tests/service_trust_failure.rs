#![cfg(unix)]
//! `DecidePeerTrust` on a whole engine reports a trust it could not store as a FAILED
//! operation with a stable code, instead of an applied one that did nothing.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::{DecidePeerTrustRequest, TrustState};
use service_support::*;
use support_channels::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trusting_a_device_that_cannot_be_reached_fails_visibly() {
	let mut a = side(440.0).await;
	let ghost = vec![7u8; 32];
	let request = DecidePeerTrustRequest {
		context: Some(ctx(&a.cli, 1)),
		peer_id: ghost.clone(),
		expected_public_key_sha256: ghost.clone(),
		requested_trust: TrustState::TofuUnverified as i32,
		out_of_band_verified: false,
	};
	let op = a
		.cli
		.raw()
		.decide_peer_trust(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_failed(&op, "UNAVAILABLE");
	let peers = a.cli.snapshot().await.expect("snapshot").peers;
	assert!(peers.iter().all(|p| p.peer_id != ghost));
	a.daemon.shutdown().await.expect("a stops");
}
