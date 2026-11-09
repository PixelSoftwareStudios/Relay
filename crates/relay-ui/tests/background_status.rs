//! What staying available says about itself: the status line, the one-time question and the
//! chip at the top of Calls on a phone (none in the sidebar).

#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{Drawn, Phone};
use relay_ui::background_settings::BackgroundState;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome, SettingOutcome, SettingResult};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

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

fn set_background(phone: &Phone, on: bool) {
	let mut state = phone::busy_state();
	state.settings.as_mut().unwrap().background_available = on;
	phone.show_state(&state);
}

const RUNNING: &str = "Relay is running in the background and can ring when the device is locked or the app is closed.";
const WAITING: &str =
	"Waiting for Wi-Fi. Relay will become available again when this device joins a network.";
const PAUSED: &str = "Paused. Open Relay or turn this switch off and on to resume.";
const OFF: &str = "Off. Others cannot reach you while the app is closed. Turn the switch on to keep ringing with the window closed.";

#[tokio::test]
async fn the_status_line_says_what_the_shell_reports() {
	let _turn = turn();
	let (mut phone, _ops) = rig(SettingsCategory::Calls, 360.0, 2400.0, Language::En);
	let drawn = phone.settle();
	assert!(has(&drawn, "Stay available"));
	for line in [RUNNING, WAITING, PAUSED, OFF] {
		assert!(!has(&drawn, line), "{line}");
	}
	for (state, line) in [
		(BackgroundState::Running, RUNNING),
		(BackgroundState::WaitingForNetwork, WAITING),
		(BackgroundState::Paused, PAUSED),
	] {
		*phone.shell.background.lock().unwrap() = Some(state);
		let drawn = phone.settle();
		assert!(has(&drawn, line), "{state:?}: {:?}", drawn.labels);
		assert!(
			y_of(&drawn, "Stay available") < y_of(&drawn, line)
				&& y_of(&drawn, line) < y_of(&drawn, "Who can ring me")
		);
	}
	set_background(&phone, false);
	let drawn = phone.settle();
	assert!(
		has(&drawn, OFF) && !has(&drawn, PAUSED),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn a_desktop_with_a_tray_says_so_and_one_without_says_nothing() {
	let _turn = turn();
	let (mut phone, _ops) = rig(SettingsCategory::Calls, 720.0, 2400.0, Language::En);
	let tray = "Relay keeps running in the tray when you close the window, and can ring.";
	phone.shell.desktop.store(true, Ordering::SeqCst);
	assert!(has(&phone.settle(), tray));
	phone.shell.desktop.store(false, Ordering::SeqCst);
	assert!(!has(&phone.settle(), tray));
}

fn ask(phone: &Phone) {
	phone
		.shell
		.remembers_questions
		.store(true, Ordering::SeqCst);
	*phone.shell.notifications.lock().unwrap() = Some(false);
	*phone.shell.battery.lock().unwrap() = Some(false);
}

const TITLE: &str = "Stay available in the background?";

#[tokio::test]
async fn the_first_run_question_is_asked_once_and_yes_enables_everything() {
	let _turn = turn();
	let (mut phone, ops) = rig(SettingsCategory::Calls, 360.0, 800.0, Language::En);
	phone.app.set_page(Page::Calls);
	ask(&phone);
	let drawn = phone.settle();
	assert!(has(&drawn, TITLE), "{:?}", drawn.labels);
	assert!(has(&drawn, "Yes, stay available") && has(&drawn, "Only while the app is open"));
	phone.click_label("Yes, stay available");
	let drawn = phone.settle();
	assert!(!has(&drawn, TITLE));
	for _ in 0..100 {
		if !ops.sent.lock().unwrap().is_empty() {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("background.available".to_owned(), "true".to_owned())]
	);
	assert_eq!(phone.shell.notification_requests.load(Ordering::SeqCst), 1);
	assert_eq!(phone.shell.battery_requests.load(Ordering::SeqCst), 1);
	assert_eq!(phone.shell.answered.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn only_while_open_turns_it_off_and_asks_for_nothing() {
	let _turn = turn();
	let (mut phone, ops) = rig(SettingsCategory::Calls, 360.0, 800.0, Language::En);
	phone.app.set_page(Page::Calls);
	ask(&phone);
	assert!(has(&phone.settle(), TITLE));
	phone.click_label("Only while the app is open");
	assert!(!has(&phone.settle(), TITLE));
	for _ in 0..100 {
		if !ops.sent.lock().unwrap().is_empty() {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("background.available".to_owned(), "false".to_owned())]
	);
	assert_eq!(phone.shell.notification_requests.load(Ordering::SeqCst), 0);
	assert_eq!(phone.shell.battery_requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_question_stays_away_when_answered_or_not_relevant() {
	let _turn = turn();
	let (mut phone, _ops) = rig(SettingsCategory::Calls, 360.0, 800.0, Language::En);
	phone.app.set_page(Page::Calls);
	// A shell that does not remember answers never asks.
	*phone.shell.notifications.lock().unwrap() = Some(true);
	assert!(!has(&phone.settle(), TITLE));
	// Already answered.
	phone
		.shell
		.remembers_questions
		.store(true, Ordering::SeqCst);
	phone
		.shell
		.answered
		.lock()
		.unwrap()
		.push("background-first-run".to_owned());
	assert!(!has(&phone.settle(), TITLE));
	// A desktop: notifications do not apply.
	let (mut desktop, _ops) = rig(SettingsCategory::Calls, 720.0, 800.0, Language::En);
	desktop.shell.desktop.store(true, Ordering::SeqCst);
	desktop
		.shell
		.remembers_questions
		.store(true, Ordering::SeqCst);
	desktop.app.set_page(Page::Calls);
	assert!(!has(&desktop.settle(), TITLE));
}

#[tokio::test]
async fn the_chip_names_the_setting_and_opens_settings_calls() {
	let _turn = turn();
	{
		let (width, desktop) = (360.0, false);
		let (mut phone, _ops) = rig(SettingsCategory::Calls, width, 800.0, Language::En);
		phone.shell.desktop.store(desktop, Ordering::SeqCst);
		phone.app.close_settings();
		phone.app.open_settings(Some(SettingsCategory::Profile));
		phone.app.set_page(Page::Calls);
		let drawn = phone.settle();
		assert!(has(&drawn, "Available in background"), "{:?}", drawn.labels);
		set_background(&phone, false);
		assert!(has(&phone.settle(), "Only while app is open"));
		phone.click_label("Only while app is open");
		phone.settle();
		assert_eq!(phone.app.page(), Page::Settings);
		assert_eq!(phone.app.settings_category(), Some(SettingsCategory::Calls));
	}
}

#[tokio::test]
async fn the_sidebar_identity_block_has_no_background_chip() {
	let _turn = turn();
	let (mut desktop, _ops) = rig(SettingsCategory::Calls, 720.0, 800.0, Language::En);
	desktop.shell.desktop.store(true, Ordering::SeqCst);
	desktop.app.close_settings();
	desktop.app.set_page(Page::Calls);
	for on in [true, false] {
		set_background(&desktop, on);
		let drawn = desktop.settle();
		assert!(
			!has(&drawn, "Available in background"),
			"{:?}",
			drawn.labels
		);
		assert!(!has(&drawn, "Only while app is open"), "{:?}", drawn.labels);
	}
}

#[tokio::test]
async fn the_new_pieces_fit_in_every_language_at_every_size() {
	let _turn = turn();
	let mut bad = Vec::new();
	for language in phone::LANGUAGES {
		for (desktop, w, h) in [
			(false, 360.0, 800.0),
			(false, 320.0, 640.0),
			(true, 720.0, 800.0),
		] {
			for state in [
				Some(BackgroundState::Running),
				Some(BackgroundState::WaitingForNetwork),
				Some(BackgroundState::Paused),
				None,
			] {
				for page in [Page::Calls, Page::Settings] {
					let (mut phone, _ops) = rig(SettingsCategory::Calls, w, h, language);
					phone.shell.desktop.store(desktop, Ordering::SeqCst);
					ask(&phone);
					*phone.shell.background.lock().unwrap() = state;
					phone.app.set_page(page);
					let drawn = phone.settle();
					let screen = phone.screen().expand(1.0);
					for r in drawn.widgets.iter().chain(&drawn.painted) {
						if r.min.x < screen.min.x || r.max.x > screen.max.x {
							bad.push(format!("{language:?} {w}x{h} {page:?}: overflow {r:?}"));
						}
					}
					if !desktop {
						for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
							if r.height() < 43.5 || r.width() < 43.5 {
								bad.push(format!(
									"{language:?} {w}x{h} {page:?}: small {:?}",
									r.size()
								));
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
