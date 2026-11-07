//! A history row reads like a phone's call log: an arrow in, an arrow out or a crossed phone
//! for every call that never connected, the time as quiet text and the length as its own badge
//! on the right, the name as the way into the contact, and a View Messages button that looks
//! like the Call back button beside it.

// The turn lock is held on purpose across awaits: the active language is process-wide.
#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use phone::{Drawn, Phone};
use relay_ui::history_view;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome, Trust};

/// 2026-11-05 12:00:00 UTC.
const NOW: u64 = 1_793_880_000_000;

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn entry(id: u64, direction: HistoryDirection, outcome: HistoryOutcome) -> HistoryEntry {
	HistoryEntry {
		id,
		call_id: hex::encode([id as u8; 16]),
		peer_id: hex::encode([2; 32]),
		peer_name: format!("Caller {id}"),
		fingerprint_short: String::new(),
		direction,
		outcome,
		started_at_ms: NOW - 5 * 60_000,
		duration_ms: 65_000,
	}
}

fn all() -> Vec<HistoryEntry> {
	use HistoryDirection::{Incoming, Outgoing};
	use HistoryOutcome::*;
	vec![
		entry(6, Incoming, Completed),
		entry(5, Outgoing, Completed),
		entry(4, Incoming, Missed),
		entry(3, Outgoing, Declined),
		entry(2, Outgoing, Failed),
		entry(1, Outgoing, Cancelled),
	]
}

fn rig(width: f32) -> Phone {
	let mut phone = Phone::new(width, 4200.0, Language::En);
	phone
		.shell
		.now_ms
		.store(NOW, std::sync::atomic::Ordering::SeqCst);
	let mut state: AppState = phone::busy_state();
	state.peers[0].peer_id = hex::encode([2; 32]);
	state.peers[0].trust = Trust::Verified;
	state.history = all();
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	phone.open_history();
	phone
}

fn count(drawn: &Drawn, icon: Icon) -> usize {
	phone::icon_rects(drawn, icon).len()
}

#[test]
fn every_outcome_has_its_call_log_icon_and_unanswered_ones_are_red() {
	let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
	let mut state = phone::busy_state();
	state.history = all();
	let rows = history_view::rows(&state, 10, NOW, &relay_io::localtime::LocalZone::utc());
	let icons: Vec<Icon> = rows.iter().map(|r| r.icon).collect();
	assert_eq!(
		icons,
		[
			Icon::CallIncoming,
			Icon::CallOutgoing,
			Icon::CallMissed,
			Icon::CallMissed,
			Icon::CallMissed,
			Icon::CallMissed
		]
	);
	let unanswered: Vec<bool> = rows.iter().map(|r| r.unanswered).collect();
	assert_eq!(unanswered, [false, false, true, true, true, true]);
}

#[tokio::test]
async fn the_icons_are_drawn_and_the_length_is_a_badge_apart_from_the_time() {
	let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
	let mut phone = rig(360.0);
	let drawn = phone.page(Page::Calls);
	assert_eq!(count(&drawn, Icon::CallIncoming), 1, "{:?}", drawn.labels);
	assert_eq!(count(&drawn, Icon::CallOutgoing), 1);
	assert_eq!(count(&drawn, Icon::CallMissed), 4);
	assert_eq!(
		count(&drawn, Icon::Clock),
		2,
		"a badge for each connected call"
	);
	let badge = drawn
		.named
		.iter()
		.find(|(t, _)| t == "1:05")
		.expect("the length")
		.1;
	let time = drawn
		.named
		.iter()
		.find(|(t, _)| t.ends_with(" AM") || t.ends_with(" PM"))
		.unwrap_or_else(|| panic!("the time in {:?}", drawn.labels))
		.1;
	assert!(
		badge.min.x > time.max.x,
		"the length sits right of the time"
	);
	assert!(badge.max.x > 360.0 * 0.75, "at the right edge of the card");
	assert!(
		!drawn.labels.iter().any(|l| l.contains(" · 1:05")),
		"no longer glued to the time"
	);
}

#[tokio::test]
async fn the_name_opens_the_contact_and_there_is_no_open_contact_button() {
	let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
	let mut phone = rig(360.0);
	let drawn = phone.page(Page::Calls);
	assert!(!drawn.labels.iter().any(|l| l == "Open contact"));
	let name = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Caller 6")
		.expect("the name")
		.1;
	let target = drawn
		.tappable
		.iter()
		.find(|r| r.contains(name.center()))
		.expect("the name can be pressed");
	assert!(target.height() >= 43.5, "touch sized: {target:?}");
	phone.click_label("Caller 6");
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn a_name_that_is_no_contact_is_plain_text() {
	let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
	let mut phone = rig(360.0);
	let mut state = phone::busy_state();
	state.history = all();
	for e in &mut state.history {
		e.peer_id = hex::encode([99; 32]);
	}
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	let drawn = phone.page(Page::Calls);
	let name = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Caller 6")
		.expect("the name")
		.1;
	assert!(!drawn.tappable.iter().any(|r| r.contains(name.center())));
}

#[tokio::test]
async fn view_messages_is_a_normal_button_the_height_of_call_back() {
	let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
	let mut phone = rig(360.0);
	let drawn = phone.page(Page::Calls);
	let rect_of = |label: &str| {
		let text = drawn.named.iter().find(|(t, _)| t == label).unwrap().1;
		*drawn
			.widgets
			.iter()
			.filter(|w| w.contains(text.center()) && w.height() < 80.0)
			.min_by(|a, b| a.area().total_cmp(&b.area()))
			.unwrap()
	};
	let (back, view) = (rect_of("Call back"), rect_of("View Messages"));
	assert!(
		(back.height() - view.height()).abs() < 0.6,
		"{back:?} {view:?}"
	);
	assert!(view.height() >= 43.5);
}
