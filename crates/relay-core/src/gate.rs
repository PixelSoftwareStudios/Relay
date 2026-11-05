//! The transmit gate as a pure policy function.
//!
//! Transmitting is not a lifecycle state. The gate is the conjunction of
//! independent conditions and every deny wins, so each can be tested alone.

use crate::call::RouteMode;

/// Facts the gate depends on for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateInputs {
	/// Engine-wide mute.
	pub global_muted: bool,
	/// Engine-wide deafen (suppresses outgoing routes by default).
	pub global_deafened: bool,
	/// The OS session is neither locked nor suspended.
	pub session_allowed: bool,
	/// The shared capture path is open and its generation valid.
	pub capture_ready: bool,
	/// The call is active and accepted.
	pub call_active: bool,
	/// The authenticated peer link is up.
	pub link_up: bool,
	/// Consent mode of the route.
	pub route: RouteMode,
	/// A controller exists, or the route carries background authority.
	pub authority_valid: bool,
	/// Per-call mute.
	pub call_muted: bool,
	/// Per-call deafen.
	pub call_deafened: bool,
	/// A live push-to-talk lease exists.
	pub ptt_held: bool,
}

/// The first deny condition that closed the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DenyReason {
	/// Global mute is on.
	GlobalMuted,
	/// Global deafen is on.
	GlobalDeafened,
	/// The OS session is locked or suspended.
	SessionBlocked,
	/// Capture is not ready for the current generation.
	CaptureInvalid,
	/// The call is not active.
	CallNotActive,
	/// The peer link is down.
	LinkDown,
	/// No explicit transmit consent (receive-only).
	NotAuthorized,
	/// No controller and no background authority.
	NoAuthority,
	/// Per-call mute is on.
	CallMuted,
	/// Per-call deafen is on.
	CallDeafened,
	/// Push-to-talk mode without a live lease.
	PttIdle,
}

/// Result of evaluating the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
	/// Microphone audio may be sent on this route.
	Open,
	/// Sending is denied for the given reason.
	Closed(DenyReason),
}

impl GateDecision {
	/// Whether the gate is open.
	pub const fn is_open(self) -> bool {
		matches!(self, Self::Open)
	}

	/// The deny reason, if closed.
	pub const fn deny(self) -> Option<DenyReason> {
		match self {
			Self::Open => None,
			Self::Closed(reason) => Some(reason),
		}
	}
}

/// Evaluates the gate. Open only if no deny condition holds.
pub fn evaluate(g: &GateInputs) -> GateDecision {
	use DenyReason::*;
	let denial = if g.global_muted {
		Some(GlobalMuted)
	} else if g.global_deafened {
		Some(GlobalDeafened)
	} else if !g.session_allowed {
		Some(SessionBlocked)
	} else if !g.capture_ready {
		Some(CaptureInvalid)
	} else if !g.call_active {
		Some(CallNotActive)
	} else if !g.link_up {
		Some(LinkDown)
	} else if g.route == RouteMode::ReceiveOnly {
		Some(NotAuthorized)
	} else if !g.authority_valid {
		Some(NoAuthority)
	} else if g.call_muted {
		Some(CallMuted)
	} else if g.call_deafened {
		Some(CallDeafened)
	} else if g.route == RouteMode::PushToTalk && !g.ptt_held {
		Some(PttIdle)
	} else {
		None
	};
	denial.map_or(GateDecision::Open, GateDecision::Closed)
}
