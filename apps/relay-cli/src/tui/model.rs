//! UI state: a cached copy of the engine state plus local view state.

use relay_api::WatchItem;
use relay_api::pb::state_event::Event;
use relay_api::pb::{Call, Device, OperationState, Peer, Problem, Snapshot, TelemetrySnapshot};

use relay_client::ptt::PttState;

use super::messages::MessagesState;
use crate::client::{ConnectionState, Update};
use crate::commands::names;
use crate::hotkey::{HotkeyReport, HotkeyUpdate, TargetStatus};

/// Top-level screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
	Peers,
	Calls,
	Devices,
	Settings,
	Messages,
}

impl Tab {
	/// All tabs in display order.
	pub const ALL: [Tab; 5] = [
		Tab::Peers,
		Tab::Calls,
		Tab::Devices,
		Tab::Settings,
		Tab::Messages,
	];

	/// Title shown in the tab bar.
	pub fn title(self) -> &'static str {
		match self {
			Tab::Peers => "Peers",
			Tab::Calls => "Calls",
			Tab::Devices => "Devices",
			Tab::Settings => "Settings",
			Tab::Messages => "Messages",
		}
	}

	/// Position in [`Tab::ALL`].
	pub fn index(self) -> usize {
		Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
	}
}

/// Everything the screens render.
#[derive(Debug)]
pub struct Model {
	pub snapshot: Snapshot,
	pub synced: bool,
	pub connection: ConnectionState,
	/// Honest state of the global push-to-talk hotkey.
	pub hotkey: HotkeyReport,
	/// What the hotkey's push-to-talk driver is doing.
	pub ptt: PttState,
	/// Which call the hotkey controls.
	pub ptt_target: Option<TargetStatus>,
	pub tab: Tab,
	pub selected: [usize; 5],
	/// The Messages tab: sessions, transcripts, compose box.
	pub messages: MessagesState,
	pub telemetry: Option<TelemetrySnapshot>,
	pub status: String,
	/// Call whose push-to-talk is latched on from this terminal.
	pub ptt_latched: Option<Vec<u8>>,
	pub quit: bool,
}

impl Default for Model {
	fn default() -> Self {
		Self::new()
	}
}

impl Model {
	/// An empty model waiting for the first snapshot.
	pub fn new() -> Self {
		Self {
			snapshot: Snapshot::default(),
			synced: false,
			connection: ConnectionState::Lost("connecting".into()),
			hotkey: HotkeyReport::Disabled("not started".into()),
			ptt: PttState::Idle,
			ptt_target: None,
			tab: Tab::Peers,
			selected: [0; 5],
			messages: MessagesState::default(),
			telemetry: None,
			status: String::new(),
			ptt_latched: None,
			quit: false,
		}
	}

	/// Calls that still matter (not ended, failed, declined or cancelled).
	pub fn live_calls(&self) -> Vec<&Call> {
		self.snapshot
			.calls
			.iter()
			.filter(|c| names::call_is_live(c.state))
			.collect()
	}

	/// Display name for a peer id, falling back to a short id.
	pub fn peer_name(&self, peer_id: &[u8]) -> String {
		self.snapshot
			.peers
			.iter()
			.find(|p| p.peer_id == peer_id)
			.map_or_else(|| names::short_id(peer_id), |p| p.display_name.clone())
	}

	/// Number of rows in the current tab's list.
	pub fn row_count(&self, tab: Tab) -> usize {
		match tab {
			Tab::Peers => self.snapshot.peers.len(),
			Tab::Calls => self.live_calls().len(),
			Tab::Devices => self.snapshot.devices.len(),
			Tab::Settings => 5,
			Tab::Messages => 0,
		}
	}

	/// The selected row of a tab, clamped to the list.
	pub fn selection(&self, tab: Tab) -> usize {
		self.selected[tab.index()].min(self.row_count(tab).saturating_sub(1))
	}

