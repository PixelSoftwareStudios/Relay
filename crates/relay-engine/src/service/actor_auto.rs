//! Auto-answer: an incoming call from a verified contact the user opted in is answered by
//! the engine after a short delay.
//!
//! This opens the way to the local microphone for someone else, so every condition is
//! checked twice, when the call starts ringing and again when the delay is over: the
//! global switch is on, the caller is flagged, stored as verified and linked by its pinned
//! key, no other call is live, and a client is attached to own the call. The call then
//! connects exactly like an accepted one: the microphone state is the user's default call
//! mode (`calls.default_transmit_mode`), never forced open. The delay is the user's window
//! to decline; the ring, the notification and the connect chime tell everyone nearby.

use std::collections::BTreeMap;
use std::time::Duration;

use relay_api::pb::{AutoAnswered, StateEvent, state_event};
use relay_config::prefs::CallAlert;
use relay_core::{
	CallId, CallState, Command, Direction, Input, OperationId, PeerId, Reply, Request,
};
use tokio::time::Instant;

use super::actor::Actor;

/// How long a call from an auto-answer contact rings before it is answered.
pub const AUTO_ANSWER_DELAY: Duration = Duration::from_millis(1500);

/// Calls waiting for their answer (`Some(deadline)`) or already tried (`None`).
pub(super) type AutoPending = BTreeMap<CallId, Option<Instant>>;

impl Actor {
	/// Whether an incoming call may ring aloud on this device (`calls.alert`).
	pub(super) fn ring_audible(&self) -> bool {
		!matches!(
			self.directory.aux.call_alert,
			CallAlert::Vibrate | CallAlert::Silent
		)
	}

	/// When the next auto-answer is due.
	pub(super) fn auto_deadline(&self) -> Option<Instant> {
		self.auto_pending.values().flatten().min().copied()
	}

	fn auto_operation(&mut self) -> OperationId {
		self.auto_ops += 1;
		OperationId::from_u128((1 << 127) | u128::from(self.auto_ops))
	}

	/// Schedules, cancels and performs auto-answers to match the calls now.
	pub(super) fn sync_auto_answer(&mut self) {
		let snapshot = self.engine.state().snapshot();
		let ringing: Vec<(CallId, PeerId)> = snapshot
			.calls
			.values()
			.filter(|c| c.direction == Direction::Incoming && c.state == CallState::IncomingRinging)
			.map(|c| (c.id, c.peer))
			.collect();
		// A call that stopped ringing (declined, ended, answered by hand) is no longer ours.
		self.auto_pending
			.retain(|id, _| ringing.iter().any(|(call, _)| call == id));
		let now = Instant::now();
		for (id, peer) in &ringing {
			if !self.auto_pending.contains_key(id) && self.may_auto_answer(*id, *peer) {
				self.auto_pending.insert(*id, Some(now + AUTO_ANSWER_DELAY));
			}
		}
		let due: Vec<CallId> = self
			.auto_pending
			.iter()
			.filter(|(_, at)| at.is_some_and(|at| at <= now))
			.map(|(id, _)| *id)
			.collect();
		for id in due {
			self.auto_pending.insert(id, None);
			if let Some((_, peer)) = ringing.iter().find(|(call, _)| *call == id)
				&& self.may_auto_answer(id, *peer)
			{
				self.auto_answer(id, *peer);
			}
		}
	}

	/// All the conditions for answering `call` from `peer` by itself.
	fn may_auto_answer(&self, call: CallId, peer: PeerId) -> bool {
		let others_live = self
			.engine
			.state()
			.snapshot()
			.calls
			.values()
			.any(|c| c.id != call && !c.state.is_terminal());
		self.directory.aux.auto_answer_enabled
			&& self.directory.may_auto_answer(peer)
			&& !others_live
			&& !self.reported.is_empty()
	}

	fn auto_answer(&mut self, call: CallId, peer: PeerId) {
		let Some(owner) = self.reported.iter().next().copied() else {
			return;
		};
		// Marked first so the call's own events already carry the indicator.
		self.directory.note_auto_answered(call);
		let operation = self.auto_operation();
		let lease = self.run_input(Input::Client(Request {
			client: owner,
			operation,
			command: Command::AcquireController {
				call,
				takeover: false,
			},
		}));
		let lease = match lease.map(|o| o.result) {
			Ok(Ok(Reply::Controller(lease))) => lease,
			_ => return self.directory.forget_call(call),
		};
		let operation = self.auto_operation();
		let accepted = self.run_input(Input::Client(Request {
			client: owner,
			operation,
			command: Command::AcceptCall { call, lease },
		}));
		if !matches!(accepted.map(|o| o.result), Ok(Ok(_))) {
			return self.directory.forget_call(call);
		}
		self.tones.force_chime(call);
		tracing::info!(%call, %peer, "incoming call answered automatically");
		let event = AutoAnswered {
			call_id: call.to_bytes().to_vec(),
			peer_id: peer.as_bytes().to_vec(),
			peer_name: self.directory.peer_name(peer).unwrap_or_default(),
		};
		self.publish(vec![StateEvent {
			event: Some(state_event::Event::AutoAnswered(event)),
		}]);
	}
}
