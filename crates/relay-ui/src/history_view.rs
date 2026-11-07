//! The call history as the Calls page shows it: one row per past call with its marker word,
//! time, duration and whether it can be called back now. Window-less, so it tests
//! without drawing.

use relay_io::localtime::LocalZone;

use crate::avatar::Avatar;
use crate::history_time::{self, Relative};
use crate::icons::Icon;
use crate::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome};
use crate::tr;
use crate::viewmodel::peer_rows;

/// Rows shown before "Show more", and added by each press of it.
pub const PAGE: usize = 20;

/// Why a row cannot be called back right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
	/// The device is on the network but may not be called (key changed, blocked, in a call).
	Cannot,
	/// The device is not reachable, or no longer known.
	Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
	pub id: u64,
	/// Hex call id; the key of the call's messaging session.
	pub call_id: String,
	pub peer_id: String,
	pub name: String,
	/// The picture of the device, when it is known; otherwise the row shows an initial.
	pub avatar: Option<Avatar>,
	/// The marker word: Incoming, Outgoing, Missed, Declined, Not reached or Cancelled.
	pub marker: &'static str,
	/// A call the user did not answer; drawn with emphasis.
	pub missed: bool,
	/// The call never connected (missed, declined, not reached, cancelled): drawn in red.
	pub unanswered: bool,
	/// Incoming, outgoing or unanswered, like the phone's own call log.
	pub icon: Icon,
	pub when: Relative,
	/// The full local date and time with the UTC clock, for hover text.
	pub when_hover: String,
	/// `m:ss` / `h:mm:ss`, only for a call that connected.
	pub duration: Option<String>,
	/// `None` when Call back works.
	pub blocked: Option<Blocked>,
}

/// The marker word of an entry, in the active language.
pub fn marker(entry: &HistoryEntry) -> &'static str {
	match (entry.outcome, entry.direction) {
		(HistoryOutcome::Completed, HistoryDirection::Incoming) => tr!("history-incoming"),
		(HistoryOutcome::Completed, HistoryDirection::Outgoing) => tr!("history-outgoing"),
		(HistoryOutcome::Missed, _) => tr!("history-missed"),
		(HistoryOutcome::Declined, _) => tr!("history-declined"),
		(HistoryOutcome::Failed, _) => tr!("history-not-reached"),
		(HistoryOutcome::Cancelled, _) => tr!("history-cancelled"),
	}
}

/// The call-log icon of an entry: an arrow in for incoming, out for outgoing, and a crossed
/// phone for anything that never connected.
pub fn icon(entry: &HistoryEntry) -> Icon {
	match (entry.outcome, entry.direction) {
		(HistoryOutcome::Completed, HistoryDirection::Incoming) => Icon::CallIncoming,
		(HistoryOutcome::Completed, HistoryDirection::Outgoing) => Icon::CallOutgoing,
		_ => Icon::CallMissed,
	}
}

/// The first `shown` rows of the history, newest first, relative to `now_ms` in `zone`.
pub fn rows(state: &AppState, shown: usize, now_ms: u64, zone: &LocalZone) -> Vec<HistoryRow> {
	let peers = peer_rows(state);
	state
		.history
		.iter()
		.take(shown)
		.map(|e| {
			let peer = peers.iter().find(|p| p.peer_id == e.peer_id);
			HistoryRow {
				id: e.id,
				call_id: e.call_id.clone(),
				peer_id: e.peer_id.clone(),
				name: e.peer_name.clone(),
				avatar: peer.map(|p| p.avatar.clone()),
				marker: marker(e),
				missed: e.outcome == HistoryOutcome::Missed,
				unanswered: e.outcome != HistoryOutcome::Completed,
				icon: icon(e),
				when: history_time::relative(now_ms, e.started_at_ms, zone),
				when_hover: zone.hover(e.started_at_ms),
				duration: (e.outcome == HistoryOutcome::Completed)
					.then(|| history_time::duration(e.duration_ms)),
				blocked: match peer {
					Some(p) if p.can_call => None,
					Some(p) if p.reachable => Some(Blocked::Cannot),
					_ => Some(Blocked::Offline),
				},
			}
		})
		.collect()
}

/// Whether older entries remain beyond the first `shown`.
pub fn has_more(state: &AppState, shown: usize) -> bool {
	state.history.len() > shown
}
