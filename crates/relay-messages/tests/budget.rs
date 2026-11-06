mod common;

use common::*;
use relay_messages::limits::MESSAGE_OVERHEAD_BYTES;
use relay_messages::message::*;
use relay_messages::{HistoryManager, StoreError};
use std::sync::Arc;

#[test]
fn cost_is_payload_plus_overhead() {
	let (mut s, _) = store();
	s.compose_text("hello").unwrap();
	assert_eq!(s.used_bytes(), 5 + MESSAGE_OVERHEAD_BYTES);
	let bytes: Arc<[u8]> = png(100, 100).into();
	let n = bytes.len() as u64;
	s.compose_image(bytes).unwrap();
	assert_eq!(s.used_bytes(), 5 + n + 2 * MESSAGE_OVERHEAD_BYTES);
}

#[test]
fn rejects_at_limit_and_never_evicts() {
	let one = 10 + MESSAGE_OVERHEAD_BYTES;
	let (mut s, clock) = store_with(one * 3);
	for i in 0..3 {
		s.compose_text(&format!("message {i}")).unwrap();
		clock.advance(200);
	}
	let before: Vec<_> = s.messages().iter().map(|m| m.id).collect();
	let err = s.compose_text("message 3").unwrap_err();
	assert!(matches!(err, StoreError::BudgetExceeded { .. }));
	// Inbound is refused too, with the HistoryFull reason, and nothing is evicted.
	let r = s.receive(wire_text(1, "message x"));
	assert_eq!(
		r.as_ref().unwrap_err().reject_reason(),
		relay_messages::RejectReason::HistoryFull
	);
	assert_eq!(
		relay_messages::store::ack_status(&r),
		AckStatus::Rejected(relay_messages::RejectReason::HistoryFull)
	);
	let after: Vec<_> = s.messages().iter().map(|m| m.id).collect();
	assert_eq!(before, after);
	// Clearing frees the budget again.
	s.clear();
	assert_eq!(s.used_bytes(), 0);
	s.compose_text("message 3").unwrap();
}

#[test]
fn grows_on_demand_with_no_count_limit() {
	let (mut s, clock) = store();
	for i in 0..300u64 {
		s.receive(wire_text(i + 1, "x")).unwrap();
		clock.advance(1000);
	}
	assert_eq!(s.messages().len(), 300);
}

#[test]
fn preview_counts_toward_budget() {
	let data = b"hello";
	let (mut s, _) = store_with(MESSAGE_OVERHEAD_BYTES + 200 + 100);
	s.compose_offer(meta(7, "notes.txt", data)).unwrap();
	let before = s.used_bytes();
	s.attach_preview(OfferId(7), "line one\nline two").unwrap();
	assert_eq!(s.used_bytes(), before + 17);
	let big = "x".repeat(300);
	assert!(matches!(
		s.attach_preview(OfferId(7), &big),
		Err(StoreError::WrongState | StoreError::BudgetExceeded { .. })
	));
}

#[test]
fn global_eviction_drops_oldest_ended_first_and_spares_active() {
	let clock = TestClock::new();
	let per = 1000 + MESSAGE_OVERHEAD_BYTES;
	let mut h = HistoryManager::with_budget(clock.clone(), per * 2 + 10);
	for n in 1..=4u8 {
		h.start_session(info(n, n), Permissions::default());
		let s = h.get_mut(SessionId([n; 16])).unwrap();
		s.compose_text(&"a".repeat(1000)).unwrap();
	}
	// End 1, 2: fit. End 3: evicts 1 (oldest ended). Session 4 stays active throughout.
	assert!(h.end_session(SessionId([1; 16])).is_empty());
	assert!(h.end_session(SessionId([2; 16])).is_empty());
	let ev = h.end_session(SessionId([3; 16]));
	assert_eq!(ev, vec![SessionId([1; 16])]);
	assert!(h.get(SessionId([1; 16])).is_none());
	assert!(h.get(SessionId([2; 16])).is_some());
	assert!(h.get(SessionId([4; 16])).is_some_and(|s| !s.is_ended()));
	assert_eq!(h.ended_ids(), &[SessionId([2; 16]), SessionId([3; 16])]);
	assert!(h.retained_bytes() <= per * 2 + 10);
}

