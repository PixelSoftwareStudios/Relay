//! An older daemon does not know every settings key. The panels then say to
//! restart the background service instead of repeating "unknown setting".

use relay_ui::engine::{SettingOutcome, SettingResult};
use relay_ui::hotkey_settings::{self, HotkeyPanel};
use relay_ui::limits_settings::{self, LimitsPanel};
use relay_ui::setting_support::{friendly, is_unknown_key};

const SENTENCE: &str = "Not supported by the running background service: restart it.";

fn rejected(key: &str, reason: &str) -> SettingResult {
	SettingResult {
		key: key.into(),
		outcome: SettingOutcome::Rejected,
		reason: reason.into(),
	}
}

#[test]
fn unknown_key_messages_are_recognised() {
	assert!(is_unknown_key("unknown setting `history.enabled`"));
	assert!(is_unknown_key("Unknown key"));
	assert!(!is_unknown_key("value out of range"));
	assert_eq!(friendly("unknown setting `x`".into()), SENTENCE);
	assert_eq!(friendly("socket closed".into()), "socket closed");
}

#[test]
fn rejected_keys_of_an_old_daemon_read_as_restart_it() {
	let hotkey = hotkey_settings::outcome_text(&rejected(
		"hotkeys.push_to_talk",
		"unknown setting `hotkeys.push_to_talk`",
	));
	assert!(
		hotkey.contains(SENTENCE) && !hotkey.contains("unknown"),
		"{hotkey}"
	);
	let limits = limits_settings::outcome_text(&rejected(
		"limits.avatar_cache_mib",
		"unknown setting `limits.avatar_cache_mib`",
	));
	assert!(limits.contains(SENTENCE), "{limits}");
	// Other reasons are kept as the daemon gave them.
	let other = limits_settings::outcome_text(&rejected("limits.avatar_cache_mib", "too big"));
	assert!(other.contains("too big"), "{other}");
}

#[test]
fn a_failed_save_names_the_restart_not_the_key() {
	let mut hotkey = HotkeyPanel::default();
	hotkey.saved(Err("unknown setting `hotkeys.push_to_talk`".into()));
	assert_eq!(hotkey.report, Some(Err(SENTENCE.to_owned())));
	let mut limits = LimitsPanel::default();
	limits.saved(Err("unknown setting `limits.x`".into()));
	assert_eq!(limits.report, Some(Err(SENTENCE.to_owned())));
}
