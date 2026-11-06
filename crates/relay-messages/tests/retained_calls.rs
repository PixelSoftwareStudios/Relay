mod common;

use common::*;
use relay_messages::HistoryManager;
use relay_messages::limits::MESSAGE_OVERHEAD_BYTES;
use relay_messages::message::*;

fn id(n: u8) -> SessionId {
	SessionId([n; 16])
}

/// A manager with `count` sessions, each with its own peer, all still running.
fn running(count: u8, budget: u64) -> HistoryManager {
	let mut h = HistoryManager::with_budget(TestClock::new(), budget);
	for n in 1..=count {
		h.start_session(info(n, n), Permissions::default());
		h.get_mut(id(n)).unwrap().compose_text("hello").unwrap();
	}
	h
}

#[test]
fn the_fourth_ended_call_evicts_the_first() {
	let mut h = running(4, u64::MAX);
	h.set_max_ended(3);
	for n in 1..=3 {
		assert!(h.end_session(id(n)).is_empty());
	}
	assert_eq!(h.end_session(id(4)), vec![id(1)]);
	assert!(h.get(id(1)).is_none());
	assert_eq!(h.ended_ids(), &[id(2), id(3), id(4)]);
}

#[test]
fn zero_keeps_nothing_once_a_call_ends() {
	let mut h = running(2, u64::MAX);
	h.set_max_ended(0);
	assert_eq!(h.end_session(id(1)), vec![id(1)]);
	assert!(h.get(id(1)).is_none());
	assert!(h.get(id(2)).is_some_and(|s| !s.is_ended()), "running chats stay");
	assert!(h.ended_ids().is_empty());
}

#[test]
fn the_byte_budget_still_applies_when_it_is_stricter() {
	let per = 5 + MESSAGE_OVERHEAD_BYTES;
	let mut h = running(3, per * 2);
	h.set_max_ended(10);
	assert!(h.end_session(id(1)).is_empty());
	assert!(h.end_session(id(2)).is_empty());
	assert_eq!(h.end_session(id(3)), vec![id(1)], "bytes evict before the count");
}

#[test]
fn the_count_applies_when_it_is_stricter_than_the_bytes() {
	let mut h = running(3, u64::MAX);
	h.set_max_ended(2);
	h.end_session(id(1));
	h.end_session(id(2));
	assert_eq!(h.end_session(id(3)), vec![id(1)]);
}

#[test]
fn lowering_the_count_evicts_the_oldest_at_once() {
	let mut h = running(4, u64::MAX);
	for n in 1..=4 {
		h.end_session(id(n));
	}
	assert_eq!(h.ended_ids().len(), 4);
	assert_eq!(h.set_max_ended(2), vec![id(1), id(2)]);
	assert_eq!(h.ended_ids(), &[id(3), id(4)]);
	assert!(h.set_max_ended(5).is_empty(), "raising removes nothing");
}
