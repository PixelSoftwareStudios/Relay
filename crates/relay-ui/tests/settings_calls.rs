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
