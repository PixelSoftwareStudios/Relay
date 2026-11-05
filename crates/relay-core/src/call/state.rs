//! Call lifecycle states and the permitted transition table.

/// Lifecycle state of a call. Transmitting is deliberately not a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallState {
	/// Local request accepted; link and trust checks pending.
	Preparing,
	/// The peer received our invitation.
	OutgoingRinging,
	/// An authenticated invitation awaits the local user.
	IncomingRinging,
	/// Both sides accepted; media path and resources are being validated.
	Connecting,
	/// Media path ready (not necessarily transmitting).
	Active,
	/// Link, device or session lifecycle invalidated the call.
	Interrupted,
	/// Local stop ordered; gates already closed.
	Ending,
	/// Terminal: finished normally.
	Ended,
	/// Terminal: declined.
	Declined,
	/// Terminal: cancelled before it connected.
	Cancelled,
	/// Terminal: failed.
	Failed,
}

impl CallState {
	/// Whether no further transition is possible.
	pub const fn is_terminal(self) -> bool {
		matches!(
			self,
			Self::Ended | Self::Declined | Self::Cancelled | Self::Failed
		)
	}

	/// Whether media resources may exist in this state.
	pub const fn has_media(self) -> bool {
		matches!(
			self,
			Self::Connecting | Self::Active | Self::Interrupted | Self::Ending
		)
	}

	/// The lifecycle transition table.
	pub const fn can_transition_to(self, to: Self) -> bool {
		use CallState::*;
		matches!(
			(self, to),
			(Preparing, OutgoingRinging | Failed | Ended)
				| (OutgoingRinging, Connecting | Declined | Cancelled | Failed)
				| (IncomingRinging, Connecting | Declined | Ended | Failed)
				| (Connecting, Active | Failed | Ended | Ending)
				| (Active, Interrupted | Ending | Failed)
				| (Interrupted, Connecting | Ending | Failed)
				| (Ending, Ended)
		)
	}
}

/// Why a call left its non-terminal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndReason {
	/// The local user hung up.
	LocalHangup,
	/// The remote side hung up or cancelled.
	RemoteHangup,
	/// The local user declined.
	LocalDeclined,
	/// The remote side declined.
	RemoteDeclined,
	/// The local user cancelled an outgoing invitation.
	LocalCancelled,
	/// The secure link could not be set up in time.
	LinkSetupTimeout,
	/// Nobody answered in time.
	RingTimeout,
	/// The media path was not ready in time.
	PathTimeout,
	/// The peer link was lost.
	LinkLost,
	/// This invitation lost the simultaneous-invite tie-break.
	GlareLost,
	/// Media teardown was not acknowledged in time.
	TeardownTimeout,
	/// Media teardown completed.
	TeardownComplete,
	/// A generation counter ran out.
	GenerationExhausted,
	/// The client that owned the call went away and nobody took it over.
	OwnerGone,
}
