//! The call aggregate and its lifecycle.

mod route;
mod state;

use std::collections::BTreeMap;

pub use route::{CallControls, Gain, RouteMode, TransmitRoute};
pub use state::{CallState, EndReason};

use crate::effect::{GateSnapshot, ReceiveMix};
use crate::id::{
	CallId, ClientId, ControllerLeaseId, InputLeaseId, InputSourceId, PeerId, RouteGeneration,
	SessionId,
};
use crate::time::MonoTime;

/// Who initiated the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
	/// The local engine invited the peer.
	Outgoing,
	/// The peer invited the local engine.
	Incoming,
}

/// Exclusive control over a call held by one client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerLease {
	/// The lease identifier; stale identifiers are rejected.
	pub id: ControllerLeaseId,
	/// The client that holds the lease.
	pub holder: ClientId,
}

/// A renewable push-to-talk authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputLease {
	/// Lease identifier.
	pub id: InputLeaseId,
	/// Instant after which the lease is dead.
	pub expires_at: MonoTime,
	/// The controller lease it was issued under.
	pub controller: ControllerLeaseId,
	/// The route generation it was issued under.
	pub route_generation: RouteGeneration,
}

/// What a pending deadline is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	/// Secure link setup (Preparing).
	LinkSetup,
	/// User answer (ringing).
	Ring,
	/// Media path and resources (Connecting).
	Path,
	/// Media teardown acknowledgement (Ending).
	Teardown,
}

/// A timer owned by a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
	/// When the phase expires.
	pub at: MonoTime,
	/// The phase being timed.
	pub phase: Phase,
}

/// Effects last published for a call, used to emit only changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Published {
	/// Last published gate.
	pub gate: Option<GateSnapshot>,
	/// Last published receive mix.
	pub mix: Option<ReceiveMix>,
}

/// One call as seen by the state owner.
#[derive(Debug, Clone)]
pub struct Call {
	/// Call identifier.
	pub id: CallId,
	/// Session identifier negotiated with the peer.
	pub session: SessionId,
	/// The remote peer.
	pub peer: PeerId,
	/// Who invited whom.
	pub direction: Direction,
	/// Explicit second call to the same peer; skips glare coalescing.
	pub allow_parallel: bool,
	/// Lifecycle state.
	pub state: CallState,
	/// Why the call ended or was interrupted, if known.
	pub reason: Option<EndReason>,
	/// Current controller, if any.
	pub controller: Option<ControllerLease>,
	/// Transmit consent.
	pub route: TransmitRoute,
	/// Generation of the transmit consent.
	pub route_generation: RouteGeneration,
	/// User controls.
	pub controls: CallControls,
	/// Live push-to-talk leases keyed by (client, source).
	pub input_leases: BTreeMap<(ClientId, InputSourceId), InputLease>,
	/// Pending timer.
	pub deadline: Option<Deadline>,
	/// The media path to the peer was validated.
	pub path_validated: bool,
	/// Local media resources are prepared.
	pub media_prepared: bool,
	/// The route generation counter ran out; consent can no longer be granted.
	pub route_exhausted: bool,
	/// What was last published to the media side.
	pub published: Published,
}

impl Call {
	/// Creates a call in its initial state.
	pub fn new(
		id: CallId,
		session: SessionId,
		peer: PeerId,
		direction: Direction,
		allow_parallel: bool,
		state: CallState,
	) -> Self {
		Self {
			id,
			session,
			peer,
			direction,
			allow_parallel,
			state,
			reason: None,
			controller: None,
			route: TransmitRoute::RECEIVE_ONLY,
			route_generation: RouteGeneration::ZERO,
			controls: CallControls::default(),
			input_leases: BTreeMap::new(),
			deadline: None,
			path_validated: false,
			media_prepared: false,
			route_exhausted: false,
			published: Published::default(),
		}
	}

	/// Moves to the next route generation. On exhaustion consent is withdrawn
	/// for good rather than wrapping into an old generation.
	pub fn advance_route_generation(&mut self) {
		match self.route_generation.next() {
			Some(next) => self.route_generation = next,
			None => {
				self.route_exhausted = true;
				self.route = TransmitRoute::RECEIVE_ONLY;
			}
		}
	}

	/// Whether the call still counts against admission limits.
	pub const fn is_live(&self) -> bool {
		!self.state.is_terminal()
	}
}
