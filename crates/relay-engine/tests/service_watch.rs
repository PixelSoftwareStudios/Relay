#![cfg(unix)]
//! Snapshot and watch: no gaps, replay, resync and bounded streams.

#[macro_use]
mod service_support;

use relay_api::pb::{SetGlobalControlsRequest, state_event::Event};
use relay_api::{RelayClient, WatchItem, WatchOptions};
use service_support::*;
use tonic::Code;

async fn toggle(client: &mut RelayClient, op: u8, muted: bool) {
	let request = SetGlobalControlsRequest {
		context: Some(ctx(client, op)),
		muted: Some(muted),
		deafened: None,
		microphone_gain: None,
		playback_gain: None,
		ptt_release_delay_ms: None,
	};
	client
		.raw()
		.set_global_controls(request)
		.await
		.expect("toggle");
}

#[tokio::test]
async fn a_watcher_sees_every_revision_in_order() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut gui = rig.full_client("desktop").await;
	let (snapshot, mut watch) = gui.snapshot_and_watch().await.expect("watch");

	for op in 1..=20u8 {
		toggle(&mut cli, op, op % 2 == 1).await;
	}
	// Strict tracking inside the client fails on any gap; 20 changes, 20 events.
	let mut last = snapshot.revision;
	for _ in 0..20 {
		match watch.next().await.expect("no gap").expect("open") {
			WatchItem::Event { revision, event } => {
				assert_eq!(revision, last + 1);
				assert!(matches!(event.event, Some(Event::Settings(_))));
				last = revision;
			}
			other => panic!("unexpected {other:?}"),
		}
	}
}

#[tokio::test]
async fn a_fresh_watch_starts_with_the_snapshot_then_continues_without_overlap() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	toggle(&mut cli, 1, true).await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	let first = watch.next().await.expect("item").expect("open");
	let WatchItem::Snapshot(snapshot) = first else {
		panic!("a watch without a cursor begins with a snapshot");
	};
	assert!(snapshot.settings.expect("settings").global_muted);
	toggle(&mut cli, 2, false).await;
	let WatchItem::Event { revision, .. } = watch.next().await.expect("item").expect("open") else {
		panic!("event expected");
	};
	assert_eq!(revision, snapshot.revision + 1);
}

#[tokio::test]
async fn resuming_replays_exactly_what_was_missed() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let base = cli.snapshot().await.expect("snapshot").revision;
	for op in 1..=3u8 {
		toggle(&mut cli, op, op % 2 == 1).await;
	}
	let mut watch = cli
		.watch(WatchOptions {
			after_revision: Some(base),
			..WatchOptions::default()
		})
		.await
		.expect("resume");
	for expected in 1..=3u64 {
		let WatchItem::Event { revision, .. } = watch.next().await.expect("item").expect("open")
		else {
			panic!("replayed event expected");
		};
		assert_eq!(revision, base + expected);
	}
}

#[tokio::test]
async fn a_cursor_outside_the_replay_window_must_resync() {
	let rig = Rig::with(|options| {
		options.limits.lifecycle_replay_events = 4;
		options.limits.client_lifecycle_queue = 4;
		options.api.subscriber_queue = 4;
	})
	.await;
	let mut cli = rig.full_client("relay-cli").await;
	for op in 1..=12u8 {
		toggle(&mut cli, op, op % 2 == 1).await;
	}
	let mut stale = cli
		.watch(WatchOptions {
			after_revision: Some(1),
			..WatchOptions::default()
		})
		.await
		.expect("watch opens");
	let item = stale.next().await.expect("item").expect("open");
	let WatchItem::Resync(resync) = item else {
		panic!("expected resync, got {item:?}");
	};
	assert!(resync.current_revision >= 12);
	// A cursor from the future is not trusted either.
	let mut ahead = cli
		.watch(WatchOptions {
			after_revision: Some(10_000),
			..WatchOptions::default()
		})
		.await
		.expect("watch opens");
	assert!(matches!(ahead.next().await, Ok(Some(WatchItem::Resync(_)))));
	// Recovery is a fresh watch.
	let mut fresh = cli.watch(WatchOptions::default()).await.expect("fresh");
	assert!(matches!(
		fresh.next().await,
		Ok(Some(WatchItem::Snapshot(_)))
	));
}

#[tokio::test]
async fn call_filters_and_peer_opt_out_narrow_the_stream() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut watcher = rig.full_client("desktop").await;
	let mut watch = watcher
		.watch(WatchOptions {
			call_filter: vec![vec![0xAA; 16]],
			include_peers: false,
			..WatchOptions::default()
		})
		.await
		.expect("filtered watch");
	assert!(matches!(
		watch.next().await,
		Ok(Some(WatchItem::Snapshot(_)))
	));
	start_call(&mut cli, 1).await;
	toggle(&mut cli, 2, true).await;
	// The unrelated call is filtered out; the settings change still arrives.
	let WatchItem::Event { event, .. } = watch.next().await.expect("item").expect("open") else {
		panic!("event expected");
	};
	assert!(matches!(event.event, Some(Event::Settings(_))));
}

#[tokio::test]
async fn open_streams_are_bounded() {
	let rig = Rig::with(|options| options.api.max_streams = 2).await;
	let mut cli = rig.full_client("relay-cli").await;
	let _a = cli.watch(WatchOptions::default()).await.expect("first");
	let _b = cli.watch(WatchOptions::default()).await.expect("second");
	let error = cli
		.watch(WatchOptions::default())
		.await
		.map(|_| ())
		.expect_err("full");
	assert_eq!(code_of(&error), Code::ResourceExhausted);
}

#[tokio::test]
async fn discovered_peers_and_devices_arrive_as_events() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let (_, mut watch) = cli.snapshot_and_watch().await.expect("watch");
	let info = relay_engine::service::PeerInfo {
		id: peer_id(),
		display_name: "Kitchen".to_owned(),
		addresses: vec!["192.168.1.20:7000".to_owned()],
		trust: relay_engine::service::PeerTrust::TofuUnverified,
		reachable: true,
		avatar: None,
	};
	rig.daemon
		.control()
		.peers_changed(vec![info.clone()])
		.await
		.expect("peers");
	let WatchItem::Event { event, .. } = watch.next().await.expect("item").expect("open") else {
		panic!("event expected");
	};
	let Some(Event::Peer(peer)) = event.event else {
		panic!("peer event expected");
	};
	assert_eq!(peer.display_name, "Kitchen");
	assert_eq!(peer.trust, relay_api::pb::TrustState::TofuUnverified as i32);

	rig.daemon
		.control()
		.peers_changed(Vec::new())
		.await
		.expect("peers gone");
	let WatchItem::Event { event, .. } = watch.next().await.expect("item").expect("open") else {
		panic!("event expected");
	};
	assert!(matches!(event.event, Some(Event::Removed(r)) if r.kind == "peer"));
}
