#![cfg(unix)]
//! A key-changed contact reaches clients with the pinned key's identity beside the new one.

#[macro_use]
mod service_support;

use relay_api::WatchItem;
use relay_api::pb::{Problem, TrustState, state_event::Event};
use relay_core::PeerId;
use relay_engine::service::{PeerInfo, PeerTrust};
use service_support::*;

#[tokio::test]
async fn the_peer_event_carries_the_key_change() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let (_, mut watch) = cli.snapshot_and_watch().await.expect("watch");
	let previous = PeerId::from_bytes([0x1a; 32]);
	let info = PeerInfo {
		display_name: "Dantes-Laptop.local".to_owned(),
		trust: PeerTrust::KeyChanged,
		key_change: Some(previous),
		..PeerInfo::bare(peer_id())
	};
	let warning = Problem {
		code: "PEER_KEY_CHANGED".to_owned(),
		message: "refused".to_owned(),
		retryable: false,
		recovery_action: String::new(),
	};
	rig.daemon
		.control()
		.attention(Some(info), warning)
		.await
		.expect("flagged");
	let peer = loop {
		let WatchItem::Event { event, .. } = watch.next().await.expect("item").expect("open")
		else {
			panic!("event expected");
		};
		if let Some(Event::Peer(peer)) = event.event {
			break peer;
		}
	};
	assert_eq!(peer.trust, TrustState::KeyChanged as i32);
	assert!(peer.key_changed);
	assert_eq!(peer.previous_peer_id, previous.as_bytes());
	assert_eq!(peer.previous_fingerprint_short.len(), 24);
	assert_eq!(peer.fingerprint_short.len(), 24);
	assert_ne!(peer.previous_fingerprint_short, peer.fingerprint_short);
	let snapshot = cli.snapshot().await.expect("snapshot");
	assert!(snapshot.peers.iter().all(|p| p.key_changed));
}
