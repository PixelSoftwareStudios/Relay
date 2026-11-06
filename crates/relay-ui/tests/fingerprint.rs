//! The typed fingerprint of a key-changed device: what matches, and what the controller sends.

mod common;

use std::sync::Arc;

use common::FakeOps;
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::engine::Command;
use relay_ui::fingerprint;
use relay_ui::hub::Hub;
use relay_ui::model::{Peer, Trust};

const FULL: &str = "59f159f159f159f159f159f159f159f159f159f159f159f159f159f159f159f1";

#[test]
fn only_the_whole_fingerprint_matches_in_any_spelling() {
	assert!(fingerprint::matches(FULL, FULL));
	assert!(fingerprint::matches(FULL, &FULL.to_uppercase()));
	assert!(fingerprint::matches(
		FULL,
		&format!("  {} ", fingerprint::lines(FULL).join(" "))
	));
	assert!(fingerprint::matches(
		FULL,
		&fingerprint::lines(FULL).join("-")
	));
	assert!(!fingerprint::matches(FULL, &FULL[..60]));
	assert!(!fingerprint::matches(FULL, &format!("{FULL}0")));
	assert!(!fingerprint::matches(FULL, &format!("{}0", &FULL[..63])));
	assert!(!fingerprint::matches(FULL, ""));
	assert!(!fingerprint::matches(FULL, "59F1-59F1-59F1-59F1-59F1"));
	assert_eq!(fingerprint::lines(FULL).len(), 4);
	assert_eq!(fingerprint::lines(FULL)[0], "59F1 59F1 59F1 59F1");
}

fn changed(key_changed: bool) -> Peer {
	Peer {
		peer_id: FULL.into(),
		display_name: "Dantes-Laptop.local".into(),
		addresses: Vec::new(),
		trust: if key_changed {
			Trust::KeyChanged
		} else {
			Trust::Verified
		},
		reachable: false,
		avatar_hash: String::new(),
		fingerprint_short: String::new(),
		port: 0,
		last_seen_unix_ms: 0,
		shared_channels: Vec::new(),
		favorite: false,
		favorite_position: 0,
		key_changed,
		previous_peer_id: if key_changed {
			"1a10".repeat(16)
		} else {
			String::new()
		},
		previous_fingerprint_short: String::new(),
	}
}

fn controller(peer: Peer) -> (Controller, Arc<FakeOps>) {
	let hub = Arc::new(Hub::default());
	hub.update(|s| s.peers.push(peer));
	let slot = Arc::new(SessionSlot::default());
	let ops = Arc::new(FakeOps::default());
	slot.set(ops.clone());
	(Controller::new(hub, slot), ops)
}

#[tokio::test]
async fn the_controller_sends_nothing_unless_the_full_fingerprint_is_given() {
	let (ctl, ops) = controller(changed(true));
	assert!(ctl.replace_pinned_key(FULL, &FULL[..32]).await.is_err());
	assert!(
		ctl.replace_pinned_key(FULL, &"ab".repeat(32))
			.await
			.is_err()
	);
	assert!(
		ctl.replace_pinned_key(&"ab".repeat(32), FULL)
			.await
			.is_err()
	);
	assert!(ops.recorded().is_empty());
	ctl.replace_pinned_key(FULL, &FULL.to_uppercase())
		.await
		.unwrap();
	assert_eq!(
		ops.recorded(),
		[Command::ReplacePinnedKey {
			peer_id: [0x59, 0xf1].repeat(16),
			fingerprint: FULL.into(),
		}]
	);

	// A contact without a key change has nothing to replace.
	let (ctl, ops) = controller(changed(false));
	assert!(ctl.replace_pinned_key(FULL, FULL).await.is_err());
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn forgetting_sends_the_stored_key_after_a_change_and_the_plain_one_otherwise() {
	let (ctl, ops) = controller(changed(true));
	ctl.forget_peer(FULL).await.unwrap();
	assert_eq!(
		ops.recorded(),
		[Command::ForgetPeer {
			peer_id: [0x1a, 0x10].repeat(16)
		}]
	);
	let (ctl, ops) = controller(changed(false));
	ctl.forget_peer(FULL).await.unwrap();
	assert_eq!(
		ops.recorded(),
		[Command::ForgetPeer {
			peer_id: [0x59, 0xf1].repeat(16)
		}]
	);
}
