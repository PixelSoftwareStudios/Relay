//! Revisioned events, views and the snapshot a watcher folds them into.
//!
//! Events carry whole updated views, so folding them onto a snapshot is
//! trivial and any gap is detectable from the revision alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use thiserror::Error;

use crate::call::{Call, CallControls, CallState, Direction, EndReason, Gain, TransmitRoute};
use crate::gate::DenyReason;
use crate::id::{
	AudioGeneration, CallId, ClientId, ControllerLeaseId, Incarnation, PeerId, Revision,
	RouteGeneration, SessionId, SettingsRevision,
};

/// State of the shared capture path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureState {
	/// Not open; nothing needs it.
	Closed,
	/// Open requested, completion pending.
	Opening,
	/// Open and valid for the current generation.
	Ready,
	/// Device lost or failed; waits for an explicit resume.
	Lost,
}

/// Engine-wide settings and device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalView {
	/// User mute (preserved across deafen).
	pub muted: bool,
	/// User deafen.
	pub deafened: bool,
	/// Master output gain.
	pub output_gain: Gain,
	/// Push-to-talk release delay.
	pub ptt_release_delay: Duration,
	/// Settings revision.
	pub settings: SettingsRevision,
	/// Desktop locked.
	pub locked: bool,
	/// Machine suspended.
	pub suspended: bool,
	/// Capture path state.
	pub capture: CaptureState,
	/// Capture generation.
	pub capture_generation: AudioGeneration,
}

/// Gate state of one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateView {
	/// Whether microphone audio is being sent.
	pub open: bool,
	/// Why it is not, when closed.
	pub deny: Option<DenyReason>,
}

/// Public view of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallView {
	/// Call identifier.
	pub id: CallId,
	/// Session identifier.
	pub session: SessionId,
	/// Remote peer.
	pub peer: PeerId,
	/// Direction.
	pub direction: Direction,
	/// Lifecycle state.
	pub state: CallState,
	/// End or interruption reason.
	pub reason: Option<EndReason>,
	/// Controller, if any.
	pub controller: Option<(ClientId, ControllerLeaseId)>,
	/// Transmit consent.
	pub route: TransmitRoute,
	/// Route generation.
	pub route_generation: RouteGeneration,
	/// User controls.
	pub controls: CallControls,
	/// A push-to-talk lease is live.
	pub ptt_held: bool,
	/// Every live lease is in its release hold (key already up).
	pub releasing: bool,
	/// Gate state.
	pub gate: GateView,
}

impl CallView {
	pub(crate) fn of(call: &Call) -> Self {
		let gate = call.published.gate;
		Self {
			id: call.id,
			session: call.session,
			peer: call.peer,
			direction: call.direction,
			state: call.state,
			reason: call.reason,
			controller: call.controller.map(|lease| (lease.holder, lease.id)),
			route: call.route,
			route_generation: call.route_generation,
			controls: call.controls,
			ptt_held: !call.input_leases.is_empty(),
			releasing: !call.input_leases.is_empty()
				&& call.input_leases.values().all(|lease| lease.holding),
			gate: GateView {
				open: gate.is_some_and(|g| g.open),
				deny: gate.and_then(|g| g.deny),
			},
		}
	}
}

/// Why push-to-talk authority was withdrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFault {
	/// The lease expired without refresh.
	Expired,
	/// The owning client disconnected.
	ClientGone,
	/// Another client took the controller lease.
	Takeover,
	/// The controller lease was released or lost.
	ControllerLost,
	/// The transmit route changed generation.
	RouteChanged,
	/// The OS session locked or suspended.
	SessionBlocked,
	/// The capture device was invalidated.
	DeviceInvalid,
	/// The call ended or was interrupted.
	CallEnded,
}