#[test]
fn active_sessions_are_never_evicted_even_over_budget() {
	let clock = TestClock::new();
	let mut h = HistoryManager::with_budget(clock, 10);
	h.start_session(info(1, 1), Permissions::default());
	h.get_mut(SessionId([1; 16]))
		.unwrap()
		.compose_text("hello")
		.unwrap();
	assert!(h.rebalance().is_empty());
	assert!(h.get(SessionId([1; 16])).is_some());
	// Ending it makes it evictable, and it alone exceeds the budget.
	assert_eq!(h.end_session(SessionId([1; 16])), vec![SessionId([1; 16])]);
}

#[test]
fn replacement_session_with_same_peer_drops_older_transcript() {
	let clock = TestClock::new();
	let mut h = HistoryManager::new(clock);
	h.start_session(info(1, 9), Permissions::default());
	h.end_session(SessionId([1; 16]));
	h.start_session(info(2, 8), Permissions::default());
	h.end_session(SessionId([2; 16]));
	let replaced = h.start_session(info(3, 9), Permissions::default());
	assert_eq!(replaced, vec![SessionId([1; 16])]);
	assert!(h.get(SessionId([1; 16])).is_none());
	assert!(h.get(SessionId([2; 16])).is_some());
}

#[test]
fn ended_transcript_stays_readable_until_cleared() {
	let clock = TestClock::new();
	let mut h = HistoryManager::new(clock);
	h.start_session(info(1, 1), Permissions::default());
	h.get_mut(SessionId([1; 16]))
		.unwrap()
		.compose_text("keep me")
		.unwrap();
	h.end_session(SessionId([1; 16]));
	let s = h.get_mut(SessionId([1; 16])).unwrap();
	assert_eq!(s.messages().len(), 1);
	assert_eq!(s.compose_text("late").unwrap_err(), StoreError::Ended);
	assert_eq!(
		s.receive(wire_text(1, "late")).unwrap_err(),
		StoreError::Ended
	);
	assert!(!s.export_json(Default::default()).is_empty());
	h.clear(SessionId([1; 16]));
	assert!(h.get(SessionId([1; 16])).is_none());
}

#[test]
fn clear_on_active_session_is_local_and_keeps_sequences() {
	let (mut s, _) = store();
	let w1 = s.compose_text("one").unwrap();
	s.clear();
	let w2 = s.compose_text("two").unwrap();
	assert_eq!(w2.seq, w1.seq + 1);
	assert_eq!(s.messages().len(), 1);
}

#[test]
fn pins_search_and_copy() {
	let (mut s, clock) = store();
	let a = s
		.compose_text("Run `Make Build` now\n```sh\nls -la\n```")
		.unwrap();
	clock.advance(500);
	s.compose_text("other").unwrap();
	let id = MessageId {
		origin: Origin::Local,
		seq: a.seq,
	};
	s.set_pinned(id, true).unwrap();
	assert_eq!(s.pinned(), vec![id]);
	assert_eq!(s.search("MAKE build").unwrap().hits, vec![id]);
	assert_eq!(s.copy_code_block(id, 0).as_deref(), Some("ls -la"));
	assert_eq!(s.copy_code_block(id, 1), None);
	assert!(s.copy_text(id).unwrap().starts_with("Run"));
	assert!(s.search("").unwrap().hits.is_empty());
	assert_eq!(
		s.search(&"q".repeat(1000)).unwrap_err(),
		StoreError::QueryTooLong
	);
}

