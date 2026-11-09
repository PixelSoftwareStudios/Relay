//! The Updates group of Settings, Startup: it works with no daemon, the opt-in is off by default
//! and stored through the shell, the manual check reports its result, and the check at launch
//! fires exactly once when on and never when off. The shell here is a fake updater: nothing
//! touches the network.

mod phone;

use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::model::{AppState, Link};
use relay_ui::updates::{UpdateFailure, UpdateOffer, UpdateState};

const OPT_IN: &str = "Check for updates when Relay starts";

fn offer() -> UpdateOffer {
	UpdateOffer {
		version: "0.2.0".into(),
		release_url: "https://example.invalid/release".into(),
		can_install: true,
	}
}

/// A desktop rig with no daemon at all (offline, no settings, no calls).
fn no_daemon(self_update: bool) -> Phone {
	let mut phone = Phone::desktop(900.0, 800.0, Language::En);
	phone.shell.self_update.store(self_update, Ordering::SeqCst);
	let state = AppState {
		link: Link::Offline,
		..AppState::default()
	};
	phone.show_state(&state);
	phone.app.open_settings(Some(SettingsCategory::Startup));
	phone.settle();
	phone
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

async fn let_tasks_run() {
	tokio::time::sleep(Duration::from_millis(80)).await;
}

#[tokio::test]
async fn the_group_is_there_with_no_daemon_and_says_what_the_check_does() {
	let mut phone = no_daemon(true);
	let drawn = phone.settle();
	assert!(has(&drawn, "Updates"), "{:?}", drawn.labels);
	assert!(has(&drawn, OPT_IN));
	assert!(has(&drawn, "Check for updates"));
	let privacy = drawn
		.labels
		.iter()
		.find(|l| l.contains("github.com"))
		.expect("the privacy sentence");
	assert!(privacy.contains("IP address"), "{privacy}");
}

#[tokio::test]
async fn a_shell_without_self_update_has_no_updates_group() {
	let mut phone = no_daemon(false);
	let drawn = phone.settle();
	assert!(!has(&drawn, "Updates") && !has(&drawn, OPT_IN));
	assert!(!has(&drawn, "Check for updates"));
}

#[tokio::test]
async fn the_opt_in_is_off_by_default_and_the_switch_stores_it_in_the_shell() {
	let mut phone = no_daemon(true);
	assert!(
		!phone.shell.check_on_launch.load(Ordering::SeqCst),
		"off by default"
	);
	phone.click_label(OPT_IN);
	assert!(phone.shell.check_on_launch.load(Ordering::SeqCst));
	phone.click_label(OPT_IN);
	assert!(!phone.shell.check_on_launch.load(Ordering::SeqCst));
	assert_eq!(
		phone.shell.checks.load(Ordering::SeqCst),
		0,
		"a switch never checks"
	);
}

#[tokio::test]
async fn the_check_button_reports_up_to_date_without_a_daemon() {
	let mut phone = no_daemon(true);
	phone.click_label("Check for updates");
	let_tasks_run().await;
	let drawn = phone.settle();
	assert_eq!(phone.shell.checks.load(Ordering::SeqCst), 1);
	let want = format!("Relay {} is up to date.", relay_api::VERSION);
	assert!(has(&drawn, &want), "{:?}", drawn.labels);
}

#[tokio::test]
async fn a_found_release_offers_the_update_in_settings_too() {
	let mut phone = no_daemon(true);
	*phone.shell.check_reply.lock().unwrap() = Some(Ok(Some(offer())));
	phone.click_label("Check for updates");
	let_tasks_run().await;
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Relay 0.2.0 is available."),
		"{:?}",
		drawn.labels
	);
	// The compact layout has no sidebar: the button here is how a narrow window updates.
	assert!(
		drawn
			.labels
			.iter()
			.filter(|l| *l == "Update to 0.2.0")
			.count() >= 1
	);
}

#[tokio::test]
async fn a_failed_check_is_a_calm_line_and_the_button_can_be_pressed_again() {
	let mut phone = no_daemon(true);
	*phone.shell.check_reply.lock().unwrap() = Some(Err(UpdateFailure::Offline));
	phone.click_label("Check for updates");
	let_tasks_run().await;
	let drawn = phone.settle();
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.contains("could not reach GitHub")),
		"{:?}",
		drawn.labels
	);
	*phone.shell.check_reply.lock().unwrap() = Some(Ok(None));
	phone.click_label("Check for updates");
	let_tasks_run().await;
	assert_eq!(phone.shell.checks.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn the_check_at_launch_fires_exactly_once_when_on() {
	let mut phone = no_daemon(true);
	phone.shell.check_on_launch.store(true, Ordering::SeqCst);
	phone.app.set_launch_check_delay(Duration::ZERO);
	for _ in 0..6 {
		phone.settle();
		let_tasks_run().await;
	}
	assert_eq!(phone.shell.checks.load(Ordering::SeqCst), 1);
	// Even when the state is reset, the launch check does not repeat.
	phone.actions.shared.updates.set(UpdateState::Idle);
	phone.settle();
	let_tasks_run().await;
	assert_eq!(phone.shell.checks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_check_at_launch_never_fires_when_off_or_when_the_shell_cannot_update() {
	let mut off = no_daemon(true);
	off.app.set_launch_check_delay(Duration::ZERO);
	for _ in 0..4 {
		off.settle();
		let_tasks_run().await;
	}
	assert_eq!(off.shell.checks.load(Ordering::SeqCst), 0, "opt-in is off");
	let mut android = no_daemon(false);
	android.shell.check_on_launch.store(true, Ordering::SeqCst);
	android.app.set_launch_check_delay(Duration::ZERO);
	for _ in 0..4 {
		android.settle();
		let_tasks_run().await;
	}
	assert_eq!(
		android.shell.checks.load(Ordering::SeqCst),
		0,
		"no self-update capability"
	);
}

#[tokio::test]
async fn the_check_at_launch_waits_for_its_delay() {
	let mut phone = no_daemon(true);
	phone.shell.check_on_launch.store(true, Ordering::SeqCst);
	for _ in 0..4 {
		phone.settle();
		let_tasks_run().await;
	}
	assert_eq!(
		phone.shell.checks.load(Ordering::SeqCst),
		0,
		"not during the first seconds"
	);
}

#[tokio::test]
async fn a_quiet_launch_check_with_nothing_new_shows_nothing() {
	let mut phone = no_daemon(true);
	phone.shell.check_on_launch.store(true, Ordering::SeqCst);
	phone.app.set_launch_check_delay(Duration::ZERO);
	phone.settle();
	let_tasks_run().await;
	assert_eq!(phone.actions.shared.updates.state(), UpdateState::Idle);
}