/// A lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
	/// A client attached.
	ClientAttached(ClientId),
	/// A client detached.
	ClientDetached(ClientId),
	/// A peer link changed.
	LinkChanged {
		/// The peer.
		peer: PeerId,
		/// Whether the link is up.
		up: bool,
	},
	/// Engine-wide state changed.
	GlobalUpdated(GlobalView),
	/// A call was created or changed.
	CallUpdated(CallView),
	/// A retained terminal call was evicted.
	CallForgotten(CallId),
	/// Push-to-talk authority was withdrawn (informational, for UI faults).
	InputFault {
		/// The call.
		call: CallId,
		/// Why.
		fault: InputFault,
	},
}

/// An event with its revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revisioned {
	/// Position in the lifecycle sequence.
	pub revision: Revision,
	/// The event.
	pub event: Event,
}

/// Full state as of a revision; the base a watcher continues from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
	/// Engine incarnation.
	pub incarnation: Incarnation,
	/// Revision this snapshot reflects.
	pub revision: Revision,
	/// Engine-wide state.
	pub global: GlobalView,
	/// Attached clients.
	pub clients: BTreeSet<ClientId>,
	/// Peers with an up link.
	pub links: BTreeSet<PeerId>,
	/// Known calls, including retained terminal ones.
	pub calls: BTreeMap<CallId, CallView>,
}

/// Folding an event that does not directly follow the snapshot's revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("event revision {got} does not follow snapshot revision {have}")]
pub struct GapError {
	/// Revision the snapshot is at.
	pub have: Revision,
	/// Revision of the offending event.
	pub got: Revision,
}

impl Snapshot {
	/// Position to continue watching from.
	pub fn cursor(&self) -> Cursor {
		Cursor {
			incarnation: self.incarnation,
			revision: self.revision,
		}
	}

	/// Folds the next event. Fails on a gap instead of silently diverging.
	pub fn apply(&mut self, item: &Revisioned) -> Result<(), GapError> {
		if self.revision.next() != Some(item.revision) {
			return Err(GapError {
				have: self.revision,
				got: item.revision,
			});
		}
		self.revision = item.revision;
		match &item.event {
			Event::ClientAttached(id) => {
				self.clients.insert(*id);
			}
			Event::ClientDetached(id) => {
				self.clients.remove(id);
			}
			Event::LinkChanged { peer, up } => {
				if *up {
					self.links.insert(*peer);
				} else {
					self.links.remove(peer);
				}
			}
			Event::GlobalUpdated(global) => self.global = *global,
			Event::CallUpdated(view) => {
				self.calls.insert(view.id, view.clone());
			}
			Event::CallForgotten(id) => {
				self.calls.remove(id);
			}
			Event::InputFault { .. } => {}
		}
		Ok(())
	}
}

/// A watcher's position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
	/// The incarnation the position belongs to.
	pub incarnation: Incarnation,
	/// The last revision the watcher has seen.
	pub revision: Revision,
}

/// Result of asking for events after a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
	/// Every event after the cursor, in order, with no gap.
	Events(Vec<Revisioned>),
	/// The watcher fell behind or belongs to another incarnation; replace
	/// local state with this snapshot and continue from its cursor.
	Resync(Box<Snapshot>),
}

/// Bounded in-memory replay window.
#[derive(Debug)]
pub(crate) struct EventLog {
	events: VecDeque<Revisioned>,
	capacity: usize,
}

impl EventLog {
	pub(crate) fn new(capacity: usize) -> Self {
		Self {
			events: VecDeque::with_capacity(capacity.min(1024)),
			capacity,
		}
	}

	pub(crate) fn push(&mut self, item: Revisioned) {
		if self.events.len() == self.capacity {
			self.events.pop_front();
		}
		self.events.push_back(item);
	}

	/// Events after `since`, or `None` if the window no longer covers them
	/// or more than `max_lag` would have to be delivered.
	pub(crate) fn after(&self, since: Revision, max_lag: usize) -> Option<Vec<Revisioned>> {
		let pending: Vec<_> = self
			.events
			.iter()
			.filter(|item| item.revision > since)
			.cloned()
			.collect();
		let covered = pending
			.first()
			.is_none_or(|first| since.next() == Some(first.revision));
		(covered && pending.len() <= max_lag).then_some(pending)
	}
}
