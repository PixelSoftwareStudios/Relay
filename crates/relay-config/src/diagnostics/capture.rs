//! Explicit opt-in gate for raw-audio test capture.

use crate::prefs::Preferences;

/// Proof that the user enabled raw-audio test capture. It can only be obtained
/// from preferences where `diagnostics.record_raw_audio` is true; the default is off.
/// Captured audio is for local testing only and is never part of a support bundle.
#[derive(Debug, Clone, Copy)]
pub struct RawCaptureConsent(());

impl RawCaptureConsent {
	/// Returns consent only when the user opted in.
	pub fn from_preferences(prefs: &Preferences) -> Option<Self> {
		prefs.diagnostics.record_raw_audio.then_some(Self(()))
	}
}
