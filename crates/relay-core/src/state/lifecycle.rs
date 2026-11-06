//! Call lifecycle: local commands, the transition engine and timers.

use super::State;
use crate::call::{Call, CallState, Deadline, Direction, EndReason, Phase};
use crate::effect::{Effect, PeerMessage};
use crate::error::Reject;
use crate::event::{Event, InputFault};
use crate::id::{CallId, ClientId, ControllerLeaseId, PeerId, SessionId};
use crate::outcome::{OpResult, Reply};
use crate::time::MonoTime;

impl State {
	pub(super) fn start_call(
		&mut self,
		client: ClientId,
		call: CallId,
		session: SessionId,
		peer: PeerId,
		allow_parallel: bool,
		now: MonoTime,
	) -> OpResult {
		if self.calls.contains_key(&call) || self.aliases.contains_key(&call) {
			return Err(Reject::CallIdInUse(call));
		}
		if self.live_calls() >= self.limits.active_calls {
			return Err(Reject::CallCapReached);
		}
		if !allow_parallel
			&& let Some(existing) = self
				.calls
				.values()
				.find(|c| c.is_live() && c.peer == peer && !c.allow_parallel)
		{
			return Err(Reject::SamePeerCallExists(existing.id));
		}
		let lease = self.grant_lease(client);
		let mut entry = Call::new(
			call,
			session,
			peer,
			Direction::Outgoing,
			allow_parallel,
			CallState::Preparing,
		);
		entry.controller = Some(lease);
		entry.deadline = Some(Deadline {
			at: now.saturating_add(self.limits.link_setup_timeout),
			phase: Phase::LinkSetup,
		});
		self.calls.insert(call, entry);
		self.mark_dirty(call);
		if self.links.contains(&peer) {
			self.begin_ringing(call, now);
		} else {
			self.push_effect(Effect::EnsureLink { peer });
		}
		Ok(Reply::Controller(lease.id))
	}

	/// Preparing -> OutgoingRinging and send the invitation.
	pub(crate) fn begin_ringing(&mut self, id: CallId, now: MonoTime) {
		let Some(call) = self.calls.get(&id) else {
			return;
		};
		let (peer, session, parallel) = (call.peer, call.session, call.allow_parallel);
		self.transition(id, CallState::OutgoingRinging, None, now);
		let message = PeerMessage::Invite {
			call: id,
			session,
			allow_parallel_same_peer: parallel,
		};
		self.push_effect(Effect::Peer { peer, message });
	}

