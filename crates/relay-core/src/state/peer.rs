//! Peer-link events: invitations, glare resolution and link health.

use super::State;
use crate::call::{Call, CallState, Deadline, Direction, EndReason, Phase};
use crate::command::PeerEvent;
use crate::effect::{Effect, PeerMessage};
use crate::error::Reject;
use crate::event::Event;
use crate::id::{CallId, PeerId, SessionId};
use crate::outcome::{OpResult, Reply};
use crate::time::MonoTime;

/// Stable tie-break key from the authenticated initiator key and the
/// invitation. Never derived from display names.
pub(crate) fn tie_key(initiator: &PeerId, call: CallId) -> [u8; 32] {
	let mut hasher = blake3::Hasher::new();
	hasher.update(initiator.as_bytes());
	hasher.update(&call.to_bytes());
	*hasher.finalize().as_bytes()
}

impl State {
	pub(super) fn on_peer(&mut self, event: PeerEvent, now: MonoTime) -> OpResult {
		match event {
			PeerEvent::LinkUp(peer) => {
				self.link_up(peer, now);
				Ok(Reply::Done)
			}
			PeerEvent::LinkDown(peer) => {
				self.link_down(peer, now);
				Ok(Reply::Done)
			}
			PeerEvent::Invite {
				call,
				session,
				peer,
				allow_parallel_same_peer,
			} => self.on_invite(call, session, peer, allow_parallel_same_peer, now),
			PeerEvent::Accepted { call, session } => self.on_accepted(call, session, now),
			PeerEvent::Declined { call, session } => {
				let id = self.checked_session(call, session)?;
				if self.state_of(id) == Some(CallState::OutgoingRinging) {
					self.transition(
						id,
						CallState::Declined,
						Some(EndReason::RemoteDeclined),
						now,
					);
				}
				Ok(Reply::Done)
			}
			PeerEvent::Ended { call, session } => self.on_remote_end(call, session, now),
			PeerEvent::PathValidated { call, session } => {
				let id = self.checked_session(call, session)?;
				match self.state_of(id) {
					Some(CallState::Connecting) => {
						if let Some(entry) = self.calls.get_mut(&id) {
							entry.path_validated = true;
						}
						self.try_activate(id);
						Ok(Reply::Done)
					}
					Some(CallState::Active) => Ok(Reply::Done),
					Some(state) => Err(Reject::InvalidState { state }),
					None => Err(Reject::UnknownCall(call)),
				}
			}
		}
	}

	fn state_of(&self, id: CallId) -> Option<CallState> {
		self.calls.get(&id).map(|call| call.state)
	}

	fn calls_of(&self, peer: PeerId) -> Vec<CallId> {
		self.calls
			.values()
			.filter(|call| call.peer == peer)
			.map(|call| call.id)
			.collect()
	}

	fn link_up(&mut self, peer: PeerId, now: MonoTime) {
		if self.links.insert(peer) {
			self.push_event(Event::LinkChanged { peer, up: true });
		}
		for id in self.calls_of(peer) {
			match self.state_of(id) {
				Some(CallState::Preparing) => self.begin_ringing(id, now),
				Some(CallState::Interrupted) => {
					// Resume only a call that still exists on both sides; the
					// route stays withdrawn and PTT never revives.
					self.transition(id, CallState::Connecting, None, now);
					if let Some(session) = self.calls.get(&id).map(|c| c.session) {
						self.push_effect(Effect::PrepareMedia { call: id, session });
					}
				}
				_ => {}
			}
		}
	}

	fn link_down(&mut self, peer: PeerId, now: MonoTime) {
		if self.links.remove(&peer) {
			self.push_event(Event::LinkChanged { peer, up: false });
		}
		for id in self.calls_of(peer) {
			let to = match self.state_of(id) {
				Some(CallState::Active) => CallState::Interrupted,
				Some(
					CallState::Preparing
					| CallState::OutgoingRinging
					| CallState::IncomingRinging
					| CallState::Connecting,
				) => CallState::Failed,
				_ => continue,
			};
			self.transition(id, to, Some(EndReason::LinkLost), now);
		}
	}

	fn on_accepted(&mut self, call: CallId, session: SessionId, now: MonoTime) -> OpResult {
		let id = self.checked_session(call, session)?;
		match self.state_of(id) {
			Some(CallState::OutgoingRinging) => {
				self.transition(id, CallState::Connecting, None, now);
				self.push_effect(Effect::PrepareMedia { call: id, session });
				Ok(Reply::Done)
			}
			Some(CallState::Connecting | CallState::Active) => Ok(Reply::Done),
			Some(state) => Err(Reject::InvalidState { state }),
			None => Err(Reject::UnknownCall(call)),
		}
	}

