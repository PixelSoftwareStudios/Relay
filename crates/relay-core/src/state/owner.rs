//! Who owns a call, and what happens when the owner is gone.
//!
//! A call belongs to the client holding its controller lease. A client is
//! *attached* while it holds an open state watch (the host reports that with
//! `SystemEvent::Watching`). A live call whose owner is not attached is
//! orphaned: after the grace period it is ended like a normal hangup (the peer
//! is told, media stops, capture closes, the route resets). Re-attaching, or
//! another attached client taking the lease over, within the grace keeps it.
//! A ringing incoming call has no owner until someone takes control; it is
//! declined once nobody at all is attached.
//!
//! None of this applies until the host enables presence tracking.

use super::State;
use crate::call::{Call, CallState, EndReason};
use crate::effect::{Effect, PeerMessage};
use crate::id::{CallId, ClientId};
use crate::time::MonoTime;

impl State {
	/// Whether `client` holds an open watch (always, while untracked).
	pub(crate) fn is_attached(&self, client: ClientId) -> bool {
		self.presence
			.as_ref()
			.is_none_or(|attached| attached.contains(&client))
	}

	/// Whether any client holds an open watch (always, while untracked).
	pub(crate) fn anyone_attached(&self) -> bool {
		self.presence
			.as_ref()
			.is_none_or(|attached| !attached.is_empty())
	}

	fn orphaned(&self, call: &Call) -> bool {
		if self.presence.is_none() {
			return false;
		}
		match call.state {
			CallState::Ending => false,
			state if state.is_terminal() => false,
			CallState::IncomingRinging => call.controller.map_or_else(
				|| !self.anyone_attached(),
				|lease| !self.is_attached(lease.holder),
			),
			_ => call
				.controller
				.is_none_or(|lease| !self.is_attached(lease.holder)),
		}
	}

	/// Starts or cancels each live call's grace timer from the facts now.
	pub(super) fn reconcile_owners(&mut self, now: MonoTime) {
		let grace = self.limits.owner_grace;
		let verdicts: Vec<(CallId, bool)> = self
			.calls
			.values()
			.map(|call| (call.id, self.orphaned(call)))
			.collect();
		for (id, orphaned) in verdicts {
			let Some(call) = self.calls.get_mut(&id) else {
				continue;
			};
			match (orphaned, call.orphan_at) {
				(true, None) => call.orphan_at = Some(now.saturating_add(grace)),
				(false, Some(_)) => call.orphan_at = None,
				_ => {}
			}
		}
	}

	/// Ends every call whose grace ran out and is still orphaned.
	pub(super) fn end_orphans(&mut self, now: MonoTime) {
		let due: Vec<CallId> = self
			.calls
			.values()
			.filter(|call| call.orphan_at.is_some_and(|at| at <= now))
			.map(|call| call.id)
			.collect();
		for id in due {
			let Some(call) = self.calls.get_mut(&id) else {
				continue;
			};
			call.orphan_at = None;
			if self.calls.get(&id).is_some_and(|call| self.orphaned(call)) {
				self.hang_up(id, now);
			}
		}
	}

	/// A normal local hangup, without needing the owner's lease.
	fn hang_up(&mut self, id: CallId, now: MonoTime) {
		let Some(call) = self.calls.get(&id) else {
			return;
		};
		let (peer, session, state) = (call.peer, call.session, call.state);
		let end = PeerMessage::End { call: id, session };
		match state {
			CallState::Preparing => {
				self.transition(id, CallState::Ended, Some(EndReason::OwnerGone), now);
			}
			CallState::OutgoingRinging => {
				self.transition(id, CallState::Cancelled, Some(EndReason::OwnerGone), now);
				self.push_effect(Effect::Peer { peer, message: end });
			}
			CallState::IncomingRinging => {
				self.transition(id, CallState::Declined, Some(EndReason::OwnerGone), now);
				let message = PeerMessage::Decline {
					call: id,
					session,
					busy: true,
				};
				self.push_effect(Effect::Peer { peer, message });
			}
			CallState::Connecting | CallState::Active | CallState::Interrupted => {
				self.transition(id, CallState::Ending, Some(EndReason::OwnerGone), now);
				self.push_effect(Effect::Peer { peer, message: end });
			}
			_ => {}
		}
	}
}
