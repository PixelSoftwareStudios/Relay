//! `relay favorites [list|add|remove|move]` against the scripted control and the fake engine.

mod common;

use common::fixtures::{ALICE, BOB};
use common::{FakeControl, run_with};
use relay_cli::cli::exit;

/// Alice is the second favorite, Bob the first.
fn control() -> FakeControl {
	let mut c = FakeControl::new();
	c.snapshot.peers[0].favorite = true;
	c.snapshot.peers[0].favorite_position = 1;
	c.snapshot.peers[1].favorite = true;
	c.snapshot.peers[1].favorite_position = 0;
	c
}

#[tokio::test]
async fn list_is_the_default_and_follows_the_order() {
	let mut c = control();
	let text = run_with(&mut c, &["favorites"], false).await.unwrap();
	let (bob, alice) = (text.find("Bob").unwrap(), text.find("Alice").unwrap());
	assert!(bob < alice, "{text}");
	assert_eq!(
		text,
		run_with(&mut c, &["favorites", "list"], false)
			.await
			.unwrap()
	);
	let json: serde_json::Value = serde_json::from_str(
		&run_with(&mut c, &["favorites", "list"], true)
			.await
			.unwrap(),
	)
	.unwrap();
	assert_eq!(json[0]["name"], "Bob");
	assert_eq!(json[0]["position"], 1);
	assert_eq!(json[1]["position"], 2);
	assert!(c.log.is_empty(), "listing changes nothing");
}

#[tokio::test]
async fn an_empty_list_says_how_to_add() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["favorites", "list"], false)
		.await
		.unwrap();
	assert!(text.contains("relay favorites add"), "{text}");
	assert_eq!(
		run_with(&mut c, &["favorites", "list"], true)
			.await
			.unwrap()
			.trim(),
		"[]"
	);
}

#[tokio::test]
async fn add_appends_and_asks_for_the_whole_list() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["favorites", "add", "alice"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["favorites aa"]);
	// A second add by id prefix: the list is the old one plus the new peer, in that order.
	c.snapshot.peers[0].favorite = true;
	run_with(
		&mut c,
		&["favorites", "add", &hex::encode(&BOB[..4])],
		false,
	)
	.await
	.unwrap();
	assert_eq!(c.log[1], "favorites aa,bb");
	let unknown = run_with(&mut c, &["favorites", "add", "nobody"], false).await;
	assert_eq!(unknown.unwrap_err().exit_code(), exit::TARGET);
}

#[tokio::test]
async fn a_seventh_favorite_is_refused_with_the_reason() {
	let mut c = FakeControl::new();
	for n in 0..6u8 {
		let mut peer = common::fixtures::peer(
			[n + 1; 32],
			&format!("Friend {n}"),
			true,
			relay_api::pb::TrustState::Verified,
		);
		peer.favorite = true;
		peer.favorite_position = u32::from(n);
		c.snapshot.peers.push(peer);
	}
	let err = run_with(&mut c, &["favorites", "add", "alice"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	assert!(err.to_string().contains("up to 6 favorites"), "{err}");
	assert!(c.log.is_empty());
}

#[tokio::test]
async fn remove_and_move_rewrite_the_order() {
	let mut c = control();
	run_with(&mut c, &["favorites", "remove", "bob"], false)
		.await
		.unwrap();
	assert_eq!(c.log[0], "favorites aa");
	// Alice goes to the front: Bob is first now, so moving Alice to 1 swaps them.
	run_with(&mut c, &["favorites", "move", "alice", "1"], false)
		.await
		.unwrap();
	assert_eq!(c.log[1], "favorites aa,bb");
	run_with(&mut c, &["favorites", "move", "alice", "2"], false)
		.await
		.unwrap();
	assert_eq!(c.log[2], "favorites bb,aa");
	for bad in ["0", "3"] {
		let err = run_with(&mut c, &["favorites", "move", "alice", bad], false).await;
		assert_eq!(err.unwrap_err().exit_code(), exit::USAGE, "{bad}");
	}
	assert_eq!(c.log.len(), 3);
	let mut none = FakeControl::new();
	let err = run_with(&mut none, &["favorites", "remove", "alice"], false).await;
	assert_eq!(err.unwrap_err().exit_code(), exit::USAGE);
	let _ = ALICE;
}

#[cfg(unix)]
#[tokio::test]
async fn add_and_move_round_trip_through_the_engine() {
	let h = common::engine::EngineHarness::start().await;
	h.run(&["favorites", "add", "alice"]).await.unwrap();
	h.run(&["favorites", "add", "bob"]).await.unwrap();
	h.run(&["favorites", "move", "bob", "1"]).await.unwrap();
	let json = h.run(&["--json", "favorites", "list"]).await.unwrap();
	let rows: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(rows[0]["name"], "Bob");
	assert_eq!(rows[1]["name"], "Alice");
	h.run(&["favorites", "remove", "alice"]).await.unwrap();
	let json = h.run(&["--json", "favorites"]).await.unwrap();
	let rows: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(rows.as_array().unwrap().len(), 1);
	assert!(h.log().iter().any(|l| l == "favorites bb,aa"));
}
