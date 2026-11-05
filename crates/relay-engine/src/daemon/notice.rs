//! Recovery notices as warnings clients can show.

use relay_api::pb::Problem;
use relay_config::Notice;

pub(super) fn problem(notice: &Notice) -> Problem {
	let (code, message) = match notice {
		Notice::PreferencesRecoveredFromBackup => (
			"PREFERENCES_RECOVERED",
			"Preferences were damaged; the backup was used and the microphone selection cleared."
				.to_owned(),
		),
		Notice::PreferencesReset { .. } => (
			"PREFERENCES_RESET",
			"Preferences were unreadable and were reset; no microphone is selected.".to_owned(),
		),
		Notice::TrustRecoveredFromBackup => (
			"TRUST_RECOVERED",
			"The trust store was damaged; recent entries may need re-verification.".to_owned(),
		),
		Notice::TrustStoreLost { .. } => (
			"TRUST_LOST",
			"The trust store was lost; every peer must be verified again.".to_owned(),
		),
	};
	Problem {
		code: code.to_owned(),
		message,
		retryable: false,
		recovery_action: "review".to_owned(),
	}
}
