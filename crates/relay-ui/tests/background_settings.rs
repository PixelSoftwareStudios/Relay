//! Settings for staying available in the background, the call alert and the global
//! auto-answer switch: the keys they send and the warnings the shell raises.

#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome, SettingOutcome, SettingResult};
use relay_ui::i18n::Language;

static TURN: Mutex<()> = Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Ops {
	sent: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, _command: Command) -> Result<Outcome, EngineError> {
		Ok(Outcome::default())
	}

	async fn set_settings(
		&self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, EngineError> {
		self.sent.lock().unwrap().extend(values.clone());
		Ok(values
			.into_iter()
			.map(|(key, _)| SettingResult {
				key,
				outcome: SettingOutcome::AppliedLive,
				reason: String::new(),
			})
			.collect())
	}
}

fn rig(
	category: SettingsCategory,
	width: f32,
	height: f32,
	language: Language,
) -> (Phone, Arc<Ops>) {
	let mut phone = Phone::new(width, height, language);
	phone.show_state(&phone::busy_state());
	let ops = Arc::new(Ops::default());
	phone.slot.set(ops.clone());
	phone.app.open_settings(Some(category));
	phone.settle();
	(phone, ops)
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

async fn settle_until(phone: &mut Phone, text: &str) -> Drawn {
	for _ in 0..200 {
		let drawn = phone.settle();
		if has(&drawn, text) {
			return drawn;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("{text:?} never appeared: {:?}", phone.settle().labels);
}

fn y_of(drawn: &Drawn, text: &str) -> f32 {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
		.1
		.center()
		.y
}

#[tokio::test]
async fn the_background_switch_is_first_explained_and_sends_its_key() {
	let _turn = turn();
	let (mut phone, ops) = rig(SettingsCategory::Calls, 360.0, 2400.0, Language::En);
	let drawn = phone.settle();
	assert!(has(&drawn, "Stay available in the background"));
	assert!(has(
		&drawn,
		"Keeps Relay reachable and ringing while the window or app is closed, as long as this device is on the network."
	));
	assert!(
		y_of(&drawn, "Stay available in the background") < y_of(&drawn, "Who can ring me"),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Stay available in the background");
	settle_until(&mut phone, "Saved.").await;
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("background.available".to_owned(), "false".to_owned())]
	);
}

#[tokio::test]
async fn the_call_alert_sits_by_the_ringtone_and_sends_its_key() {
	let _turn = turn();
	let (mut phone, ops) = rig(SettingsCategory::Calls, 360.0, 2400.0, Language::En);
	phone.shell.vibrates.store(true, Ordering::SeqCst);
	let drawn = phone.settle();
	assert!(y_of(&drawn, "Call alert") > y_of(&drawn, "Ringtone"));
	phone.click_label("Follow device settings");
	let open = phone.settle();
	for option in ["Ring and vibrate", "Vibrate only", "Silent"] {
		assert!(has(&open, option), "{option}: {:?}", open.labels);
	}
	phone.click_label("Vibrate only");
	settle_until(&mut phone, "Saved.").await;
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("calls.alert".to_owned(), "vibrate".to_owned())]
	);
}

#[tokio::test]
async fn a_computer_offers_no_vibrate_choice() {
	let _turn = turn();
	let (mut phone, _ops) = rig(SettingsCategory::Calls, 720.0, 2400.0, Language::En);
	phone.click_label("Follow system settings");
	let open = phone.settle();
	assert!(
		has(&open, "Ring") && has(&open, "Silent"),
		"{:?}",
		open.labels
	);
	assert!(
		!open.labels.iter().any(|l| l.contains("ibrate")),
		"{:?}",
		open.labels
	);
}

#[tokio::test]
async fn the_privacy_page_has_the_automatic_answering_switch() {
	let _turn = turn();
	let (mut phone, ops) = rig(SettingsCategory::Privacy, 360.0, 3000.0, Language::En);
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Allow automatic answering"),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Allow automatic answering");
	settle_until(&mut phone, "Saved.").await;
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("calls.auto_answer_enabled".to_owned(), "false".to_owned())]
	);
}

#[tokio::test]
async fn shell_warnings_show_only_when_the_shell_says_no_and_their_buttons_call_it() {
	let _turn = turn();
	let (mut phone, _ops) = rig(SettingsCategory::Calls, 360.0, 2400.0, Language::En);
	let quiet = phone.settle();
	assert!(
		!quiet
			.labels
			.iter()
			.any(|l| l.starts_with("Battery optimization"))
	);
	assert!(!has(&quiet, "Allow notifications"));
	*phone.shell.battery.lock().unwrap() = Some(true);
	*phone.shell.notifications.lock().unwrap() = Some(true);
	assert!(!has(&phone.settle(), "Allow in battery settings"));

	*phone.shell.battery.lock().unwrap() = Some(false);
	*phone.shell.notifications.lock().unwrap() = Some(false);
	let warned = phone.settle();
	for line in [
		"Battery optimization may stop Relay from ringing while the app is closed.",
		"Notifications are off, so you would not see incoming calls while the app is closed.",
	] {
		assert!(has(&warned, line), "{line}: {:?}", warned.labels);
	}
	phone.click_label("Allow in battery settings");
	phone.click_label("Allow notifications");
	assert_eq!(phone.shell.battery_requests.load(Ordering::SeqCst), 1);
	assert_eq!(phone.shell.notification_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_pages_fit_in_every_language_at_every_size() {
	let _turn = turn();
	let mut bad = Vec::new();
	for language in phone::LANGUAGES {
		for (desktop, w, h) in [
			(false, 360.0, 800.0),
			(false, 320.0, 640.0),
			(true, 720.0, 800.0),
		] {
			for category in [SettingsCategory::Calls, SettingsCategory::Privacy] {
				let (mut phone, _ops) = rig(category, w, h, language);
				if desktop {
					phone.shell.desktop.store(true, Ordering::SeqCst);
				} else {
					phone.shell.vibrates.store(true, Ordering::SeqCst);
				}
				*phone.shell.battery.lock().unwrap() = Some(false);
				*phone.shell.notifications.lock().unwrap() = Some(false);
				let drawn = phone.settle();
				let screen = phone.screen().expand(1.0);
				for r in drawn.widgets.iter().chain(&drawn.painted) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{language:?} {w}x{h} {category:?}: overflow {r:?}"));
					}
				}
				if !desktop {
					for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
						if r.height() < 43.5 || r.width() < 43.5 {
							bad.push(format!("{language:?} {w}x{h}: small {:?}", r.size()));
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
