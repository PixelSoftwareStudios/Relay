//! What the media layer reports upward. Sinks must not block: workers call
//! them from the deadline path.

use std::sync::Arc;

use relay_core::{PeerEvent, SystemEvent};

/// A completion or fault the owner of the domain state must hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvent {
	/// Capture, preparation or stop completions and capture loss.
	System(SystemEvent),
	/// Media-path validation.
	Peer(PeerEvent),
	/// The playback device went away and reopening it keeps failing. Remote
	/// audio is paused until it returns or the user chooses an output; the
	/// media layer keeps trying and sends `OutputRestored` when it succeeds.
	/// A route change that a reopen fixes never produces this.
	OutputLost,
	/// The playback device is back after `OutputLost`.
	OutputRestored,
	/// The microphone could not be opened (no device, permission refused, in
	/// use). The domain also gets `CaptureFailed`; this carries the reason so
	/// the user is told what to do.
	CaptureUnavailable(String),
	/// The microphone opened (for a call or a test): an earlier
	/// `CaptureUnavailable` no longer holds.
	CaptureOpened,
	/// A media thread panicked or stopped responding. Gates are already
	/// closed and the affected call (if known) is out of the media path; the
	/// owner should tell the user and end calls that cannot recover.
	WorkerFailed(super::WorkerFailure),
}

/// Non-blocking receiver of media events.
pub type EventSink = Arc<dyn Fn(MediaEvent) + Send + Sync>;
