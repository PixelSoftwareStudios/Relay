//! A daemon older than this app may not know a settings key. Its answers
//! then say "unknown setting"; this turns them into one sentence a person can
//! act on: restart the background service.

use crate::tr;

/// Whether a daemon message means it does not know the settings key.
pub fn is_unknown_key(message: &str) -> bool {
	let lower = message.to_lowercase();
	lower.contains("unknown setting") || lower.contains("unknown key")
}

/// `message`, or the not-supported sentence when it is about an unknown key.
pub fn friendly(message: String) -> String {
	if is_unknown_key(&message) {
		tr!("setting-unsupported").to_owned()
	} else {
		message
	}
}

/// The reason of a rejected key, or the not-supported sentence for an unknown key.
pub fn reason(reason: &str) -> &str {
	if is_unknown_key(reason) {
		tr!("setting-unsupported")
	} else {
		reason.trim()
	}
}
