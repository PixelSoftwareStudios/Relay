//! A daemon older than this app: one clear card at the top of Calls and in
//! Settings, Advanced, with a Restart that is only enabled when no call is
//! active, and the history switch defaulting to on.

mod phone;

use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

const SENTENCE: &str = "The background service is out of date. Restart it to finish updating.";

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

/// A desktop rig whose daemon reports `revision`; `calls` keeps or drops the
/// seeded calls.
fn rig(revision: Option<u32>, calls: bool) -> Phone {
	let phone = Phone::desktop(720.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	let mut state = phone::busy_state();
	if !calls {
		state.calls.clear();
	}
	if let Some(s) = state.settings.as_mut() {
		s.daemon_api_revision = revision.unwrap_or(relay_api::API_REVISION);
		s.history_enabled = false;
	}
	phone.show_state(&state);
	phone
}

/// Whether an enabled, tappable widget covers the painted `label`.
fn tappable(drawn: &Drawn, label: &str) -> bool {
	let (_, at) = drawn
		.named
		.iter()
		.find(|(t, _)| t == label)
		.unwrap_or_else(|| panic!("no {label:?} in {:?}", drawn.labels));
	drawn.tappable.iter().any(|r| r.contains(at.center()))
}

#[tokio::test]
async fn a_current_daemon_shows_no_card() {
	let mut phone = rig(None, false);
	let drawn = phone.page(Page::Calls);
	assert!(!has(&drawn, SENTENCE) && !has(&drawn, "Restart"));
}

#[tokio::test]
async fn an_old_daemon_gets_the_card_on_calls_and_in_advanced_only() {
	let mut phone = rig(Some(0), false);
	for page in [Page::Contacts, Page::Connect, Page::Channels] {
		assert!(!has(&phone.page(page), SENTENCE), "{page:?}");
	}
	let calls = phone.page(Page::Calls);
	assert!(has(&calls, SENTENCE) && has(&calls, "Restart"));
	phone.app.open_settings(Some(SettingsCategory::Advanced));
	let advanced = phone.settle();
	assert!(has(&advanced, SENTENCE) && has(&advanced, "Restart"));
	phone.app.open_settings(Some(SettingsCategory::Audio));
	assert!(!has(&phone.settle(), SENTENCE));
}

#[tokio::test]
async fn restart_is_only_enabled_without_an_active_call_and_stops_then_starts() {
	let mut busy = rig(Some(0), true);
	let drawn = busy.page(Page::Calls);
	assert!(has(&drawn, "Restart"));
	assert!(!tappable(&drawn, "Restart"), "a call is active");
	assert!(has(
		&drawn,
		"End the active call first: restarting would end it."
	));

	let mut idle = rig(Some(0), false);
	let drawn = idle.page(Page::Calls);
	assert!(tappable(&drawn, "Restart"));
	idle.click_label("Restart");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(
		*idle.shell.daemon_log.lock().unwrap(),
		vec!["stop", "start"]
	);
}

#[tokio::test]
async fn a_shell_that_cannot_manage_the_daemon_still_explains_without_a_button() {
	let mut phone = rig(Some(0), false);
	phone.shell.manage_daemon.store(false, Ordering::SeqCst);
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn, SENTENCE) && !has(&drawn, "Restart"));
}

#[tokio::test]
async fn the_history_switch_defaults_to_on_when_an_old_daemon_does_not_report_it() {
	let off = relay_ui::i18n::t("history-off-note");
	let mut old = rig(Some(0), false);
	let shown = old.page(Page::Calls);
	assert!(!shown.labels.iter().any(|l| l == off), "old daemon: on");
	let mut current = rig(None, false);
	let shown = current.page(Page::Calls);
	assert!(shown.labels.iter().any(|l| l == off), "a real off is shown");
}
