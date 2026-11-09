//! The sidebar's update button: it exists only when a release is on offer and the shell can
//! update itself, sits directly above Settings, is disabled with a reason during a call, shows
//! progress in place of its label, a calm failure with Try again, and View release for a copy
//! that cannot install itself.

mod phone;

use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::updates::{UpdateFailure, UpdateOffer, UpdateState};

fn offer(can_install: bool) -> UpdateOffer {
	UpdateOffer {
		version: "0.2.0".into(),
		release_url: "https://github.com/PixelSoftwareStudios/Relay/releases/tag/v0.2.0".into(),
		can_install,
	}
}

/// A desktop rig that can update itself; `calls` keeps or drops the seeded calls.
fn rig(calls: bool, state: UpdateState) -> Phone {
	let phone = Phone::desktop(900.0, 800.0, Language::En);
	phone.shell.self_update.store(true, Ordering::SeqCst);
	let mut shown = phone::busy_state();
	if !calls {
		shown.calls.clear();
	}
	phone.show_state(&shown);
	phone.actions.shared.updates.set(state);
	phone
}

fn rect(drawn: &Drawn, label: &str) -> Option<egui::Rect> {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == label)
		.map(|(_, r)| *r)
}

fn tappable(drawn: &Drawn, label: &str) -> bool {
	rect(drawn, label).is_some_and(|r| drawn.tappable.iter().any(|t| t.contains(r.center())))
}

async fn let_tasks_run() {
	tokio::time::sleep(Duration::from_millis(80)).await;
}

#[tokio::test]
async fn the_button_appears_only_when_a_release_is_available() {
	for state in [
		UpdateState::Idle,
		UpdateState::Checking,
		UpdateState::UpToDate("0.1.0".into()),
	] {
		let mut phone = rig(false, state.clone());
		let drawn = phone.settle();
		assert!(rect(&drawn, "Update to 0.2.0").is_none(), "{state:?}");
		assert!(!drawn.labels.iter().any(|l| l.starts_with("Update to")));
	}
	let mut phone = rig(false, UpdateState::Available(offer(true)));
	assert!(rect(&phone.settle(), "Update to 0.2.0").is_some());
}

#[tokio::test]
async fn a_shell_that_cannot_update_itself_shows_nothing() {
	let mut phone = rig(false, UpdateState::Available(offer(true)));
	phone.shell.self_update.store(false, Ordering::SeqCst);
	let drawn = phone.settle();
	assert!(!drawn.labels.iter().any(|l| l.starts_with("Update to")));
	assert!(!drawn.labels.iter().any(|l| l == "View release"));
}

#[tokio::test]
async fn the_button_sits_directly_above_the_settings_entry() {
	let mut phone = rig(false, UpdateState::Available(offer(true)));
	let drawn = phone.settle();
	let button = rect(&drawn, "Update to 0.2.0").expect("button");
	let settings = rect(&drawn, "Settings").expect("settings entry");
	assert!(
		button.max.y <= settings.min.y,
		"{button:?} above {settings:?}"
	);
	assert!(
		settings.min.y - button.max.y < 40.0,
		"directly above, not far away"
	);
	assert!(
		button.max.x < relay_ui::theme::SIDEBAR_WIDTH,
		"inside the sidebar"
	);
	assert!(tappable(&drawn, "Update to 0.2.0"));
}

#[tokio::test]
async fn pressing_it_installs_and_quits_the_app_once_it_finished() {
	let mut phone = rig(false, UpdateState::Available(offer(true)));
	phone.click_label("Update to 0.2.0");
	let_tasks_run().await;
	assert_eq!(*phone.shell.installs.lock().unwrap(), ["0.2.0"]);
	assert_eq!(
		phone.shell.quits.load(Ordering::SeqCst),
		1,
		"applying quits the app"
	);
}

#[tokio::test]
async fn it_is_disabled_during_a_call_with_a_short_reason() {
	let mut phone = rig(true, UpdateState::Available(offer(true)));
	let drawn = phone.settle();
	assert!(rect(&drawn, "Update to 0.2.0").is_some());
	assert!(!tappable(&drawn, "Update to 0.2.0"), "a call is active");
	assert!(drawn.labels.iter().any(|l| l == "Finish your call first."));
	phone.click_label("Update to 0.2.0");
	let_tasks_run().await;
	assert!(phone.shell.installs.lock().unwrap().is_empty());
	// The action itself refuses too, whatever the button says.
	phone.actions.install_update();
	let_tasks_run().await;
	assert!(phone.shell.installs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn progress_replaces_the_label_and_cannot_be_pressed() {
	let offer = offer(true);
	for (state, text) in [
		(
			UpdateState::Downloading {
				offer: offer.clone(),
				received: 42,
				total: 100,
			},
			"Downloading 42%",
		),
		(UpdateState::Verifying(offer.clone()), "Checking signature…"),
		(UpdateState::Installing(offer), "Installing…"),
	] {
		let mut phone = rig(false, state);
		let drawn = phone.settle();
		assert!(rect(&drawn, text).is_some(), "{text}: {:?}", drawn.labels);
		assert!(!tappable(&drawn, text));
		assert!(rect(&drawn, "Update to 0.2.0").is_none());
	}
}

#[tokio::test]
async fn a_failed_install_is_calm_says_nothing_was_installed_and_can_retry() {
	let state = UpdateState::Failed {
		failure: UpdateFailure::Rejected,
		offer: Some(offer(true)),
	};
	let mut phone = rig(false, state);
	let drawn = phone.settle();
	assert!(
		drawn.labels.iter().any(|l| l.contains("NOT installed")),
		"{:?}",
		drawn.labels
	);
	assert!(tappable(&drawn, "Try again"));
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.to_lowercase().contains("anyway"))
	);
	phone.click_label("Try again");
	let_tasks_run().await;
	assert_eq!(
		*phone.shell.installs.lock().unwrap(),
		["0.2.0"],
		"Try again installs again"
	);
}

#[tokio::test]
async fn a_cancelled_install_returns_to_the_button_and_a_failure_frees_the_daemon() {
	let phone = rig(false, UpdateState::Available(offer(true)));
	*phone.shell.install_reply.lock().unwrap() = Some(Err(UpdateFailure::Cancelled));
	phone.actions.install_update();
	let_tasks_run().await;
	assert_eq!(
		phone.actions.shared.updates.state(),
		UpdateState::Available(offer(true))
	);
	assert_eq!(phone.shell.quits.load(Ordering::SeqCst), 0);
	*phone.shell.install_reply.lock().unwrap() = Some(Err(UpdateFailure::NotWritable));
	phone.actions.install_update();
	let_tasks_run().await;
	assert!(matches!(
		phone.actions.shared.updates.state(),
		UpdateState::Failed {
			failure: UpdateFailure::NotWritable,
			..
		}
	));
	assert!(
		!phone.actions.shared.autostart.paused(),
		"the daemon may start again"
	);
}

#[tokio::test]
async fn a_copy_that_cannot_install_itself_offers_view_release_and_never_installs() {
	let mut phone = rig(true, UpdateState::Available(offer(false)));
	let drawn = phone.settle();
	assert!(rect(&drawn, "Update to 0.2.0").is_none());
	assert!(
		tappable(&drawn, "View release"),
		"opening a page is fine during a call"
	);
	phone.click_label("View release");
	assert_eq!(
		*phone.shell.opened_releases.lock().unwrap(),
		[offer(false).release_url]
	);
	assert!(phone.shell.installs.lock().unwrap().is_empty());
	phone.actions.install_update();
	let_tasks_run().await;
	assert!(phone.shell.installs.lock().unwrap().is_empty());
}
