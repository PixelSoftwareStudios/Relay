//! Inputs accepted by the state owner.

use std::time::Duration;

use crate::call::{Gain, TransmitRoute};
use crate::id::{
	AudioGeneration, CallId, ClientId, ControllerLeaseId, InputSourceId, OperationId, PeerId,
	SessionId, SettingsRevision,
};

/// A mutating client request, deduplicated by `(client, operation)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
	/// The authenticated local session making the request.
	pub client: ClientId,
	/// Idempotency key scoped to the client.
	pub operation: OperationId,
	/// The command.
	pub command: Command,
}

/// A client command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
	/// Attach the client session.
	Attach,
	/// Detach the client session, releasing everything it holds.
	Detach,
	/// Start an outgoing call. The caller becomes its controller.
	StartCall {
		/// Fresh call identifier.
		call: CallId,
		/// Fresh session identifier.
		session: SessionId,
		/// The peer to call.
		peer: PeerId,
		/// Allow a second concurrent call to the same peer.
		allow_parallel_same_peer: bool,
	},
	/// Accept an incoming call.
	AcceptCall {
		/// The call.
		call: CallId,
		/// Controller lease proving authority.
		lease: ControllerLeaseId,
	},
	/// Decline an incoming call.
	DeclineCall {
		/// The call.
		call: CallId,
		/// Controller lease proving authority.
		lease: ControllerLeaseId,
	},
	/// Hang up or cancel. Repeating it on a finished call succeeds.
	EndCall {
		/// The call.
		call: CallId,
		/// Controller lease proving authority.
		lease: ControllerLeaseId,
	},
	/// Acquire (or take over) the controller lease of a call.
	AcquireController {
		/// The call.
		call: CallId,
		/// Replace an existing holder.
		takeover: bool,
	},
	/// Give up the controller lease.
	ReleaseController {
		/// The call.
		call: CallId,
		/// The lease being released.
		lease: ControllerLeaseId,
	},
	/// Set explicit transmit consent for a call.
	SetTransmitRoute {
		/// The call.
		call: CallId,
		/// Controller lease proving authority.
		lease: ControllerLeaseId,
		/// The new consent.
		route: TransmitRoute,
	},
	/// Change per-call mute, deafen or receive gain.
	SetCallControls {
		/// The call.
		call: CallId,
		/// Controller lease proving authority.
		lease: ControllerLeaseId,
		/// New mute state, if changing.
		muted: Option<bool>,
		/// New deafen state, if changing.
		deafened: Option<bool>,
		/// New receive gain, if changing.
		gain: Option<Gain>,
	},
	/// Change engine-wide mute, deafen or master gain.
	SetGlobalControls {
		/// New mute state, if changing.
		muted: Option<bool>,
		/// New deafen state, if changing.
		deafened: Option<bool>,
		/// New master output gain, if changing.
		output_gain: Option<Gain>,
		/// New push-to-talk release delay (at most 2000 ms), if changing.
		ptt_release_delay: Option<Duration>,
		/// Reject if settings changed since this revision.
		expected: Option<SettingsRevision>,
	},
	/// Explicitly resume capture after the device was lost.
	ResumeCapture,
	/// A push-to-talk press, refresh or release.
	Ptt(PttUpdate),
}

/// One update from an input source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PttUpdate {
	/// The call chosen when the press began.
	pub call: CallId,
	/// Controller lease the source acts under.
	pub lease: ControllerLeaseId,
	/// The input source.
	pub source: InputSourceId,
	/// Strictly increasing per source; stale or reordered updates are rejected.
	pub sequence: u64,
	/// Key is held (press or refresh) or released.
	pub pressed: bool,
	/// Requested lease length; clamped to the hard maximum.
	pub ttl: Option<Duration>,
	/// Release only: close at once, skipping the release delay. Set for
	/// source retirement and stream closure; an ordinary key-up leaves it off.
	pub immediate: bool,
}

/// Events from the peer-link adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerEvent {
	/// The authenticated secure link is up.
	LinkUp(PeerId),
	/// The secure link is gone.
	LinkDown(PeerId),
	/// An authenticated invitation arrived.
	Invite {
		/// Call identifier chosen by the remote.
		call: CallId,
		/// Session identifier chosen by the remote.
		session: SessionId,
		/// The inviting peer.
		peer: PeerId,
		/// The remote asks for an explicit parallel call.
		allow_parallel_same_peer: bool,
	},
	/// The peer accepted our invitation.
	Accepted {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// The peer declined our invitation.
	Declined {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// The peer hung up or cancelled.
	Ended {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// The bidirectional UDP path was validated.
	PathValidated {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
}

/// OS session lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
	/// The desktop was locked.
	Locked,
	/// The desktop was unlocked.
	Unlocked,
	/// The machine is suspending.
	Suspended,
	/// The machine resumed.
	Resumed,
}

/// Events from device and media adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemEvent {
	/// OS lock/suspend state changed.
	Session(SessionEvent),
	/// Capture finished opening.
	CaptureReady {
		/// The generation the open was issued under.
		generation: AudioGeneration,
	},
	/// Capture failed to open.
	CaptureFailed {
		/// The generation the open was issued under.
		generation: AudioGeneration,
	},
	/// The capture device went away or was invalidated.
	CaptureLost {
		/// The generation that was running.
		generation: AudioGeneration,
	},
	/// Local media resources for a call are ready.
	MediaPrepared {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// Media for a call was stopped and retired.
	MediaStopped {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
}

/// Anything the state owner can be asked to process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
	/// A client request.
	Client(Request),
	/// A client transport dropped without a graceful detach.
	Disconnected(ClientId),
	/// A peer-link event.
	Peer(PeerEvent),
	/// A device, media or OS event.
	System(SystemEvent),
	/// Time advanced; expire leases and timers.
	Tick,
}

impl Input {
	/// Inputs that only close gates. The engine applies them without waiting
	/// behind the bounded command queue.
	pub fn is_safety_deny(&self) -> bool {
		match self {
			Self::Disconnected(_) | Self::Peer(PeerEvent::LinkDown(_)) | Self::Tick => true,
			Self::System(event) => matches!(
				event,
				SystemEvent::Session(SessionEvent::Locked | SessionEvent::Suspended)
					| SystemEvent::CaptureLost { .. }
			),
			Self::Client(request) => match &request.command {
				Command::Ptt(update) => !update.pressed,
				Command::SetGlobalControls {
					muted, deafened, ..
				} => *muted == Some(true) || *deafened == Some(true),
				_ => false,
			},
			Self::Peer(_) => false,
		}
	}
}
