//! "Reset Relay data…": reachable with no daemon (Settings > Advanced and the strip shown when
//! the daemon is not running), confirmed before anything happens, and cancellable.

mod phone;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_client::reset::ResetError;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::daemon_control::reset_message;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::Link;

const ASK: &str = "Reset Relay data…";
const CONFIRM: &str = "Delete data and restart";

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn mentions(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

fn log(phone: &Phone) -> Vec<&'static str> {
	phone.shell.daemon_log.lock().unwrap().clone()
}

/// A desktop window whose daemon is not running.
fn offline() -> Phone {
	let phone = Phone::desktop(900.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	let mut state = phone::busy_state();
	state.calls.clear();
	state.mock = false;
	state.link = Link::Offline;
	state.settings = None;
	state.engine = None;
	phone.show_state(&state);
	phone
}

fn advanced(phone: &mut Phone) -> Drawn {
	phone.app.open_settings(Some(SettingsCategory::Advanced));
	phone.settle()
}

#[tokio::test]
async fn advanced_offers_the_reset_with_no_daemon() {
	let mut phone = offline();
	let drawn = advanced(&mut phone);
	assert!(has(&drawn, "Reset Relay data"), "{:?}", drawn.labels);
	assert!(has(&drawn, ASK));
	assert!(log(&phone).is_empty());
}

#[tokio::test]
async fn nothing_happens_until_the_confirmation_is_accepted() {
	let mut phone = offline();
	advanced(&mut phone);
	phone.click_label(ASK);
	let drawn = phone.settle();
	// Plain words: what goes, what stays.
	assert!(mentions(&drawn, "contacts") && mentions(&drawn, "profile picture"));
	assert!(mentions(&drawn, "history") && mentions(&drawn, "settings"));
	assert!(mentions(&drawn, "device identity is kept"));
	assert!(has(&drawn, CONFIRM) && has(&drawn, "Cancel"));
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert!(log(&phone).is_empty(), "asking is not doing");

	phone.click_label(CONFIRM);
	tokio::time::sleep(Duration::from_millis(100)).await;
	// The data goes first, then the daemon is started again.
	assert_eq!(log(&phone), ["reset", "start"]);
}

#[tokio::test]
async fn cancel_does_nothing_and_goes_back_to_the_button() {
	let mut phone = offline();
	advanced(&mut phone);
	phone.click_label(ASK);
	phone.click_label("Cancel");
	let drawn = phone.settle();
	assert!(has(&drawn, ASK) && !has(&drawn, CONFIRM));
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert!(log(&phone).is_empty());
}

#[tokio::test]
async fn the_daemon_not_running_strip_has_the_reset_too() {
	let mut phone = offline();
	phone
		.actions
		.shared
		.set_error("The Relay daemon did not start: exit 1.".to_owned());
	let drawn = phone.page(Page::Calls);
	assert!(mentions(&drawn, "did not start"), "{:?}", drawn.labels);
	assert!(has(&drawn, ASK), "{:?}", drawn.labels);

	phone.click_label(ASK);
	assert!(has(&phone.settle(), CONFIRM));
	assert!(log(&phone).is_empty());
	phone.click_label(CONFIRM);
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert_eq!(log(&phone), ["reset", "start"]);
}

#[tokio::test]
async fn a_connected_daemon_shows_no_reset_on_the_strip() {
	let mut phone = Phone::desktop(900.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	let drawn = phone.page(Page::Calls);
	assert!(!has(&drawn, ASK));
}

#[tokio::test]
async fn a_shell_that_cannot_reset_says_where_to_clear_the_storage() {
	let mut phone = offline();
	phone.shell.manage_daemon.store(false, Ordering::SeqCst);
	let drawn = advanced(&mut phone);
	assert!(!has(&drawn, ASK));
	assert!(mentions(
		&drawn,
		"clear Relay's storage in the system settings"
	));
}

#[test]
fn failures_are_worded_for_the_person() {
	assert!(reset_message(Ok(vec![PathBuf::from("relay.db")])).is_ok());
	let running = reset_message(Err(ResetError::DaemonRunning)).unwrap_err();
	assert!(running.contains("Quit the daemon first"), "{running}");
	let partial = reset_message(Err(ResetError::Incomplete {
		removed: Vec::new(),
		failed: vec![(PathBuf::from("/x/relay.db"), "busy".into())],
	}))
	.unwrap_err();
	assert!(partial.contains("relay.db (busy)"), "{partial}");
	assert!(
		reset_message(Err(ResetError::Unavailable("no dir".into())))
			.unwrap_err()
			.contains("no dir")
	);
}
