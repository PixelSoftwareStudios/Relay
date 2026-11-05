//! `relay messages watch`: lines, JSON lines, resync handling.

mod common;

use common::chat::SESSION_A;
use common::{FakeControl, run_messages};
use relay_api::SessionItem;
use relay_api::pb::session_event::Event;
use relay_api::pb::*;

fn event(revision: u64, event: Event) -> Result<SessionItem, relay_api::ApiError> {
	Ok(SessionItem::Event {
		revision,
		event: Box::new(SessionEvent { event: Some(event) }),
	})
}

fn list(c: &FakeControl) -> Result<SessionItem, relay_api::ApiError> {
	let l = c.chat.list();
	Ok(SessionItem::List(SessionList {
		sessions: l.sessions,
		settings: l.settings,
	}))
}

/// An entry the engine announces; it is not in the fake's transcript until a test puts it there.
fn entry_event(
	ordinal: u64,
	text: &str,
	revision: u64,
) -> Result<SessionItem, relay_api::ApiError> {
	let e = TranscriptEntry {
		ordinal,
		seq: ordinal,
		delivery: DeliveryState::Delivered as i32,
		received_at_unix_ms: 1_700_000_000_000,
		body: Some(relay_api::pb::transcript_entry::Body::Text(text.into())),
		..TranscriptEntry::default()
	};
	event(
		revision,
		Event::Entry(NewEntry {
			session_id: SESSION_A.to_vec(),
			entry: Some(e),
		}),
	)
}

#[tokio::test]
async fn watch_prints_new_entries_not_history_and_ends_when_the_stream_closes() {
	let mut c = FakeControl::new();
	c.chat.incoming_text(&SESSION_A, "old news");
	let first = list(&c);
	let new = entry_event(2, "fresh ```x```", 12);
	let delivered = event(
		13,
		Event::Delivery(DeliveryChanged {
			session_id: SESSION_A.to_vec(),
			seq: 1,
			delivery: DeliveryState::Failed as i32,
			failure: "timeout".into(),
		}),
	);
	c.session_watches.push_back(vec![first, new, delivered]);
	let ran = run_messages(&mut c, &["watch"], false).await;
	let out = ran.out.clone();
	assert!(out.contains("watching: Alice (active)"), "{out}");
	assert!(
		out.contains("[Alice] #2 ") && out.contains("fresh"),
		"{out}"
	);
	assert!(!out.contains("old news"), "{out}");
	assert!(
		out.contains("[Alice] sent seq 1 is failed (timeout)"),
		"{out}"
	);
	assert!(
		ran.result.is_err(),
		"a closed stream is an error, not a silent success"
	);
}

#[tokio::test]
async fn watch_json_lines_have_an_event_tag() {
	let mut c = FakeControl::new();
	let first = list(&c);
	let new = entry_event(1, "hi", 11);
	c.session_watches.push_back(vec![first, new]);
	let ran = run_messages(&mut c, &["watch"], true).await;
	let lines: Vec<serde_json::Value> = ran
		.out
		.lines()
		.map(|l| serde_json::from_str(l).unwrap())
		.collect();
	assert_eq!(lines[0]["event"], "sessions");
	assert_eq!(lines[1]["event"], "entry");
	assert_eq!(lines[1]["entry"]["text"], "hi");
	assert_eq!(lines[1]["peer"], "Alice");
}

#[tokio::test]
async fn a_resync_reopens_the_watch_and_catches_up_without_repeats() {
	let mut c = FakeControl::new();
	let first = list(&c);
	let seen = entry_event(1, "before the gap", 11);
	c.session_watches.push_back(vec![
		first,
		seen,
		Ok(SessionItem::Resync(SessionResync {
			current_revision: 40,
			reason: "lagged".into(),
		})),
	]);
	// While the stream was down the engine kept both; only the transcript has them.
	c.on_watch_open.push_back(Box::new(|_| {}));
	c.on_watch_open.push_back(Box::new(|chat| {
		chat.incoming_text(&SESSION_A, "before the gap");
		chat.incoming_text(&SESSION_A, "during the gap");
	}));
	let second = list(&c);
	let after = entry_event(3, "after the gap", 42);
	c.session_watches.push_back(vec![second, after]);
	let ran = run_messages(&mut c, &["watch"], false).await;
	let out = ran.out;
	let order: Vec<usize> = [
		"before the gap",
		"-- resync: lagged",
		"during the gap",
		"after the gap",
	]
	.iter()
	.map(|needle| {
		out.find(needle)
			.unwrap_or_else(|| panic!("{needle} missing in {out}"))
	})
	.collect();
	assert!(order.windows(2).all(|w| w[0] < w[1]), "{out}");
	assert_eq!(out.matches("before the gap").count(), 1, "{out}");
	assert_eq!(out.matches("after the gap").count(), 1, "{out}");
	assert_eq!(
		c.chat.log.iter().filter(|l| l.starts_with("watch")).count(),
		2
	);
}

#[tokio::test]
async fn a_revision_gap_is_handled_like_a_resync() {
	let mut c = FakeControl::new();
	let first = list(&c);
	c.session_watches.push_back(vec![
		first,
		Err(relay_api::ApiError::RevisionGap {
			expected: 12,
			received: 20,
		}),
	]);
	let second = list(&c);
	c.session_watches.push_back(vec![second]);
	let ran = run_messages(&mut c, &["watch"], false).await;
	assert!(ran.out.contains("-- resync:"), "{}", ran.out);
}

#[tokio::test]
async fn permission_pin_offer_and_clear_events_are_reported() {
	let mut c = FakeControl::new();
	let first = list(&c);
	let items = vec![
		first,
		event(
			11,
			Event::Permissions(PermissionsChanged {
				session_id: SESSION_A.to_vec(),
				permissions: Some(SessionPermissions {
					allow_messages: true,
					allow_files: false,
				}),
				by_peer: true,
			}),
		),
		event(
			12,
			Event::Pin(PinChanged {
				session_id: SESSION_A.to_vec(),
				message: Some(MessageRef {
					ordinal: 3,
					outgoing: false,
					seq: 3,
				}),
				pinned: true,
			}),
		),
		event(
			13,
			Event::Offer(OfferChanged {
				session_id: SESSION_A.to_vec(),
				ordinal: 4,
				offer: Some(FileOfferBody {
					offer_id: vec![0xab; 16],
					name: "a.zip".into(),
					size: 1000,
					transferred_bytes: 250,
					state: OfferState::InProgress as i32,
					..FileOfferBody::default()
				}),
			}),
		),
		event(
			14,
			Event::Cleared(SessionCleared {
				session_id: SESSION_A.to_vec(),
				removed: false,
				next_ordinal: 9,
			}),
		),
	];
	c.session_watches.push_back(items);
	let out = run_messages(&mut c, &["watch"], false).await.out;
	assert!(
		out.contains("permissions: messages on, files off (set by the peer)"),
		"{out}"
	);
	assert!(out.contains("#3 pinned"), "{out}");
	assert!(
		out.contains("incoming offer abababab \"a.zip\"")
			|| out.contains("outgoing offer abababab"),
		"{out}"
	);
	assert!(out.contains("25%"), "{out}");
	assert!(out.contains("transcript cleared"), "{out}");
}
