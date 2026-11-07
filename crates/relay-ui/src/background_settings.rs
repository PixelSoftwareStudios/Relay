//! Keys of the background, call-alert and auto-answer preferences, and how the mock engine
//! applies them.

use crate::model::{CallAlert, Settings};

/// Stay reachable with no window open.
pub const KEY_BACKGROUND: &str = "background.available";
/// How an incoming call alerts.
pub const KEY_CALL_ALERT: &str = "calls.alert";
/// Global auto-answer switch.
pub const KEY_AUTO_ANSWER: &str = "calls.auto_answer_enabled";

/// Applies one of the three keys to `settings`. `None` when the key is not one of them,
/// `Some(Err(why))` when the value is refused.
pub fn apply(settings: &mut Settings, key: &str, value: &str) -> Option<Result<(), &'static str>> {
	let flag = |value: &str| value.parse::<bool>().map_err(|_| "must be true or false");
	Some(match key {
		KEY_BACKGROUND => flag(value).map(|on| settings.background_available = on),
		KEY_AUTO_ANSWER => flag(value).map(|on| settings.auto_answer_enabled = on),
		KEY_CALL_ALERT => CallAlert::ALL
			.into_iter()
			.find(|a| a.key() == value)
			.map(|a| settings.call_alert = a)
			.ok_or("must be system, ring_and_vibrate, vibrate or silent"),
		_ => return None,
	})
}
