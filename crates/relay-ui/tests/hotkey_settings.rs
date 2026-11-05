//! The hotkey settings panel logic against a fake daemon: reads through
//! `GetSettings`, writes through `SetSettings`, shows the per-key outcome.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_ui::controller::{ControlError, Controller, SessionSlot};
use relay_ui::engine::{Command, EngineError, Operations, Outcome, SettingOutcome, SettingResult};
use relay_ui::hotkey_settings::{
	HotkeyPanel, HotkeyValues, KEY_ACCELERATOR, KEY_RELEASE_DELAY, outcome_text,
};
use relay_ui::hub::Hub;

/// A daemon that stores two keys and answers like the real one: the release
/// delay is live, the hotkey needs a restart, anything else is refused.
#[derive(Default)]
struct FakeDaemon {
	store: Mutex<BTreeMap<String, String>>,
	sets: Mutex<Vec<Vec<(String, String)>>>,
	fail_get: Mutex<Option<String>>,
}

impl FakeDaemon {
	fn new() -> Arc<Self> {
		let fake = Self::default();
		*fake.store.lock().unwrap() = BTreeMap::from([
			(KEY_ACCELERATOR.to_owned(), "Ctrl+Alt+Space".to_owned()),
			(KEY_RELEASE_DELAY.to_owned(), "150".to_owned()),
		]);
		Arc::new(fake)
	}
}

#[async_trait]
impl Operations for FakeDaemon {
	async fn run(&self, _command: Command) -> Result<Outcome, EngineError> {
		Ok(Outcome::default())
	}

	async fn get_settings(&self) -> Result<BTreeMap<String, String>, EngineError> {
		if let Some(message) = self.fail_get.lock().unwrap().clone() {
			return Err(EngineError::Unavailable(message));
		}
		Ok(self.store.lock().unwrap().clone())
	}

