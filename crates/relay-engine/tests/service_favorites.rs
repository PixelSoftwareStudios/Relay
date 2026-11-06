#![cfg(unix)]
//! Favorites over the local API, on two real engines: the list, its limits and the
//! cascade when a peer is forgotten.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_pair;

use relay_api::RelayClient;
use relay_api::pb::{Operation, state_event::Event};
use relay_api::{WatchItem, pb};
use service_support::*;
use support_pair::{paired, peer_where};

fn reason(op: &Operation) -> String {
	op.problem.as_ref().expect("problem").message.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_trusted_peer_becomes_a_favorite_and_clients_hear_about_it() {
	let (mut a, b, b_on_a, _) = paired().await;
	assert!(!b_on_a.favorite, "nobody is a favorite at first");
	let (_, mut watch) = a.cli.snapshot_and_watch().await.unwrap();

	let done = a
		.cli
		.set_favorites(std::slice::from_ref(&b_on_a.peer_id))
		.await
		.unwrap();
	assert_applied(&done);
	let heard =
		eventually!(
			match tokio::time::timeout(std::time::Duration::from_millis(50), watch.next()).await {
				Ok(Ok(Some(WatchItem::Event { event, .. }))) => match event.event {
					Some(Event::Peer(p)) if p.peer_id == b_on_a.peer_id && p.favorite => Some(p),
					_ => None,
				},
				_ => None,
			}
		);
	assert_eq!(heard.favorite_position, 0);
	let listed = peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id)
		.await
		.unwrap();
	assert!(listed.favorite);

	// Setting the same list again is harmless; an empty list clears it.
	assert_applied(
		&a.cli
			.set_favorites(std::slice::from_ref(&b_on_a.peer_id))
			.await
			.unwrap(),
	);
	assert_applied(&a.cli.set_favorites(&[]).await.unwrap());
	let cleared = peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id)
		.await
		.unwrap();
	assert!(!cleared.favorite);
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_list_is_limited_trusted_only_and_without_duplicates() {
	let (mut a, b, b_on_a, _) = paired().await;
	let stranger = vec![7u8; 32];

	let untrusted = a
		.cli
		.set_favorites(std::slice::from_ref(&stranger))
		.await
		.unwrap();
	assert_failed(&untrusted, "INVALID_REQUEST");
	assert!(reason(&untrusted).contains("trusted"), "{untrusted:?}");

	let seven: Vec<Vec<u8>> = (1..=7u8).map(|n| vec![n; 32]).collect();
	let full = a.cli.set_favorites(&seven).await.unwrap();
	assert_failed(&full, "INVALID_REQUEST");
	assert!(
		reason(&full).contains("up to 6 favorites"),
		"{}",
		reason(&full)
	);

	let twice = a
		.cli
		.set_favorites(&[b_on_a.peer_id.clone(), b_on_a.peer_id.clone()])
		.await
		.unwrap();
	assert_failed(&twice, "INVALID_REQUEST");

	let short = a
		.cli
		.set_favorites(&[vec![1, 2, 3]])
		.await
		.expect_err("malformed id");
	assert!(
		format!("{short:?}").contains("InvalidArgument"),
		"{short:?}"
	);

	// A refusal changed nothing.
	let listed = peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id)
		.await
		.unwrap();
	assert!(!listed.favorite);

	let mut observer = RelayClient::connect(a.daemon.endpoint()).await.unwrap();
	observer
		.register("watcher", "test", &["observe"])
		.await
		.unwrap();
	let denied = observer.set_favorites(&[]).await.expect_err("denied");
	assert!(format!("{denied:?}").contains("PermissionDenied"));
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forgetting_a_peer_removes_its_favorite() {
	let (mut a, b, b_on_a, _) = paired().await;
	assert_applied(
		&a.cli
			.set_favorites(std::slice::from_ref(&b_on_a.peer_id))
			.await
			.unwrap(),
	);
	assert_applied(&a.cli.forget_peer(Some(&b_on_a.peer_id), "").await.unwrap());
	// It is no longer trusted, so it cannot be put back.
	let again: pb::Operation = a
		.cli
		.set_favorites(std::slice::from_ref(&b_on_a.peer_id))
		.await
		.unwrap();
	assert_failed(&again, "INVALID_REQUEST");
	assert!(reason(&again).contains("trusted"));
	b.daemon.shutdown().await.unwrap();
	a.daemon.shutdown().await.unwrap();
}
