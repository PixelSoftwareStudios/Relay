#![cfg(unix)]
//! Forgetting trust over the local API, on two real engines.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_pair;

use relay_api::pb::{DecidePeerTrustRequest, TrustState, state_event::Event};
use relay_api::{RelayClient, WatchItem};
use service_support::*;
use support_pair::paired;
use tonic::Code;

fn first_group(short: &str) -> String {
	short
		.rsplit_once('-')
		.map_or(short, |(head, _)| head)
		.to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_is_forgotten_by_its_short_fingerprint_and_leaves_the_feed() {
	let (mut a, b, b_on_a, _) = paired().await;
	let short = b_on_a.fingerprint_short.clone();
	assert_eq!(short.len(), 24, "{short}");
	assert!(short.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));

	let (_, mut watch) = a.cli.snapshot_and_watch().await.unwrap();
	let nothing = a.cli.forget_peer(None, "no such device").await.unwrap();
	assert_failed(&nothing, "NOT_FOUND");
	let too_short = a.cli.forget_peer(None, "ab12").await.unwrap();
	assert_failed(&too_short, "NOT_FOUND");
	let blank = a.cli.forget_peer(None, "  ").await.expect_err("blank");
	assert!(
		format!("{blank:?}").contains("InvalidArgument"),
		"{blank:?}"
	);

	let done = a.cli.forget_peer(None, &first_group(&short)).await.unwrap();
	assert_applied(&done);
	let removed =
		eventually!(
			match tokio::time::timeout(std::time::Duration::from_millis(50), watch.next()).await {
				Ok(Ok(Some(WatchItem::Event { event, .. }))) => match event.event {
					Some(Event::Removed(r)) if r.kind == "peer" => Some(r.id),
					_ => None,
				},
				_ => None,
			}
		);
	assert_eq!(removed, b_on_a.peer_id);
	// It cannot be forgotten twice.
	let again = a.cli.forget_peer(Some(&b_on_a.peer_id), "").await.unwrap();
	assert_failed(&again, "NOT_FOUND");
	// The link is cut: B is no longer a trusted, reachable peer of A.
	eventually!(
		support_pair::peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id
			&& p.trust == TrustState::TofuUnverified as i32)
		.await
		.is_none()
		.then_some(())
	);
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_trust_decision_of_unknown_forgets_without_a_key_hash() {
	let (mut a, b, b_on_a, _) = paired().await;
	let request = |cli: &RelayClient, op: u8, expected: Vec<u8>| DecidePeerTrustRequest {
		context: Some(ctx(cli, op)),
		peer_id: b_on_a.peer_id.clone(),
		expected_public_key_sha256: expected,
		requested_trust: TrustState::Unknown as i32,
		out_of_band_verified: false,
	};
	let wrong = request(&a.cli, 40, vec![9; 32]);
	let refused = a
		.cli
		.raw()
		.decide_peer_trust(wrong)
		.await
		.expect_err("mismatch");
	assert_eq!(refused.code(), Code::InvalidArgument);
	let right = request(&a.cli, 41, Vec::new());
	let done = a
		.cli
		.raw()
		.decide_peer_trust(right)
		.await
		.unwrap()
		.into_inner();
	assert_applied(&done);
	assert!(
		support_pair::peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id
			&& p.trust == TrustState::TofuUnverified as i32)
		.await
		.is_none()
	);

	// Forgetting is a mutation: it needs call-control.
	let mut observer = RelayClient::connect(a.daemon.endpoint()).await.unwrap();
	observer
		.register("watcher", "test", &["observe"])
		.await
		.unwrap();
	let denied = observer
		.forget_peer(None, "anyone")
		.await
		.expect_err("denied");
	assert!(format!("{denied:?}").contains("PermissionDenied"));
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}
