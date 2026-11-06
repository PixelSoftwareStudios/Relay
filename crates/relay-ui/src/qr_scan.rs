//! Scanning a QR code with the device camera. The shell owns the camera and
//! reports the text it read; this module owns what that text means. A scanned
//! string takes the same path as a pasted connection string: the same check,
//! the same plain-language problems, and nothing is trusted or dialled by the
//! scan itself. It only fills in the "Add a device by address" field.

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

/// What the Connect section says about the last scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanNote {
	/// A connection string was read and put in the address field.
	Filled,
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

/// The result of a scan turned into what the page shows. `Ok(None)` is a
/// cancelled scan: nothing changes. The second value is the text to put in the
/// address field, if any.
pub fn outcome(result: Result<Option<String>, ScanError>) -> Option<(ScanNote, Option<String>)> {
	match result {
		Ok(None) => None,
		Ok(Some(text)) => Some(match read(&text) {
			Ok(address) => (ScanNote::Filled, Some(address)),
			Err(problem) => (ScanNote::Wrong(problem), None),
		}),
		Err(error) => Some((ScanNote::Failed(error), None)),
	}
}

impl ScanNote {
	/// The sentence for the page.
	pub fn message(self) -> &'static str {
		match self {
			Self::Filled => tr!("scan-filled"),
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

	/// Whether this is a problem (shown as a warning) rather than a success.
	pub fn is_problem(self) -> bool {
		!matches!(self, Self::Filled)
	}
}
