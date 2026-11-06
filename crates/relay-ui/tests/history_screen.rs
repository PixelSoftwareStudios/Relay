//! The History section of the Calls page on screen: always present, a row per outcome, Call
//! back through the controller, Show more, a confirmed Clear and the Settings switch.

mod common;
mod phone;

use std::sync::Arc;
use std::time::Duration;

use common::FakeOps;
use phone::{LANGUAGES, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome, Link};

/// 2026-11-05 12:00:00 UTC.
const NOW: u64 = 1_793_880_000_000;

fn entry(id: u64, peer: u8, direction: HistoryDirection, outcome: HistoryOutcome) -> HistoryEntry {
	HistoryEntry {
		id,
		call_id: hex::encode([id as u8; 16]),
		peer_id: hex::encode([peer; 32]),
		peer_name: format!("Caller {id}"),
		fingerprint_short: String::new(),
		direction,
		outcome,
		started_at_ms: NOW - 5 * 60_000,
		duration_ms: 65_000,
	}
}

fn every_outcome() -> Vec<HistoryEntry> {
	use HistoryDirection::{Incoming, Outgoing};
	use HistoryOutcome::*;
	vec![
		entry(6, 2, Incoming, Completed),
		entry(5, 2, Outgoing, Completed),
		entry(4, 2, Incoming, Missed),
		entry(3, 2, Outgoing, Declined),
		entry(2, 2, Outgoing, Failed),
		entry(1, 2, Outgoing, Cancelled),
	]
}

fn many(count: u64) -> Vec<HistoryEntry> {
	(1..=count)
		.rev()
		.map(|i| entry(i, 2, HistoryDirection::Incoming, HistoryOutcome::Completed))
		.collect()
}

fn phone_with(history: Vec<HistoryEntry>) -> Phone {
	let mut phone = Phone::new(360.0, 14000.0, Language::En);
	phone
		.shell
		.now_ms
		.store(NOW, std::sync::atomic::Ordering::SeqCst);
	let mut state = phone::busy_state();
	state.history = history;
	phone.show_state(&state);
	phone.settle();
	phone
}

fn count(labels: &[String], text: &str) -> usize {
	labels.iter().filter(|l| l.as_str() == text).count()
}

async fn until(mut done: impl FnMut() -> bool) {
	for _ in 0..100 {
		if done() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("condition not reached in time");
}

fn connected(phone: &Phone) -> Arc<FakeOps> {
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	ops
}

#[tokio::test]
async fn the_section_is_always_there_and_says_so_when_empty() {
	let mut cases: Vec<AppState> = Vec::new();
	let busy = phone::busy_state();
	cases.push(busy.clone());
	let mut nobody = busy.clone();
	nobody.peers.clear();
	nobody.calls.clear();
	cases.push(nobody);
	let mut offline = busy;
	offline.link = Link::Offline;
	offline.settings = None;
	cases.push(offline);
	for state in cases {
		let mut phone = Phone::new(360.0, 3000.0, Language::En);
		phone.show_state(&state);
		let drawn = phone.page(Page::Calls);
		assert!(
			drawn.labels.contains(&"History".to_owned()),
			"{:?}",
			drawn.labels
		);
		assert!(
			drawn.labels.contains(&"No call history yet".to_owned()),
			"{:?}",
			drawn.labels
		);
		assert_eq!(count(&drawn.labels, "Clear history"), 0, "nothing to clear");
	}
}

#[tokio::test]
async fn a_row_per_outcome_with_its_marker_time_and_length() {
	let mut phone = phone_with(every_outcome());
	let drawn = phone.page(Page::Calls);
	for word in [
		"Incoming",
		"Outgoing",
		"Missed",
		"Declined",
		"Not reached",
		"Cancelled",
	] {
		assert_eq!(count(&drawn.labels, word), 1, "{word}: {:?}", drawn.labels);
	}
	for id in 1..=6 {
		assert!(drawn.labels.contains(&format!("Caller {id}")));
	}
	let times = drawn
		.labels
		.iter()
		.filter(|l| l.starts_with("5 min ago"))
		.count();
	assert_eq!(times, 6, "{:?}", drawn.labels);
	// Only the two calls that connected show a length.
	let lengths = drawn
		.labels
		.iter()
		.filter(|l| l.ends_with(" · 1:05"))
		.count();
	assert_eq!(lengths, 2, "{:?}", drawn.labels);
	assert!(!drawn.labels.contains(&"No call history yet".to_owned()));
}

#[tokio::test]
async fn call_back_uses_the_same_path_as_calling_from_people() {
	let mut phone = phone_with(every_outcome());
	let ops = connected(&phone);
	phone.click_label("Call back");
	until(|| !ops.recorded().is_empty()).await;
	assert_eq!(
		ops.recorded(),
		[Command::StartCall {
			peer_id: vec![2; 32]
		}]
	);
}

#[tokio::test]
async fn call_back_is_disabled_with_the_reason_when_the_device_cannot_be_called() {
	use HistoryDirection::Outgoing;
	use HistoryOutcome::Completed;
	// Device 1 is already in a call, 3 is unreachable and 9 is not known.
	let mut phone = phone_with(vec![
		entry(3, 1, Outgoing, Completed),
		entry(2, 3, Outgoing, Completed),
		entry(1, 9, Outgoing, Completed),
	]);
	let ops = connected(&phone);
	let drawn = phone.page(Page::Calls);
	assert_eq!(count(&drawn.labels, "Call back"), 3);
	assert!(
		drawn
			.labels
			.contains(&"This device cannot be called right now.".to_owned())
	);
	assert!(drawn.labels.contains(&"Not reachable".to_owned()));
	phone.click_label("Call back");
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert!(ops.recorded().is_empty(), "{:?}", ops.recorded());
}

#[tokio::test]
async fn twenty_rows_then_show_more() {
	let mut phone = phone_with(many(45));
	let drawn = phone.page(Page::Calls);
	assert_eq!(count(&drawn.labels, "Call back"), 20);
	assert_eq!(count(&drawn.labels, "Show more"), 1);
	phone.click_label("Show more");
	let drawn = phone.settle();
	assert_eq!(count(&drawn.labels, "Call back"), 40);
	phone.click_label("Show more");
	let drawn = phone.settle();
	assert_eq!(count(&drawn.labels, "Call back"), 45);
	assert_eq!(count(&drawn.labels, "Show more"), 0);
}

#[tokio::test]
async fn clearing_asks_first_and_then_deletes() {
	let mut phone = phone_with(every_outcome());
	let ops = connected(&phone);
	phone.click_label("Clear history");
	let drawn = phone.settle();
	assert!(
		drawn
			.labels
			.contains(&"Delete the whole call history on this device?".to_owned())
	);
	assert!(ops.recorded().is_empty(), "asking deletes nothing");
	phone.click_label("Keep");
	let drawn = phone.settle();
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Delete the whole"))
	);
	assert!(ops.recorded().is_empty());
	phone.click_label("Clear history");
	phone.click_label("Clear history");
	until(|| !ops.recorded().is_empty()).await;
	assert_eq!(ops.recorded(), [Command::ClearCallHistory]);
}

