//! A daemon from another release than the app: after a self-update the app is new but the
//! background service that was already running is not. The API revision may be the same, yet the
//! out-of-date card appears; matching releases (a development tree) show nothing.

mod phone;

use std::sync::atomic::Ordering;

use phone::Phone;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::AppState;

const SENTENCE: &str = "The background service is out of date. Restart it to finish updating.";

fn state_with(version: &str, api: u32) -> AppState {
	let mut state = phone::busy_state();
	state.calls.clear();
	if let Some(s) = state.settings.as_mut() {
		s.daemon_version = version.into();
		s.daemon_api_revision = api;
	}
	state
}

fn shows_card(state: &AppState) -> bool {
	let mut phone = Phone::desktop(720.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	phone.show_state(state);
	let drawn = phone.page(Page::Calls);
	drawn.labels.iter().any(|l| l == SENTENCE)
}

#[tokio::test]
async fn the_same_release_and_revision_is_current() {
	let state = state_with(relay_api::VERSION, relay_api::API_REVISION);
	assert!(!state.daemon_out_of_date());
	assert!(!shows_card(&state));
}

#[tokio::test]
async fn another_release_with_the_same_api_revision_is_out_of_date() {
	let state = state_with("0.0.1", relay_api::API_REVISION);
	assert!(state.daemon_out_of_date());
	assert!(shows_card(&state));
}

#[tokio::test]
async fn a_daemon_that_reports_no_version_is_out_of_date() {
	let state = state_with("", relay_api::API_REVISION);
	assert!(state.daemon_out_of_date());
	assert!(shows_card(&state));
}

#[tokio::test]
async fn the_restart_waits_for_a_call_to_end() {
	let mut state = phone::busy_state();
	if let Some(s) = state.settings.as_mut() {
		s.daemon_version = "0.0.1".into();
	}
	let mut phone = Phone::desktop(720.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	phone.show_state(&state);
	let drawn = phone.page(Page::Calls);
	let (_, at) = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Restart")
		.expect("restart button");
	assert!(
		!drawn.tappable.iter().any(|r| r.contains(at.center())),
		"restarting would end the call"
	);
	phone.click_label("Restart");
	assert!(
		phone.shell.daemon_log.lock().unwrap().is_empty(),
		"nothing was stopped"
	);
}