	/// Move the selection of the current tab.
	pub fn move_selection(&mut self, delta: isize) {
		let max = self.row_count(self.tab).saturating_sub(1);
		let next = self
			.selection(self.tab)
			.saturating_add_signed(delta)
			.min(max);
		self.selected[self.tab.index()] = next;
	}

	/// The highlighted peer.
	pub fn selected_peer(&self) -> Option<&Peer> {
		self.snapshot.peers.get(self.selection(Tab::Peers))
	}

	/// The call a call-scoped key acts on: the highlighted one on the Calls
	/// tab, otherwise the only live call.
	pub fn target_call(&self) -> Option<&Call> {
		let live = self.live_calls();
		if self.tab == Tab::Calls {
			live.get(self.selection(Tab::Calls)).copied()
		} else if let [only] = live.as_slice() {
			Some(only)
		} else {
			None
		}
	}

	/// The highlighted device.
	pub fn selected_device(&self) -> Option<&Device> {
		self.snapshot.devices.get(self.selection(Tab::Devices))
	}

	/// Apply something from the feeds.
	pub fn apply(&mut self, update: Update) {
		match update {
			Update::Connection(state) => self.connection = state,
			Update::Telemetry(sample) => self.telemetry = Some(sample),
			Update::Messages(update) => self.messages.apply(*update),
			Update::Hotkey(update) => match update {
				HotkeyUpdate::Report(report) => self.hotkey = report,
				HotkeyUpdate::Ptt(state) => self.ptt = state,
				HotkeyUpdate::Target(target) => self.ptt_target = Some(target),
			},
			Update::State(item) => match *item {
				WatchItem::Snapshot(snapshot) => {
					self.snapshot = snapshot;
					self.synced = true;
				}
				WatchItem::Event { revision, event } => {
					self.snapshot.revision = revision;
					if let Some(event) = event.event {
						self.apply_event(event);
					}
				}
				WatchItem::Resync(_) => self.synced = false,
			},
		}
	}

	fn apply_event(&mut self, event: Event) {
		let snap = &mut self.snapshot;
		match event {
			Event::Peer(p) => upsert(&mut snap.peers, p, |a, b| a.peer_id == b.peer_id),
			Event::Call(c) => upsert(&mut snap.calls, c, |a, b| a.call_id == b.call_id),
			Event::Device(d) => upsert(&mut snap.devices, d, |a, b| {
				a.device_id == b.device_id && a.direction == b.direction
			}),
			Event::Channel(c) => upsert(&mut snap.channels, c, |a, b| a.channel_id == b.channel_id),
			Event::Settings(s) => snap.settings = Some(s),
			Event::Removed(r) => match r.kind.as_str() {
				"peer" => snap.peers.retain(|p| p.peer_id != r.id),
				"call" => snap.calls.retain(|c| c.call_id != r.id),
				"channel" => snap.channels.retain(|c| c.channel_id != r.id),
				"device" => snap
					.devices
					.retain(|d| d.device_id.as_bytes() != r.id.as_slice()),
				"warning" => snap
					.warnings
					.retain(|w| w.code.as_bytes() != r.id.as_slice()),
				_ => {}
			},
			Event::Warning(w) => push_warning(&mut snap.warnings, w),
			Event::AutoAnswered(note) => {
				self.status = format!("answered automatically: {}", note.peer_name);
			}
			Event::Operation(op) => {
				if op.state == OperationState::Failed as i32 {
					let problem = op.problem.unwrap_or_default();
					self.status = format!("failed: {}", problem.message);
				}
			}
		}
	}
}

fn upsert<T>(items: &mut Vec<T>, item: T, same: impl Fn(&T, &T) -> bool) {
	match items.iter_mut().find(|existing| same(existing, &item)) {
		Some(slot) => *slot = item,
		None => items.push(item),
	}
}

fn push_warning(warnings: &mut Vec<Problem>, warning: Problem) {
	match warnings.iter_mut().find(|w| w.code == warning.code) {
		Some(slot) => *slot = warning,
		None => warnings.push(warning),
	}
}
