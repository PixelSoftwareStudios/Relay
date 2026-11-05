//! The state feed reopens the watch on gaps, resyncs and lost streams.

mod common;

use std::time::Duration;

use common::FakeControl;
use common::fixtures;
use relay_api::pb::{Peer, ResyncRequired, StateEvent, state_event};
use relay_api::{ApiError, WatchItem};
use relay_cli::client::{ConnectionState, Update, run_state_feed};
use tokio::sync::mpsc;

fn event(revision: u64) -> WatchItem {
	WatchItem::Event {
		revision,
		event: StateEvent {
			event: Some(state_event::Event::Peer(Peer::default())),
		},
	}
}

async fn collect(control: FakeControl, count: usize) -> Vec<Update> {
	let (tx, mut rx) = mpsc::channel(64);
	let task = tokio::spawn(run_state_feed(control, tx, Duration::from_millis(5)));
	let mut seen = Vec::new();
	while seen.len() < count {
		match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
			Ok(Some(u)) => seen.push(u),
			_ => break,
		}
	}
	task.abort();
	seen
}

fn shape(updates: &[Update]) -> Vec<String> {
	updates
		.iter()
		.map(|u| match u {
			Update::Connection(ConnectionState::Live) => "live".into(),
			Update::Connection(ConnectionState::Resyncing(_)) => "resyncing".into(),
			Update::Connection(ConnectionState::Lost(_)) => "lost".into(),
			Update::State(item) => match **item {
				WatchItem::Snapshot(_) => "snapshot".into(),
				WatchItem::Event { revision, .. } => format!("event{revision}"),
				WatchItem::Resync(_) => "resync".into(),
			},
			Update::Telemetry(_) => "telemetry".into(),
			Update::Hotkey(_) => "hotkey".into(),
		})
		.collect()
}

#[tokio::test]
async fn revision_gap_reopens_the_watch_with_a_fresh_snapshot() {
	let mut c = FakeControl::new();
	c.watches.push_back(vec![
		Ok(WatchItem::Snapshot(fixtures::snapshot())),
		Ok(event(11)),
		Err(ApiError::RevisionGap {
			expected: 12,
			received: 14,
		}),
	]);
	c.watches
		.push_back(vec![Ok(WatchItem::Snapshot(fixtures::snapshot()))]);
	let seen = collect(c, 7).await;
	assert_eq!(
		shape(&seen),
		[
			"live",
			"snapshot",
			"event11",
			"resyncing",
			"live",
			"snapshot",
			"lost"
		],
	);
}

#[tokio::test]
async fn engine_resync_request_and_overflow_both_resync() {
	let mut c = FakeControl::new();
	c.watches.push_back(vec![
		Ok(WatchItem::Snapshot(fixtures::snapshot())),
		Ok(WatchItem::Resync(ResyncRequired {
			current_revision: 99,
			reason: "replay window exceeded".into(),
		})),
	]);
	c.watches
		.push_back(vec![Err(ApiError::SubscriptionOverflow)]);
	c.watches
		.push_back(vec![Ok(WatchItem::Snapshot(fixtures::snapshot()))]);
	let seen = collect(c, 9).await;
	let kinds = shape(&seen);
	assert_eq!(
		kinds.iter().filter(|k| *k == "resyncing").count(),
		2,
		"{kinds:?}"
	);
	assert_eq!(
		kinds.iter().filter(|k| *k == "snapshot").count(),
		2,
		"{kinds:?}"
	);
	assert!(
		!kinds.contains(&"resync".to_owned()),
		"resync items are consumed, not forwarded"
	);
}

#[tokio::test]
async fn open_failure_is_reported_as_lost_and_retried() {
	let mut c = FakeControl::new();
	c.watches
		.push_back(vec![Ok(WatchItem::Snapshot(fixtures::snapshot()))]);
	// The second open fails (no script left): "lost", then retry keeps failing.
	let seen = collect(c, 5).await;
	let kinds = shape(&seen);
	assert_eq!(&kinds[..3], ["live", "snapshot", "lost"]);
	assert!(kinds[3..].iter().all(|k| k == "lost"), "{kinds:?}");
}

#[tokio::test]
async fn dropping_the_receiver_stops_the_feed() {
	let mut c = FakeControl::new();
	c.watches
		.push_back(vec![Ok(WatchItem::Snapshot(fixtures::snapshot()))]);
	let (tx, rx) = mpsc::channel(1);
	drop(rx);
	tokio::time::timeout(
		Duration::from_secs(2),
		run_state_feed(c, tx, Duration::from_millis(5)),
	)
	.await
	.expect("feed ends once nobody listens");
}
