//! Turns call lifecycle events into call history entries.
//!
//! The actor sees every `CallUpdated` event exactly once, in order, so a call's last
//! event is the single place where it ends. The recorder follows each call from its first
//! event and, at the terminal one, says what to write. Only calls the user could see are
//! recorded: an outgoing call the user started, or an incoming call that rang. Anything the
//! daemon turned away before it reached the domain never produces an event at all.

use std::collections::HashMap;
use std::sync::Arc;

use relay_config::history::{CallOutcome, Direction as Way, NewCallRecord};
use relay_core::{CallId, CallState, CallView, Direction, EndReason, MonoTime, PeerId};

use super::directory::Directory;

/// Wall clock in unix milliseconds; injectable for tests.
pub type WallClock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The system clock.
pub fn system_clock() -> WallClock {
	Arc::new(|| {
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
	})
}

#[derive(Debug)]
struct Note {
	peer: PeerId,
	way: Direction,
	started_ms: u64,
	name: String,
	/// An incoming call rang for the user (or an outgoing one was theirs).
	seen: bool,
	/// The local user accepted an incoming call.
	accepted: bool,
	connected: Option<MonoTime>,
}

/// Follows calls from their first event to their last.
pub(super) struct Recorder {
	clock: WallClock,
	notes: HashMap<CallId, Note>,
}

impl Recorder {
	pub(super) fn new(clock: WallClock) -> Self {
		Self {
			clock,
			notes: HashMap::new(),
		}
	}

	/// The wall clock, for pruning.
	pub(super) fn now_ms(&self) -> u64 {
		(self.clock)()
	}

	/// Folds one call event; returns the entry to store when the call just ended.
	pub(super) fn observe(
		&mut self,
		view: &CallView,
		directory: &Directory,
		now: MonoTime,
	) -> Option<NewCallRecord> {
		if !view.state.is_terminal() {
			let started = (self.clock)();
			let note = self.notes.entry(view.id).or_insert_with(|| Note {
				peer: view.peer,
				way: view.direction,
				started_ms: started,
				name: String::new(),
				seen: view.direction == Direction::Outgoing,
				accepted: false,
				connected: None,
			});
			if let Some(name) = directory.peer_name(view.peer) {
				note.name = name;
			}
			match view.state {
				CallState::IncomingRinging => note.seen = true,
				CallState::Connecting => note.accepted = true,
				CallState::Active => {
					note.accepted = true;
					note.connected.get_or_insert(now);
				}
				_ => {}
			}
			return None;
		}
		let mut note = self.notes.remove(&view.id)?;
		if !note.seen || view.reason == Some(EndReason::GlareLost) {
			return None;
		}
		if let Some(name) = directory.peer_name(note.peer) {
			note.name = name;
		}
		let (outcome, duration_ms) = match note.connected {
			Some(at) => (
				CallOutcome::Completed,
				u64::try_from(now.saturating_since(at).as_millis()).unwrap_or(u64::MAX),
			),
			None => (classify(&note, view.state, view.reason), 0),
		};
		let fingerprint = relay_net::identity::Fingerprint::from_bytes(*note.peer.as_bytes());
		let short = fingerprint.short();
		Some(NewCallRecord {
			peer_name: if note.name.is_empty() {
				short.clone()
			} else {
				note.name
			},
			peer_fingerprint: hex::encode(note.peer.as_bytes()),
			peer_fingerprint_short: short,
			direction: match note.way {
				Direction::Incoming => Way::Incoming,
				Direction::Outgoing => Way::Outgoing,
			},
			outcome,
			started_at_ms: note.started_ms,
			duration_ms,
			call_id: view.id.to_bytes(),
		})
	}
}

/// The outcome of a call that never connected.
fn classify(note: &Note, end: CallState, reason: Option<EndReason>) -> CallOutcome {
	match (note.way, end) {
		// The window that was ringing went away before anyone answered.
		(Direction::Incoming, CallState::Declined) if reason == Some(EndReason::OwnerGone) => {
			CallOutcome::Missed
		}
		(_, CallState::Declined) => CallOutcome::Declined,
		(Direction::Outgoing, CallState::Cancelled | CallState::Ended) => CallOutcome::Cancelled,
		(Direction::Outgoing, _) => CallOutcome::Failed,
		(Direction::Incoming, _) if note.accepted => CallOutcome::Failed,
		(Direction::Incoming, _) => CallOutcome::Missed,
	}
}
