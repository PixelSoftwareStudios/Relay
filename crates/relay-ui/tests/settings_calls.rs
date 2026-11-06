//! Settings: the push-to-talk switch, the mode a call starts in, each with the engine's answer
//! under it, and the profile name help text.

mod phone;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome, SettingOutcome, SettingResult};
use relay_ui::i18n::Language;
use relay_ui::model::Mode;

/// An engine that records the settings it is given and answers with `outcome`.
struct Ops {
	sent: Mutex<Vec<(String, String)>>,
	outcome: SettingOutcome,
	reason: &'static str,
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
				outcome: self.outcome,
				reason: self.reason.into(),
			})
			.collect())
	}
}

fn rig(
	category: SettingsCategory,
	outcome: SettingOutcome,
	reason: &'static str,
	tweak: impl FnOnce(&mut relay_ui::model::Settings),
) -> (Phone, Arc<Ops>) {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	let mut state = phone::busy_state();
	tweak(state.settings.as_mut().unwrap());
	phone.show_state(&state);
	let ops = Arc::new(Ops {
		sent: Mutex::new(Vec::new()),
		outcome,
		reason,
	});
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

#[tokio::test]
async fn the_ptt_switch_is_on_by_default_and_shows_what_the_daemon_said() {
	let (mut phone, ops) = rig(
		SettingsCategory::PushToTalk,
		SettingOutcome::AppliedLive,
		"",
		|_| {},
	);
	let drawn = phone.settle();
	assert!(has(&drawn, "Enable push-to-talk"), "{:?}", drawn.labels);
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Push-to-talk is off"))
	);
	phone.click_label("Enable push-to-talk");
	settle_until(&mut phone, "Saved.").await;
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[("ptt.enabled".to_owned(), "false".to_owned())]
	);
}

#[tokio::test]
async fn with_ptt_off_the_hotkey_and_delay_are_greyed_with_an_explanation() {
	let (mut phone, _ops) = rig(
		SettingsCategory::PushToTalk,
		SettingOutcome::AppliedLive,
		"",
		|s| s.ptt_enabled = false,
	);
	let drawn = phone.settle();
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Push-to-talk is off.")),
		"{:?}",
		drawn.labels
	);
	assert!(has(&drawn, "Push-to-talk release delay"));
	// The slider is there but cannot be used: only the switch is tappable besides the chrome.
	let (_, delay) = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Push-to-talk release delay")
		.unwrap();
	assert!(
		!drawn
			.tappable
			.iter()
			.any(|r| r.min.y > delay.max.y && r.min.y < delay.max.y + 90.0),
		"the delay slider is disabled"
	);
}

#[tokio::test]
async fn a_rejected_switch_says_why_and_what_to_do() {
	let (mut phone, _ops) = rig(
		SettingsCategory::PushToTalk,
		SettingOutcome::Rejected,
		"invalid: cannot be changed during a call",
		|_| {},
	);
	phone.click_label("Enable push-to-talk");
	let drawn = settle_until(
		&mut phone,
		"Could not save this: cannot be changed during a call. Try again.",
	)
	.await;
	assert!(!drawn.labels.iter().any(|l| l.contains("invalid:")));
}

#[tokio::test]
async fn the_default_mode_selector_sends_the_key_and_explains_each_choice() {
	let (mut phone, ops) = rig(
		SettingsCategory::Calls,
		SettingOutcome::AppliedLive,
		"",
		|_| {},
	);
	let drawn = phone.settle();
	for line in [
		"Default when a call connects",
		"Your microphone is off until you turn it on.",
		"Your microphone is on as soon as the call connects.",
		"Your microphone is on only while you hold the talk button or key.",
	] {
		assert!(has(&drawn, line), "{line}: {:?}", drawn.labels);
	}
	phone.click_label("Unmuted");
	settle_until(&mut phone, "Saved.").await;
	assert_eq!(
		*ops.sent.lock().unwrap(),
		[(
			"calls.default_transmit_mode".to_owned(),
			"unmuted".to_owned()
		)]
	);
}

#[tokio::test]
async fn with_ptt_off_the_fallback_mode_is_named() {
	let (mut phone, _ops) = rig(
		SettingsCategory::Calls,
		SettingOutcome::AppliedLive,
		"",
		|s| {
			s.ptt_enabled = false;
			s.effective_default_transmit_mode = Mode::Muted;
		},
	);
	let drawn = phone.settle();
	assert!(
		has(
			&drawn,
			"Push-to-talk is off, so calls start in this mode instead: Muted."
		),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn the_display_name_help_is_the_agreed_sentence() {
	let (mut phone, _ops) = rig(
		SettingsCategory::Profile,
		SettingOutcome::AppliedLive,
		"",
		|_| {},
	);
	let drawn = phone.settle();
	assert!(
		has(
			&drawn,
			"Defaults to your device name. Other people see this name when you are discoverable or are called."
		),
		"{:?}",
		drawn.labels
	);
}
