//! Scanning a QR code with the device camera. The shell owns the camera and
//! reports the text it read; this module owns what that text means. A scanned
//! string takes the same check as a pasted connection string and the same
//! plain-language problems. Unlike a pasted string, a valid scan is dialled and
//! trusted at once (`Controller::connect_and_trust`): the code is shown on the
//! other device's screen, so scanning it is the out-of-band check.

use relay_client::connection::{self, ConnectionStringProblem};

use crate::tr;

/// Why a scan produced no text, as the shell can tell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
	/// The camera permission was refused, but the OS will ask again.
	CameraDenied,
	/// The camera permission was refused for good: only the settings change it.
	CameraBlocked,
	/// This device has no scanner (no camera, or the OS component is missing).
	Unavailable,
	/// The scanner failed for another reason; the shell logs the detail.
	Failed,
}

/// What the Connect page says about a scan that read no usable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanNote {
	/// The code is not a usable connection string.
	Wrong(ConnectionStringProblem),
	/// No code was read.
	Failed(ScanError),
}

/// The text of a scan as a connection string, or why it is not one. Same
/// check as a pasted string; surrounding whitespace is ignored.
pub fn read(scanned: &str) -> Result<String, ConnectionStringProblem> {
	connection::check(scanned).map(|()| scanned.trim().to_owned())
}

/// What a finished scan asks the UI to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scanned {
	/// A valid connection string: dial it and trust the device.
	Connect(String),
	/// Nothing to dial: say why.
	Note(ScanNote),
}

/// The result of a scan turned into what the page does. `Ok(None)` is a
/// cancelled scan: nothing changes.
pub fn outcome(result: Result<Option<String>, ScanError>) -> Option<Scanned> {
	match result {
		Ok(None) => None,
		Ok(Some(text)) => Some(match read(&text) {
			Ok(address) => Scanned::Connect(address),
			Err(problem) => Scanned::Note(ScanNote::Wrong(problem)),
		}),
		Err(error) => Some(Scanned::Note(ScanNote::Failed(error))),
	}
}

/// How "connect and trust" after a scan stands or ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanTrust {
	/// Dialling and checking the key.
	Working,
	/// Trusted; the lowercase hex key digest of the device.
	Added(String),
	/// The address did not answer.
	Unreachable,
	/// The device that answered is not the one in the code; nothing was stored.
	Mismatch,
	/// Anything else, in the engine's words.
	Failed(String),
}

impl ScanTrust {
	/// Sort the engine's refusal into what a person can act on.
	pub fn from_failure(message: &str) -> Self {
		let lower = message.to_lowercase();
		let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
		if has(&["match", "fingerprint"]) {
			Self::Mismatch
		} else if has(&["reach", "timed out", "timeout", "refused", "no route"]) {
			Self::Unreachable
		} else {
			Self::Failed(message.to_owned())
		}
	}

	/// The lowercase key digest at the end of a connection string.
	pub fn digest_of(connection_string: &str) -> String {
		connection_string
			.rsplit('#')
			.next()
			.unwrap_or_default()
			.to_lowercase()
	}
}

impl ScanNote {
	/// The sentence for the page.
	pub fn message(self) -> &'static str {
		match self {
			Self::Wrong(ConnectionStringProblem::Scheme) => tr!("scan-wrong"),
			Self::Wrong(ConnectionStringProblem::MissingFingerprint) => {
				tr!("conn-missing-fingerprint")
			}
			Self::Wrong(ConnectionStringProblem::BadFingerprint) => tr!("conn-bad-fingerprint"),
			Self::Wrong(ConnectionStringProblem::BadAddress) => tr!("conn-bad-address"),
			Self::Failed(ScanError::CameraDenied) => tr!("scan-camera-denied"),
			Self::Failed(ScanError::CameraBlocked) => tr!("scan-camera-blocked"),
			Self::Failed(ScanError::Unavailable) => tr!("scan-unavailable"),
			Self::Failed(ScanError::Failed) => tr!("scan-failed"),
		}
	}
}
