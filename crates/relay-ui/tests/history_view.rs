//! The rows of the call history: marker words, emphasis, durations, call-back availability
//! and the page size.

use relay_io::localtime::LocalZone;
use relay_ui::history_time::Relative;
use relay_ui::history_view::{Blocked, PAGE, has_more, marker, rows};
use relay_ui::i18n::{self, Language};
use relay_ui::mock::seed;
use relay_ui::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome};

const NOW: u64 = 1_793_880_000_000;

fn entry(
	id: u64,
	peer: u8,
	direction: HistoryDirection,
	outcome: HistoryOutcome,
	ms: u64,
) -> HistoryEntry {
	HistoryEntry {
		id,
		call_id: hex::encode([id as u8; 16]),
		peer_id: hex::encode([peer; 32]),
		peer_name: format!("Name {id}"),
		fingerprint_short: String::new(),
		direction,
		outcome,
		started_at_ms: NOW - 5 * 60_000,
		duration_ms: ms,
	}
}

fn state(history: Vec<HistoryEntry>) -> AppState {
	let mut state = seed();
	state.history = history;
	state
}

#[test]
fn every_outcome_has_its_marker_word() {
	use HistoryDirection::{Incoming, Outgoing};
	use HistoryOutcome::*;
	i18n::set_active(Language::En);
	let state = state(vec![
		entry(1, 2, Incoming, Completed, 61_000),
		entry(2, 2, Outgoing, Completed, 5_000),
		entry(3, 2, Incoming, Missed, 0),
		entry(4, 2, Outgoing, Declined, 0),
		entry(5, 2, Outgoing, Failed, 0),
		entry(6, 2, Outgoing, Cancelled, 0),
	]);
	let rows = rows(&state, PAGE, NOW, &LocalZone::utc());
	let words: Vec<_> = rows.iter().map(|r| r.marker).collect();
	assert_eq!(
		words,
		[
			"Incoming",
			"Outgoing",
			"Missed",
			"Declined",
			"Not reached",
			"Cancelled"
		]
	);
	let missed: Vec<_> = rows.iter().map(|r| r.missed).collect();
	assert_eq!(missed, [false, false, true, false, false, false]);
	let lengths: Vec<_> = rows.iter().map(|r| r.duration.as_deref()).collect();
	assert_eq!(
		lengths,
		[Some("1:01"), Some("0:05"), None, None, None, None]
	);
	assert_eq!(rows[0].when, Relative::At("11:55 AM".into()));
	assert!(rows[0].when_hover.starts_with("2026-11-05 11:55:00 +00:00"));
	assert_eq!(rows[0].name, "Name 1");
	assert_eq!(marker(&state.history[2]), "Missed");
}

#[test]
fn call_back_is_available_only_for_a_device_that_can_be_called() {
	use HistoryDirection::Outgoing;
	use HistoryOutcome::Completed;
	// In the seed: device 1 is reachable but already in a call, 2 can be called, 3 is
	// unreachable with a changed key; 9 is not known at all.
	let state = state(vec![
		entry(1, 2, Outgoing, Completed, 1000),
		entry(2, 1, Outgoing, Completed, 1000),
		entry(3, 3, Outgoing, Completed, 1000),
		entry(4, 9, Outgoing, Completed, 1000),
	]);
	let rows = rows(&state, PAGE, NOW, &LocalZone::utc());
	let blocked: Vec<_> = rows.iter().map(|r| r.blocked).collect();
	assert_eq!(
		blocked,
		[
			None,
			Some(Blocked::Cannot),
			Some(Blocked::Offline),
			Some(Blocked::Offline)
		]
	);
	assert!(rows[0].avatar.is_some() && rows[3].avatar.is_none());
}

#[test]
fn only_a_page_is_shown_and_more_can_be_asked_for() {
	use HistoryDirection::Incoming;
	use HistoryOutcome::Missed;
	let all: Vec<_> = (0..45u64)
		.rev()
		.map(|i| entry(i + 1, 2, Incoming, Missed, 0))
		.collect();
	let state = state(all);
	assert_eq!(PAGE, 20);
	assert_eq!(rows(&state, PAGE, NOW, &LocalZone::utc()).len(), 20);
	assert!(has_more(&state, PAGE));
	assert_eq!(rows(&state, 2 * PAGE, NOW, &LocalZone::utc()).len(), 40);
	assert!(has_more(&state, 2 * PAGE));
	assert_eq!(rows(&state, 3 * PAGE, NOW, &LocalZone::utc()).len(), 45);
	assert!(!has_more(&state, 3 * PAGE));
	// Newest first is the engine's order and is kept.
	assert_eq!(rows(&state, 1, NOW, &LocalZone::utc())[0].id, 45);
}