#[test]
fn search_results_are_bounded() {
	let (mut s, clock) = store();
	for i in 0..150u64 {
		s.receive(wire_text(i + 1, "needle")).unwrap();
		clock.advance(1000);
	}
	let r = s.search("NEEDLE").unwrap();
	assert_eq!(r.hits.len(), 100);
	assert!(r.truncated);
}

#[test]
fn permissions_enforced_on_both_sides_and_keep_existing() {
	let (mut s, _) = store();
	s.compose_text("before").unwrap();
	s.set_permissions(Permissions {
		allow_messages: false,
		allow_files: true,
	});
	assert_eq!(s.compose_text("x").unwrap_err(), StoreError::Disabled);
	assert_eq!(
		s.receive(wire_text(1, "x")).unwrap_err(),
		StoreError::Disabled
	);
	assert_eq!(s.messages().len(), 1);
	s.set_permissions(Permissions {
		allow_messages: true,
		allow_files: false,
	});
	let img: Arc<[u8]> = png(10, 10).into();
	assert_eq!(
		s.compose_image(img.clone()).unwrap_err(),
		StoreError::Disabled
	);
	let r = s.receive(Wire {
		seq: 2,
		sent_at_ms: 0,
		payload: Payload::Image(img),
	});
	assert_eq!(r.unwrap_err(), StoreError::Disabled);
	let r = s.receive(Wire {
		seq: 3,
		sent_at_ms: 0,
		payload: Payload::Offer(meta(1, "a.txt", b"x")),
	});
	assert_eq!(r.unwrap_err(), StoreError::Disabled);
}

#[test]
fn offer_count_is_limited() {
	let (mut s, clock) = store();
	let mut n = 0u64;
	loop {
		clock.advance(1000);
		n += 1;
		let r = s.receive(Wire {
			seq: n,
			sent_at_ms: 0,
			payload: Payload::Offer(meta(u128::from(n), "a.txt", b"x")),
		});
		if let Err(e) = r {
			assert_eq!(e, StoreError::TooManyOffers);
			break;
		}
	}
	assert_eq!(n, relay_messages::limits::MAX_OFFERS_PER_SESSION as u64 + 1);
}

#[test]
fn a_new_session_limit_applies_to_new_data_and_never_deletes() {
	let (mut s, _) = store_with(10_000);
	s.compose_text("hello").unwrap();
	let used = s.used_bytes();
	s.set_limit_bytes(used);
	assert_eq!(s.limit_bytes(), used);
	assert!(matches!(
		s.compose_text("more"),
		Err(StoreError::BudgetExceeded { .. })
	));
	assert_eq!(s.messages().len(), 1, "stored messages stay");
	s.set_limit_bytes(10_000);
	s.compose_text("more").unwrap();
}

#[test]
fn a_new_retained_budget_evicts_oldest_ended_at_once() {
	let clock = TestClock::new();
	let per = 1000 + MESSAGE_OVERHEAD_BYTES;
	let mut h = HistoryManager::with_budget(clock, per * 4);
	for n in 1..=3u8 {
		h.start_session(info(n, n), Permissions::default());
		h.get_mut(SessionId([n; 16]))
			.unwrap()
			.compose_text(&"a".repeat(1000))
			.unwrap();
		assert!(h.end_session(SessionId([n; 16])).is_empty());
	}
	assert_eq!(h.largest_live_bytes(), 0);
	let evicted = h.set_budget(per * 2);
	assert_eq!(evicted, vec![SessionId([1; 16])]);
	assert!(h.retained_bytes() <= per * 2);
	h.start_session(info(9, 9), Permissions::default());
	h.get_mut(SessionId([9; 16]))
		.unwrap()
		.compose_text("hi")
		.unwrap();
	assert_eq!(h.largest_live_bytes(), 2 + MESSAGE_OVERHEAD_BYTES);
	h.set_session_limit(1);
	assert_eq!(h.get(SessionId([9; 16])).unwrap().limit_bytes(), 1);
	assert_eq!(h.get(SessionId([2; 16])).unwrap().limit_bytes(), 1);
}