	async fn set_settings(
		&self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, EngineError> {
		self.sets.lock().unwrap().push(values.clone());
		let mut out = Vec::new();
		for (key, value) in values {
			let (outcome, reason) = match key.as_str() {
				KEY_ACCELERATOR if value == "Nonsense" => {
					(SettingOutcome::Rejected, "not a key combination")
				}
				KEY_ACCELERATOR => (SettingOutcome::SavedNeedsRestart, "read at start"),
				KEY_RELEASE_DELAY => (SettingOutcome::AppliedLive, ""),
				_ => (SettingOutcome::Rejected, "unknown key"),
			};
			if outcome != SettingOutcome::Rejected {
				self.store.lock().unwrap().insert(key.clone(), value);
			}
			out.push(SettingResult {
				key,
				outcome,
				reason: reason.into(),
			});
		}
		Ok(out)
	}
}

fn controller(ops: Arc<FakeDaemon>) -> Controller {
	let slot = Arc::new(SessionSlot::default());
	slot.set(ops);
	Controller::new(Arc::new(Hub::default()), slot)
}

fn values(accelerator: &str, delay: u32) -> HotkeyValues {
	HotkeyValues {
		accelerator: accelerator.into(),
		release_delay_ms: delay,
	}
}

#[tokio::test]
async fn reads_both_keys_from_get_settings() {
	let ctl = controller(FakeDaemon::new());
	assert_eq!(
		ctl.hotkey_settings().await.unwrap(),
		values("Ctrl+Alt+Space", 150)
	);
}

#[tokio::test]
async fn a_missing_or_malformed_key_is_an_error_not_a_default() {
	let mut dump = BTreeMap::new();
	assert!(HotkeyValues::from_dump(&dump).is_err());
	dump.insert(KEY_ACCELERATOR.to_owned(), "F9".to_owned());
	dump.insert(KEY_RELEASE_DELAY.to_owned(), "soon".to_owned());
	let error = HotkeyValues::from_dump(&dump).unwrap_err();
	assert!(error.contains("ptt.release_delay_ms"), "{error}");
}

#[tokio::test]
async fn an_unreachable_daemon_surfaces_on_the_panel() {
	let daemon = FakeDaemon::new();
	*daemon.fail_get.lock().unwrap() = Some("socket missing".into());
	let error = controller(daemon).hotkey_settings().await.unwrap_err();
	assert!(error.to_string().contains("socket missing"));
	let mut panel = HotkeyPanel::default();
	panel.loaded(Err(error.to_string()));
	assert!(matches!(panel.stored, Some(Err(_))));
	assert!(!panel.dirty());
}

#[tokio::test]
async fn only_changed_keys_are_sent_and_outcomes_come_back_per_key() {
	let daemon = FakeDaemon::new();
	let ctl = controller(daemon.clone());
	let stored = ctl.hotkey_settings().await.unwrap();
	let results = ctl
		.save_hotkey_settings(&stored, &values("Ctrl+Shift+T", 150))
		.await
		.unwrap();
	assert_eq!(
		daemon.sets.lock().unwrap().as_slice(),
		[vec![(
			KEY_ACCELERATOR.to_owned(),
			"Ctrl+Shift+T".to_owned()
		)]]
	);
	assert_eq!(results.len(), 1);
	assert_eq!(results[0].outcome, SettingOutcome::SavedNeedsRestart);

	let results = ctl
		.save_hotkey_settings(&values("Ctrl+Shift+T", 150), &values("Ctrl+Shift+T", 300))
		.await
		.unwrap();
	assert_eq!(results[0].outcome, SettingOutcome::AppliedLive);
	assert_eq!(
		ctl.hotkey_settings().await.unwrap(),
		values("Ctrl+Shift+T", 300)
	);
}

#[tokio::test]
async fn a_rejected_key_keeps_the_stored_value_and_shows_the_reason() {
	let daemon = FakeDaemon::new();
	let ctl = controller(daemon);
	let stored = ctl.hotkey_settings().await.unwrap();
	let draft = values("Nonsense", 400);
	let results = ctl.save_hotkey_settings(&stored, &draft).await.unwrap();
	let by_key: BTreeMap<_, _> = results.iter().map(|r| (r.key.as_str(), r)).collect();
	assert_eq!(by_key[KEY_ACCELERATOR].outcome, SettingOutcome::Rejected);
	assert_eq!(
		by_key[KEY_RELEASE_DELAY].outcome,
		SettingOutcome::AppliedLive
	);
	assert_eq!(
		outcome_text(by_key[KEY_ACCELERATOR]),
		"Push-to-talk hotkey: rejected (not a key combination)"
	);

	let mut panel = HotkeyPanel::default();
	panel.loaded(Ok(stored));
	panel.draft = draft;
	panel.saving = true;
	panel.saved(Ok(results));
	panel.loaded(ctl.hotkey_settings().await.map_err(|e| e.to_string()));
	// The draft is not overwritten by the reread, and still differs (hotkey).
	assert_eq!(panel.draft.accelerator, "Nonsense");
	assert!(panel.dirty());
	assert!(!panel.saving);
	panel.revert();
	assert_eq!(panel.draft, values("Ctrl+Alt+Space", 400));
	assert!(!panel.dirty());
	assert!(panel.report.is_none());
}

#[tokio::test]
async fn nothing_changed_sends_nothing() {
	let daemon = FakeDaemon::new();
	let ctl = controller(daemon.clone());
	let same = values("Ctrl+Alt+Space", 150);
	assert!(
		ctl.save_hotkey_settings(&same, &same)
			.await
			.unwrap()
			.is_empty()
	);
	assert!(daemon.sets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn values_that_cannot_be_valid_never_leave_the_window() {
	let daemon = FakeDaemon::new();
	let ctl = controller(daemon.clone());
	let stored = values("Ctrl+Alt+Space", 150);
	let too_slow = ctl
		.save_hotkey_settings(&stored, &values("F9", 2001))
		.await
		.unwrap_err();
	assert_eq!(
		too_slow,
		ControlError::Invalid("release delay must be between 0 and 2000 ms")
	);
	let long = "x".repeat(65);
	assert!(
		ctl.save_hotkey_settings(&stored, &values(&long, 150))
			.await
			.is_err()
	);
	assert!(
		ctl.save_hotkey_settings(&stored, &values("a\nb", 150))
			.await
			.is_err()
	);
	assert!(daemon.sets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_a_session_the_panel_cannot_talk() {
	let ctl = Controller::new(Arc::new(Hub::default()), Arc::new(SessionSlot::default()));
	assert_eq!(
		ctl.hotkey_settings().await.unwrap_err(),
		ControlError::NotConnected
	);
}

#[test]
fn outcome_texts_distinguish_the_three_cases() {
	let result = |outcome, reason: &str| SettingResult {
		key: KEY_RELEASE_DELAY.into(),
		outcome,
		reason: reason.into(),
	};
	assert_eq!(
		outcome_text(&result(SettingOutcome::AppliedLive, "")),
		"Release delay: applied now"
	);
	assert_eq!(
		outcome_text(&result(SettingOutcome::SavedNeedsRestart, "")),
		"Release delay: saved, takes effect after a restart"
	);
	assert_eq!(
		outcome_text(&result(SettingOutcome::Rejected, "too long")),
		"Release delay: rejected (too long)"
	);
}

#[test]
fn closing_the_panel_forgets_everything() {
	let mut panel = HotkeyPanel {
		requested: true,
		..HotkeyPanel::default()
	};
	panel.loaded(Ok(values("F9", 0)));
	panel.reset();
	assert!(!panel.requested && panel.stored.is_none() && !panel.seeded);
}
