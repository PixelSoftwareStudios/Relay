#![cfg(unix)]
//! `ConnectAndTrust` on whole engines: a scanned fingerprint trusts the device, a wrong one
//! never does, and a failure leaves no trust behind.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::{ConnectAndTrustRequest, Operation, TrustState};
use service_support::*;
use support_channels::*;

fn string_of(side: &Side, fingerprint: &[u8]) -> String {
	let addr = side.daemon.listen_addr().expect("listens");
	format!("relay://{addr}#{}", hex::encode(fingerprint))
}

async fn connect(side: &mut Side, op: u8, text: &str) -> Operation {
	let request = ConnectAndTrustRequest {
		context: Some(ctx(&side.cli, op)),
		connection_string: text.to_owned(),
	};
	side.cli
		.raw()
		.connect_and_trust(request)
		.await
		.expect("rpc")
		.into_inner()
}

async fn trusted(side: &mut Side, peer: &[u8]) -> Vec<i32> {
	side.cli
		.snapshot()
		.await
		.expect("snapshot")
		.peers
		.iter()
		.filter(|p| p.peer_id == peer)
		.map(|p| p.trust)
		.collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_matching_fingerprint_trusts_the_device_as_verified() {
	let mut a = side(440.0).await;
	let b = side(1_700.0).await;
	let text = string_of(&b, &id_of(&b));
	let op = connect(&mut a, 1, &text).await;
	assert_applied(&op);
	let b_id = id_of(&b);
	eventually!(Some(trusted(&mut a, &b_id).await).filter(|t| t == &[TrustState::Verified as i32]));
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_fingerprint_is_refused_and_nothing_is_trusted() {
	let mut a = side(440.0).await;
	let b = side(1_700.0).await;
	let c = side(3_100.0).await;
	// B's address with C's fingerprint.
	let text = string_of(&b, &id_of(&c));
	let op = connect(&mut a, 1, &text).await;
	assert_failed(&op, "CONNECT_FINGERPRINT_MISMATCH");
	for peer in [id_of(&b), id_of(&c)] {
		assert!(
			trusted(&mut a, &peer)
				.await
				.iter()
				.all(|t| *t != TrustState::Verified as i32
					&& *t != TrustState::TofuUnverified as i32)
		);
	}
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreachable_address_fails_cleanly() {
	let mut a = side(440.0).await;
	let key = [7u8; 32];
	let op = connect(
		&mut a,
		1,
		&format!("relay://127.0.0.1:1#{}", hex::encode(key)),
	)
	.await;
	assert_failed(&op, "CONNECT_UNREACHABLE");
	assert!(trusted(&mut a, &key).await.is_empty());
	a.daemon.shutdown().await.expect("a stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn short_hints_and_malformed_strings_are_rejected() {
	let mut a = side(440.0).await;
	let b = side(1_700.0).await;
	let addr = b.daemon.listen_addr().expect("listens");
	let full = hex::encode(id_of(&b));
	for (op, text) in [
		(1, format!("relay://{addr}#{}", &full[..16])),
		(2, format!("relay://{addr}")),
		(3, "not a string".to_owned()),
		(4, format!("relay://8.8.8.8:47800#{full}")),
	] {
		let result = connect(&mut a, op, &text).await;
		assert_failed(&result, "CONNECT_STRING_INVALID");
	}
	assert!(trusted(&mut a, &id_of(&b)).await.is_empty());
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
