//! Plain words for why the engine refused a ringtone file. The engine answers with a stable
//! code (`RINGTONE_*`); the sentence is the app's, so it is translated.

use crate::controller::ControlError;
use crate::engine::EngineError;
use crate::tr;

/// The translated sentence for a ringtone problem code, if it is one.
pub fn words(code: &str) -> Option<&'static str> {
	Some(match code {
		"RINGTONE_EMPTY" => tr!("ringtone-err-empty"),
		"RINGTONE_TOO_LARGE" => tr!("ringtone-err-too-large"),
		"RINGTONE_TOO_LONG" => tr!("ringtone-err-too-long"),
		"RINGTONE_TOO_SHORT" => tr!("ringtone-err-too-short"),
		"RINGTONE_UNSUPPORTED" => tr!("ringtone-err-unsupported"),
		"RINGTONE_CORRUPT" => tr!("ringtone-err-corrupt"),
		"RINGTONE_SILENT" => tr!("ringtone-err-silent"),
		_ => return None,
	})
}

/// A refused ringtone becomes a friendly message; any other failure keeps the engine's text.
pub fn control_error(error: EngineError) -> ControlError {
	match &error {
		EngineError::Rejected { code, .. } => match words(code) {
			Some(text) => ControlError::Invalid(text),
			None => error.into(),
		},
		_ => error.into(),
	}
}