	pub(super) fn accept_call(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
		now: MonoTime,
	) -> OpResult {
		let id = self.authorize(client, call, lease)?;
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(call))?;
		let (peer, session) = (entry.peer, entry.session);
		match entry.state {
			CallState::IncomingRinging => {
				self.transition(id, CallState::Connecting, None, now);
				self.push_effect(Effect::Peer {
					peer,
					message: PeerMessage::Accept { call: id, session },
				});
				self.push_effect(Effect::PrepareMedia { call: id, session });
				Ok(Reply::Done)
			}
			CallState::Connecting | CallState::Active => Ok(Reply::Done),
			state => Err(Reject::InvalidState { state }),
		}
	}

	pub(super) fn decline_call(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
		now: MonoTime,
	) -> OpResult {
		let id = match self.authorize(client, call, lease) {
			Err(Reject::CallTerminal(crate::call::CallState::Declined)) => return Ok(Reply::Done),
			other => other?,
		};
		self.decline_incoming(id, EndReason::LocalDeclined, now)
	}

	fn decline_incoming(&mut self, id: CallId, reason: EndReason, now: MonoTime) -> OpResult {
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(id))?;
		let (peer, session) = (entry.peer, entry.session);
		if entry.state != CallState::IncomingRinging {
			return Err(Reject::InvalidState { state: entry.state });
		}
		self.transition(id, CallState::Declined, Some(reason), now);
		let message = PeerMessage::Decline {
			call: id,
			session,
			busy: false,
		};
		self.push_effect(Effect::Peer { peer, message });
		Ok(Reply::Done)
	}

	pub(super) fn end_call(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
		now: MonoTime,
	) -> OpResult {
		let id = match self.authorize(client, call, lease) {
			// Duplicate hangup on a finished call is a success.
			Err(Reject::CallTerminal(_)) => return Ok(Reply::Done),
			other => other?,
		};
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(call))?;
		let (peer, session) = (entry.peer, entry.session);
		let end = PeerMessage::End { call: id, session };
		match entry.state {
			CallState::Preparing => {
				self.transition(id, CallState::Ended, Some(EndReason::LocalHangup), now);
			}
			CallState::OutgoingRinging => {
				self.transition(
					id,
					CallState::Cancelled,
					Some(EndReason::LocalCancelled),
					now,
				);
				self.push_effect(Effect::Peer { peer, message: end });
			}
			CallState::IncomingRinging => {
				return self.decline_incoming(id, EndReason::LocalDeclined, now);
			}
			CallState::Connecting | CallState::Active | CallState::Interrupted => {
				self.transition(id, CallState::Ending, Some(EndReason::LocalHangup), now);
				self.push_effect(Effect::Peer { peer, message: end });
			}
			_ => {}
		}
		Ok(Reply::Done)
	}

	/// Moves a call along the lifecycle table and applies the fail-closed
	/// side effects of entering the new state. Illegal moves are ignored.
	pub(crate) fn transition(
		&mut self,
		id: CallId,
		to: CallState,
		reason: Option<EndReason>,
		now: MonoTime,
	) {
		let Some(call) = self.calls.get_mut(&id) else {
			return;
		};
		let from = call.state;
		if !from.can_transition_to(to) {
			debug_assert!(false, "illegal transition {from:?} -> {to:?}");
			return;
		}
		call.state = to;
		call.reason = reason.or(call.reason);
		let session = call.session;
		let timeouts = &self.limits;
		call.deadline = match to {
			CallState::OutgoingRinging | CallState::IncomingRinging => {
				Some((timeouts.ring_timeout, Phase::Ring))
			}
			CallState::Connecting => Some((timeouts.path_timeout, Phase::Path)),
			CallState::Ending => Some((timeouts.teardown_timeout, Phase::Teardown)),
			_ => None,
		}
		.map(|(span, phase)| Deadline {
			at: now.saturating_add(span),
			phase,
		});
		if matches!(to, CallState::Connecting | CallState::Interrupted) {
			call.route_chosen = false;
		}
		if to == CallState::Connecting {
			call.path_validated = false;
			call.media_prepared = false;
		}
		if matches!(to, CallState::Interrupted | CallState::Ending) || to.is_terminal() {
			self.reset_route(id, InputFault::CallEnded);
		}
		if to.is_terminal() {
			self.close_leases(id, InputFault::CallEnded);
			if let Some(call) = self.calls.get_mut(&id) {
				call.controller = None;
			}
			self.retire(id);
		}
		if to == CallState::Ending || (to == CallState::Failed && from.has_media()) {
			self.push_effect(Effect::StopMedia { call: id, session });
		}
		self.mark_dirty(id);
	}

	/// Keeps a bounded number of finished calls for idempotent answers.
	fn retire(&mut self, id: CallId) {
		self.terminal_order.push_back(id);
		while self.terminal_order.len() > self.limits.retained_terminal_calls {
			if let Some(old) = self.terminal_order.pop_front() {
				self.calls.remove(&old);
				self.push_event(Event::CallForgotten(old));
			}
		}
	}

	/// Both the path and local resources are ready: Connecting -> Active.
	pub(crate) fn try_activate(&mut self, id: CallId) {
		let ready = self.calls.get(&id).is_some_and(|c| {
			c.state == CallState::Connecting && c.path_validated && c.media_prepared
		});
		if ready {
			self.transition(id, CallState::Active, None, MonoTime::ZERO);
			self.apply_default_route(id);
		}
	}

	pub(super) fn media_stopped(&mut self, call: CallId, session: SessionId) -> OpResult {
		let id = self.checked_session(call, session)?;
		let state = self.calls.get(&id).map(|c| c.state);
		if state == Some(CallState::Ending) {
			self.transition(id, CallState::Ended, None, MonoTime::ZERO);
		}
		Ok(Reply::Done)
	}

	/// Fires due call timers in call-id order.
	pub(super) fn run_timers(&mut self, now: MonoTime) {
		let due: Vec<(CallId, Phase)> = self
			.calls
			.values()
			.filter_map(|c| c.deadline.filter(|d| d.at <= now).map(|d| (c.id, d.phase)))
			.collect();
		for (id, phase) in due {
			let Some(call) = self.calls.get(&id) else {
				continue;
			};
			let (peer, session, direction) = (call.peer, call.session, call.direction);
			let (to, reason) = match phase {
				Phase::LinkSetup => (CallState::Failed, EndReason::LinkSetupTimeout),
				Phase::Ring => (CallState::Failed, EndReason::RingTimeout),
				Phase::Path => (CallState::Failed, EndReason::PathTimeout),
				Phase::Teardown => (CallState::Ended, EndReason::TeardownTimeout),
			};
			let notify = match (phase, direction) {
				(Phase::Ring, Direction::Incoming) => Some(PeerMessage::Decline {
					call: id,
					session,
					busy: false,
				}),
				(Phase::Ring | Phase::Path, _) => Some(PeerMessage::End { call: id, session }),
				_ => None,
			};
			self.transition(id, to, Some(reason), now);
			if let Some(message) = notify {
				self.push_effect(Effect::Peer { peer, message });
			}
		}
	}

	/// Resolves an alias and verifies the session identifier still matches.
	pub(crate) fn checked_session(
		&self,
		call: CallId,
		session: SessionId,
	) -> Result<CallId, Reject> {
		let id = self.resolve(call);
		match self.calls.get(&id) {
			None => Err(Reject::UnknownCall(call)),
			Some(entry) if entry.session != session => Err(Reject::StaleSession),
			Some(_) => Ok(id),
		}
	}
}
