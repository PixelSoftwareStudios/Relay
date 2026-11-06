//! Settings, Push-to-talk: what became of the system-wide key the shell
//! registers is told in plain words, with the way out.

mod phone;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome};
use relay_ui::hotkey_settings::HotkeyState;
use relay_ui::i18n::Language;

struct Daemon;

#[async_trait]
impl Operations for Daemon {
	async fn run(&self, _command: Command) -> Result<Outcome, EngineError> {
		Ok(Outcome::default())
	}

	async fn get_settings(&self) -> Result<BTreeMap<String, String>, EngineError> {
		Ok(BTreeMap::from([
			(
				"hotkeys.push_to_talk".to_owned(),
				"Ctrl+Alt+Space".to_owned(),
			),
			("ptt.release_delay_ms".to_owned(), "0".to_owned()),
		]))
	}
}

async fn labels(state: Option<HotkeyState>) -> Vec<String> {
	let mut phone = Phone::desktop(720.0, 2400.0, Language::En);
	*phone.shell.hotkey.lock().unwrap() = state;
	phone.slot.set(Arc::new(Daemon));
	phone.app.open_settings(Some(SettingsCategory::PushToTalk));
	for _ in 0..50 {
		let drawn = phone.settle();
		if drawn.labels.iter().any(|l| l == "Hotkey") {
			return drawn.labels;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	phone.settle().labels
}

fn says(labels: &[String], start: &str) -> bool {
	labels.iter().any(|l| l.starts_with(start))
}

#[tokio::test]
async fn a_refused_key_says_why_and_keeps_the_button_in_view() {
	let labels = labels(Some(HotkeyState::Failed(
		"another program already uses it".into(),
	)))
	.await;
	assert!(
		labels.iter().any(|l| l
			== "The shortcut could not be registered: another program already uses it. You can still hold the on-screen button."),
		"{labels:?}"
	);
}

#[tokio::test]
async fn a_working_key_is_named() {
	let labels = labels(Some(HotkeyState::Registered("Ctrl+Alt+Space".into()))).await;
	assert!(
		says(&labels, "The hotkey Ctrl+Alt+Space is ready"),
		"{labels:?}"
	);
	assert!(!says(&labels, "The shortcut could not"), "{labels:?}");
}

#[tokio::test]
async fn a_shell_without_a_key_shows_no_panel() {
	let labels = labels(None).await;
	assert!(!says(&labels, "The hotkey"), "{labels:?}");
	assert!(!labels.iter().any(|l| l == "Hotkey"), "{labels:?}");
}