	fn on_remote_end(&mut self, call: CallId, session: SessionId, now: MonoTime) -> OpResult {
		let id = self.checked_session(call, session)?;
		match self.state_of(id) {
			Some(CallState::OutgoingRinging) => {
				self.transition(
					id,
					CallState::Declined,
					Some(EndReason::RemoteDeclined),
					now,
				);
			}
			Some(CallState::IncomingRinging) => {
				self.transition(id, CallState::Ended, Some(EndReason::RemoteHangup), now);
			}
			Some(CallState::Connecting | CallState::Active | CallState::Interrupted) => {
				self.transition(id, CallState::Ending, Some(EndReason::RemoteHangup), now);
			}
			_ => {}
		}
		Ok(Reply::Done)
	}

	fn on_invite(
		&mut self,
		call: CallId,
		session: SessionId,
		peer: PeerId,
		parallel: bool,
		now: MonoTime,
	) -> OpResult {
		// A duplicate invitation maps to its retained outcome and never rings twice.
		if self.calls.contains_key(&call) || self.aliases.contains_key(&call) {
			return Ok(Reply::Done);
		}
		self.link_up(peer, now);
		if !self.attached {
			// Nobody is home: answer as busy, ring nothing, open nothing.
			let message = PeerMessage::Decline {
				call,
				session,
				busy: true,
			};
			self.push_effect(Effect::Peer { peer, message });
			return Err(Reject::NotAttached);
		}
		if !parallel && let Some(local) = self.glare_candidate(peer) {
			return Ok(self.resolve_glare(local, call, session, peer, now));
		}
		if self.live_calls() >= self.limits.active_calls {
			let message = PeerMessage::Decline {
				call,
				session,
				busy: true,
			};
			self.push_effect(Effect::Peer { peer, message });
			return Err(Reject::CallCapReached);
		}
		self.create_incoming(call, session, peer, parallel, now);
		Ok(Reply::Done)
	}

	/// Our own not-yet-answered, non-parallel outgoing call to the same peer.
	fn glare_candidate(&self, peer: PeerId) -> Option<CallId> {
		self.calls
			.values()
			.find(|c| {
				c.peer == peer
					&& c.direction == Direction::Outgoing
					&& !c.allow_parallel
					&& matches!(c.state, CallState::Preparing | CallState::OutgoingRinging)
			})
			.map(|c| c.id)
	}

	/// Elects one surviving invitation. The loser becomes an alias of the
	/// survivor. Neither side auto-accepts: the user can still decline.
	fn resolve_glare(
		&mut self,
		local: CallId,
		remote: CallId,
		session: SessionId,
		peer: PeerId,
		now: MonoTime,
	) -> Reply {
		let remote_wins = tie_key(&peer, remote) < tie_key(&self.local, local);
		if !remote_wins {
			self.remember_alias(remote, local);
			return Reply::Done;
		}
		let controller = self.calls.get(&local).and_then(|c| c.controller);
		let local_state = self.state_of(local);
		self.create_incoming(remote, session, peer, false, now);
		if let Some(incoming) = self.calls.get_mut(&remote) {
			incoming.controller = controller;
		}
		self.remember_alias(local, remote);
		let to = match local_state {
			Some(CallState::OutgoingRinging) => CallState::Cancelled,
			_ => CallState::Ended,
		};
		self.transition(local, to, Some(EndReason::GlareLost), now);
		Reply::Done
	}

	fn create_incoming(
		&mut self,
		call: CallId,
		session: SessionId,
		peer: PeerId,
		parallel: bool,
		now: MonoTime,
	) {
		let mut entry = Call::new(
			call,
			session,
			peer,
			Direction::Incoming,
			parallel,
			CallState::IncomingRinging,
		);
		entry.deadline = Some(Deadline {
			at: now.saturating_add(self.limits.ring_timeout),
			phase: Phase::Ring,
		});
		self.calls.insert(call, entry);
		self.mark_dirty(call);
	}

	fn remember_alias(&mut self, loser: CallId, survivor: CallId) {
		self.aliases.insert(loser, survivor);
		self.alias_order.push_back(loser);
		while self.alias_order.len() > self.limits.glare_aliases {
			if let Some(old) = self.alias_order.pop_front() {
				self.aliases.remove(&old);
			}
		}
	}
}
