//! What the media layer reports upward. Sinks must not block: workers call
//! them from the deadline path.

use std::sync::Arc;

use relay_core::{PeerEvent, SystemEvent};

/// A completion or fault the owner of the domain state must hear about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaEvent {
	/// Capture, preparation or stop completions and capture loss.
	System(SystemEvent),
	/// Media-path validation.
	Peer(PeerEvent),
	/// The playback device went away. Remote audio is paused until the user
	/// chooses an output; nothing is redirected automatically.
	OutputLost,
}

/// Non-blocking receiver of media events.
pub type EventSink = Arc<dyn Fn(MediaEvent) + Send + Sync>;
