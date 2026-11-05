mod common;

use common::Fixture;
use relay_core::testing::{FakeWatcher, Node};
use relay_core::*;

#[test]
fn watcher_following_events_always_equals_a_fresh_snapshot() {
	let mut f = Fixture::new();
	let mut watcher = FakeWatcher::attach(&f.node.state);
	f.consent(TransmitRoute::push_to_talk());
	watcher.sync(&f.node.state);
	assert_eq!(watcher.view, f.node.state.snapshot());
	f.ptt(1, 1, true).unwrap();
	f.node.advance(1_000);
	watcher.sync(&f.node.state);
	assert_eq!(watcher.view, f.node.state.snapshot());
	assert_eq!(watcher.resyncs, 0, "no gap, no resync");
}

#[test]
fn every_revision_is_delivered_exactly_once_in_order() {
	let mut f = Fixture::new();
	let start = f.node.state.snapshot().cursor();
	f.consent(TransmitRoute::open_mic());
	let Watch::Events(events) = f.node.state.watch(start) else {
		panic!("expected events")
	};
	assert!(!events.is_empty());
	let mut expected = start.revision;
	for item in events {
		expected = expected.next().unwrap();
		assert_eq!(item.revision, expected);
	}
	assert_eq!(expected, f.node.state.revision());
	// Nothing new: an empty, gap-free answer.
	let tail = f.node.state.snapshot().cursor();
	assert_eq!(f.node.state.watch(tail), Watch::Events(Vec::new()));
}

#[test]
fn slow_watcher_is_resynchronized_not_buffered_without_bound() {
	let limits = Limits {
		client_lifecycle_queue: 8,
		lifecycle_replay_events: 16,
		..Limits::default()
	};
	let mut node = Node::with_limits(1, limits);
	let client = node.attach_client();
	let mut watcher = FakeWatcher::attach(&node.state);
	let (call, lease) = common::activate(&mut node, client, common::peer());
	for i in 0..20 {
		node.command(
			client,
			Command::SetCallControls {
				call,
				lease,
				muted: Some(i % 2 == 0),
				deafened: None,
				gain: None,
			},
		)
		.unwrap();
	}
	watcher.sync(&node.state);
	assert_eq!(watcher.resyncs, 1);
	assert_eq!(watcher.view, node.state.snapshot());
	// After resync it continues without gaps.
	node.command(
		client,
		Command::SetCallControls {
			call,
			lease,
			muted: Some(false),
			deafened: None,
			gain: None,
		},
	)
	.unwrap();
	watcher.sync(&node.state);
	assert_eq!(watcher.resyncs, 1);
	assert_eq!(watcher.view, node.state.snapshot());
}

#[test]
fn cursor_from_another_incarnation_or_the_future_forces_a_snapshot() {
	let f = Fixture::new();
	let mut cursor = f.node.state.snapshot().cursor();
	cursor.incarnation = Incarnation::from_u128(0xdead);
	assert!(matches!(f.node.state.watch(cursor), Watch::Resync(_)));
	let mut future = f.node.state.snapshot().cursor();
	future.revision = Revision::new(future.revision.get() + 10);
	assert!(matches!(f.node.state.watch(future), Watch::Resync(_)));
}

#[test]
fn folding_out_of_order_events_reports_a_gap() {
	let f = Fixture::new();
	let mut view = f.node.state.snapshot();
	let skipped = Revisioned {
		revision: Revision::new(view.revision.get() + 2),
		event: Event::ClientDetached(ClientId::from_u128(1)),
	};
	assert!(view.apply(&skipped).is_err());
}

#[test]
fn independent_clients_see_the_same_independent_calls() {
	let mut node = Node::new(1);
	let (cli, gui) = (node.attach_client(), node.attach_client());
	let mut cli_view = FakeWatcher::attach(&node.state);
	let mut gui_view = FakeWatcher::attach(&node.state);
	let (a, _) = common::activate(&mut node, cli, common::peer());
	let (b, _) = common::activate(&mut node, gui, relay_core::testing::device_key(0xC0));
	cli_view.sync(&node.state);
	gui_view.sync(&node.state);
	assert_eq!(cli_view.view, gui_view.view);
	let snap = node.state.snapshot();
	assert_eq!(snap.calls[&a].controller.map(|c| c.0), Some(cli));
	assert_eq!(snap.calls[&b].controller.map(|c| c.0), Some(gui));
}

#[test]
fn restart_gives_a_new_incarnation_and_nothing_resurrects() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic_background());
	let old = f.node.state.snapshot().cursor();
	let fresh = State::new(
		common::peer(),
		Incarnation::from_u128(999),
		Limits::default(),
	)
	.unwrap();
	assert!(matches!(fresh.watch(old), Watch::Resync(_)));
	assert!(fresh.snapshot().calls.is_empty());
}
