//! Navigation around calls: placing a call and accepting one go to Calls; a
//! call this client controls that connects brings the user to Calls once,
//! unless a dialog or a field in use is in the way; ending a call moves nobody.

mod phone;

use std::sync::atomic::Ordering;

use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::mock::seed;
use relay_ui::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome, Phase};

const CALL: usize = 7;

/// The mock state with exactly one call of `phase`, controlled by this client.
fn with_call(phase: Phase, mine: bool) -> AppState {
	let mut state = seed();
	let mut call = state.calls[0].clone();
	call.call_id = format!("{CALL:02x}").repeat(16);
	call.phase = phase;
	call.controlled_by_me = mine;
	state.calls = vec![call];
	state
}

/// The mock state without any call.
fn quiet() -> AppState {
	let mut state = seed();
	state.calls.clear();
	state
}

fn rigs() -> Vec<Phone> {
	vec![
		Phone::new(360.0, 2400.0, Language::En),
		Phone::desktop(1000.0, 800.0, Language::En),
	]
}

#[tokio::test]
async fn placing_a_call_goes_to_calls() {
	for mut phone in rigs() {
		phone.show_state(&quiet());
		phone.page(Page::Contacts);
		phone.click_label("Call");
		assert_eq!(phone.app.page(), Page::Calls);
	}
}

#[tokio::test]
async fn placing_a_call_from_history_goes_to_calls() {
	let mut phone = Phone::new(360.0, 4000.0, Language::En);
	let mut state = quiet();
	state.history.push(HistoryEntry {
		id: 1,
		call_id: "ab".repeat(16),
		peer_id: state.peers[3].peer_id.clone(),
		peer_name: "Katherine Johnson".into(),
		fingerprint_short: String::new(),
		direction: HistoryDirection::Outgoing,
		outcome: HistoryOutcome::Completed,
		started_at_ms: 1_793_880_000_000,
		duration_ms: 60_000,
	});
	phone.show_state(&state);
	phone.open_history();
	phone.click_label("Call back");
	assert_eq!(phone.app.page(), Page::Calls);
}

#[tokio::test]
async fn the_other_side_accepting_brings_me_to_calls() {
	for from in [Page::Contacts, Page::Connect, Page::Channels] {
		for mut phone in rigs() {
			phone.show_state(&with_call(Phase::OutgoingRinging, true));
			phone.page(from);
			assert_eq!(phone.app.page(), from, "ringing does not navigate");
			phone.show_state(&with_call(Phase::Connected, true));
			phone.settle();
			assert_eq!(phone.app.page(), Page::Calls, "{from:?}");
		}
	}
}

#[tokio::test]
async fn settings_closes_for_a_connected_call() {
	for mut phone in rigs() {
		phone.show_state(&with_call(Phase::OutgoingRinging, true));
		phone.app.open_settings(Some(SettingsCategory::Calls));
		phone.settle();
		phone.show_state(&with_call(Phase::Connected, true));
		phone.settle();
		assert_eq!(phone.app.page(), Page::Calls);
	}
}

#[tokio::test]
async fn a_call_pulls_me_back_only_once() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.page(Page::Contacts);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Calls);
	phone.page(Page::Contacts);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn a_call_someone_else_controls_does_not_navigate() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, false));
	phone.page(Page::Contacts);
	phone.show_state(&with_call(Phase::Connected, false));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn accepting_an_incoming_call_goes_to_calls() {
	for mut phone in rigs() {
		phone.show_state(&with_call(Phase::IncomingRinging, true));
		phone.page(Page::Contacts);
		phone.click_label("Accept");
		assert_eq!(phone.app.page(), Page::Calls);
		phone.page(Page::Contacts);
		phone.show_state(&with_call(Phase::Connected, true));
		phone.settle();
		assert_eq!(phone.app.page(), Page::Contacts, "already followed");
	}
}

#[tokio::test]
async fn an_incoming_call_alone_does_not_navigate() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&quiet());
	phone.page(Page::Contacts);
	phone.show_state(&with_call(Phase::IncomingRinging, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn a_call_ending_moves_nobody() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	phone.page(Page::Contacts);
	let mut state = with_call(Phase::Connected, true);
	state.calls.clear();
	phone.show_state(&state);
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn typing_keeps_me_where_i_am() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.page(Page::Connect);
	phone.click_label("relay://192.168.1.20:7000#…");
	assert!(
		phone.ctx.memory(|m| m.focused()).is_some(),
		"the field has focus"
	);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Connect);
}

#[tokio::test]
async fn a_dialog_keeps_me_where_i_am() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.page(Page::Contacts);
	phone.click_icon(Icon::More);
	phone.click_label("Forget this device");
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
	assert!(phone.app.handle_back(), "the confirmation closes first");
	phone.settle();
	assert_eq!(
		phone.app.page(),
		Page::Calls,
		"then the waiting call is followed"
	);
}

#[tokio::test]
async fn a_chat_stays_open() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.settle();
	phone.app.open_chat("0a0b");
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(
		(phone.app.page(), phone.app.chat_call()),
		(Page::Calls, Some("0a0b"))
	);
}

#[tokio::test]
async fn a_hidden_window_waits_for_show() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.page(Page::Contacts);
	phone.shell.visible.store(false, Ordering::SeqCst);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Contacts);
	phone.shell.visible.store(true, Ordering::SeqCst);
	phone.settle();
	assert_eq!(phone.app.page(), Page::Calls);
}

#[tokio::test]
async fn back_from_calls_still_leaves_the_app() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.show_state(&with_call(Phase::OutgoingRinging, true));
	phone.page(Page::Contacts);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Calls);
	assert!(!phone.app.handle_back());
}

#[tokio::test]
async fn a_call_already_connected_when_the_app_starts_does_not_navigate() {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	phone.show_state(&with_call(Phase::Connected, true));
	phone.page(Page::Contacts);
	assert_eq!(phone.app.page(), Page::Contacts);
}