#[tokio::test]
async fn a_switched_off_history_says_that_entries_stay() {
	let mut phone = phone_with(every_outcome());
	let mut state = phone::busy_state();
	state.history = every_outcome();
	state.settings.as_mut().unwrap().history_enabled = false;
	phone.show_state(&state);
	let drawn = phone.page(Page::Calls);
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Call history is off."))
	);
	assert_eq!(
		count(&drawn.labels, "Missed"),
		1,
		"entries are still listed"
	);
}

#[tokio::test]
async fn the_settings_switch_sends_the_key_and_shows_the_answer() {
	let mut phone = Phone::new(360.0, 3000.0, Language::En);
	let ops = connected(&phone);
	phone.app.open_settings(Some(SettingsCategory::Privacy));
	let drawn = phone.settle();
	assert!(
		drawn.labels.contains(&"Keep call history".to_owned()),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Keep call history");
	until(|| !ops.settings_sent.lock().unwrap().is_empty()).await;
	assert_eq!(
		*ops.settings_sent.lock().unwrap(),
		[vec![("history.enabled".to_owned(), "false".to_owned())]]
	);
	let mut drawn = phone.settle();
	for _ in 0..20 {
		if drawn.labels.iter().any(|l| l.contains("applied now")) {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
		drawn = phone.settle();
	}
	assert!(
		drawn
			.labels
			.contains(&"Keep call history: applied now".to_owned()),
		"{:?}",
		drawn.labels
	);
}

fn long_names(mut history: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
	for e in &mut history {
		e.peer_name = "Aleksandra Konstantinopolskaya-Wolfeschlegelstein".into();
	}
	history
}

#[tokio::test]
async fn nothing_overflows_and_touch_targets_stay_large() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (desktop, w, h) in [(true, 720.0, 5000.0), (false, 360.0, 5000.0)] {
			for history in [
				Vec::new(),
				long_names(every_outcome()),
				long_names(many(30)),
			] {
				let mut phone = if desktop {
					Phone::desktop(w, h, language)
				} else {
					Phone::new(w, h, language)
				};
				phone
					.shell
					.now_ms
					.store(NOW, std::sync::atomic::Ordering::SeqCst);
				let mut state = phone::busy_state();
				state.history = history.clone();
				phone.show_state(&state);
				let mut pages = vec![phone.page(Page::Calls)];
				phone.app.open_settings(Some(SettingsCategory::Privacy));
				pages.push(phone.settle());
				for drawn in pages {
					let screen = phone.screen().expand(1.0);
					let tag = format!("{language:?} {w}x{h} rows={}", history.len());
					for r in drawn.widgets.iter().chain(&drawn.painted) {
						if r.min.x < screen.min.x || r.max.x > screen.max.x {
							bad.push(format!("{tag}: overflow {r:?}"));
						}
					}
					if !desktop {
						for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
							if r.height() < 43.5 || r.width() < 43.5 {
								bad.push(format!("{tag}: small target {:?}", r.size()));
							}
						}
					}
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}
