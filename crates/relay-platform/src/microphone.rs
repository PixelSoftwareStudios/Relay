//! The OS permission to record, for platforms that ask the user before the
//! first stream opens (macOS TCC). Reading the status never prompts; only
//! [`request`] does, and only while the status is [`Authorization::NotDetermined`].

/// What the OS says about this process recording audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
	/// The user was never asked: a prompt will appear on [`request`].
	NotDetermined,
	/// Refused by the user; only the system settings can change it.
	Denied,
	/// Refused by a policy (parental controls, a profile); the user cannot change it here.
	Restricted,
	/// Allowed.
	Authorized,
	/// This platform has no such permission (nothing to ask).
	NotRequired,
}

/// The current status, read without prompting.
pub fn status() -> Authorization {
	imp::status()
}

/// Show the system prompt when the status is `NotDetermined`; `done` gets the
/// user's answer (`true` = allowed), possibly from another thread. When there
/// is nothing to ask, `done` runs at once with the current answer.
pub fn request(done: impl FnOnce(bool) + Send + 'static) {
	imp::request(Box::new(done));
}

#[cfg(target_os = "macos")]
mod imp {
	use std::cell::Cell;

	use block2::RcBlock;
	use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

	use super::Authorization;

	pub fn status() -> Authorization {
		let Some(audio) = (
			// SAFETY: reading an extern constant that AVFoundation initialises
			// when the framework loads; it is `None` only on a system without it.
			unsafe { AVMediaTypeAudio }
		) else {
			return Authorization::NotRequired;
		};
		// SAFETY: a class method taking the audio media type constant, which is
		// one of the two types the method accepts; it only reads the TCC state.
		let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
		if status == AVAuthorizationStatus::Authorized {
			Authorization::Authorized
		} else if status == AVAuthorizationStatus::Denied {
			Authorization::Denied
		} else if status == AVAuthorizationStatus::Restricted {
			Authorization::Restricted
		} else {
			Authorization::NotDetermined
		}
	}

	pub fn request(done: Box<dyn FnOnce(bool) + Send>) {
		let Some(audio) = (
			// SAFETY: as in `status`.
			unsafe { AVMediaTypeAudio }
		) else {
			done(false);
			return;
		};
		// The handler may run once, on any queue; `Cell<Option<_>>` hands the
		// one-shot closure out of a block that is typed `Fn`.
		let slot = Cell::new(Some(done));
		let handler = RcBlock::new(move |granted: objc2::runtime::Bool| {
			if let Some(done) = slot.take() {
				done(granted.as_bool());
			}
		});
		// SAFETY: a class method taking the audio media type constant and a
		// completion block that the framework copies and calls at most once.
		unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler) };
	}
}

#[cfg(not(target_os = "macos"))]
mod imp {
	use super::Authorization;

	pub fn status() -> Authorization {
		Authorization::NotRequired
	}

	pub fn request(done: Box<dyn FnOnce(bool) + Send>) {
		done(true);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reading_the_status_never_prompts() {
		// Linking AVFoundation and asking the question must work everywhere;
		// off macOS there is nothing to ask.
		let status = status();
		if cfg!(not(target_os = "macos")) {
			assert_eq!(status, Authorization::NotRequired);
		}
	}

	#[test]
	fn a_platform_with_nothing_to_ask_answers_at_once() {
		if cfg!(not(target_os = "macos")) {
			let (tx, rx) = std::sync::mpsc::channel();
			request(move |ok| tx.send(ok).unwrap());
			assert!(rx.try_recv().unwrap());
		}
	}
}
