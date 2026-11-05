//! Effects requested of adapters. The state owner performs no I/O itself.

use crate::call::Gain;
use crate::gate::DenyReason;
use crate::id::{AudioGeneration, CallId, PeerId, Revision, RouteGeneration, SessionId};

/// A control message for the peer link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerMessage {
	/// Invitation to a new call.
	Invite {
		/// Call identifier (also the invitation identifier).
		call: CallId,
		/// Session identifier chosen by the initiator.
		session: SessionId,
		/// Explicit parallel call to a peer we already call.
		allow_parallel_same_peer: bool,
	},
	/// The invitation was accepted.
	Accept {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// The invitation was declined.
	Decline {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
		/// The local engine is at capacity.
		busy: bool,
	},
	/// Hangup or cancel.
	End {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
}

/// A coherent transmit-gate snapshot for the media send path.
///
/// Frames carrying an older generation than `capture` or `route` must be
/// dropped before packet emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSnapshot {
	/// The call.
	pub call: CallId,
	/// Whether the microphone may be sent.
	pub open: bool,
	/// The deny reason when closed.
	pub deny: Option<DenyReason>,
	/// Current route generation.
	pub route: RouteGeneration,
	/// Current capture generation.
	pub capture: AudioGeneration,
}

/// What the render side should do with a call's decoded audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveMix {
	/// The call.
	pub call: CallId,
	/// Whether the call is audible in the local mix.
	pub audible: bool,
	/// Per-call gain (applied before the master gain).
	pub gain: Gain,
	/// Master output gain.
	pub master: Gain,
}

/// A request to an adapter. Gate updates are always ordered first so a deny
/// is published before any acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
	/// Publish a new gate snapshot.
	GateUpdate {
		/// The snapshot.
		gate: GateSnapshot,
		/// Revision at which it became current.
		revision: Revision,
	},
	/// Update the local receive mix for a call.
	ReceiveMix(ReceiveMix),
	/// Open the shared capture path under a new generation.
	OpenCapture {
		/// The generation completions must echo.
		generation: AudioGeneration,
	},
	/// Close the shared capture path.
	CloseCapture {
		/// The generation being closed.
		generation: AudioGeneration,
	},
	/// Establish (or reuse) the secure link to a peer.
	EnsureLink {
		/// The peer.
		peer: PeerId,
	},
	/// Send a control message to a peer.
	Peer {
		/// The peer.
		peer: PeerId,
		/// The message.
		message: PeerMessage,
	},
	/// Prepare media resources and start path validation (no transmit).
	PrepareMedia {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
	/// Stop media for a call and retire its resources.
	StopMedia {
		/// The call.
		call: CallId,
		/// Its session.
		session: SessionId,
	},
}

impl Effect {
	/// Whether this is a gate update (these are ordered first).
	pub const fn is_gate(&self) -> bool {
		matches!(self, Self::GateUpdate { .. })
	}
}
