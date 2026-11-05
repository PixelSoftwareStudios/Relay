//! Window-less view model: everything a frame needs, computed from state.
//! Drawing code only reads these structs, so the logic is testable headless.

use crate::format::{TransmitStatus, describe_transmit, phase_label};
use crate::model::{AppState, Call, CallMeter, Meters, Mode, Peer, Phase, Trust};

#[derive(Debug, Clone, PartialEq)]
pub struct CallCard {
	pub call_id: String,
	pub peer_id: String,
	pub name: String,
	pub phase: Phase,
	pub subtitle: String,
	pub incoming: bool,
	/// Controls such as mute and transmit mode are shown for these.
	pub controllable: bool,
	pub speaking: bool,
	pub receive_dbfs: Option<f32>,
	pub transmit: TransmitStatus,
	pub call: Call,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerRow {
	pub peer_id: String,
	pub name: String,
	pub subtitle: String,
	pub can_call: bool,
}

pub fn trust_label(trust: Trust) -> &'static str {
	match trust {
		Trust::Unknown => "Unknown",
		Trust::TofuUnverified => "Seen before, not verified",
		Trust::Verified => "Verified",
		Trust::KeyChanged => "Key changed: do not trust",
		Trust::Blocked => "Blocked",
	}
}

fn meter_for<'a>(meters: &'a Meters, call_id: &str) -> Option<&'a CallMeter> {
	meters.calls.iter().find(|m| m.call_id == call_id)
}

pub fn call_cards(state: &AppState, meters: &Meters) -> Vec<CallCard> {
	state
		.calls
		.iter()
		.filter(|c| !c.phase.is_terminal())
		.map(|call| {
			let name = state
				.peers
				.iter()
				.find(|p| p.peer_id == call.peer_id)
				.map_or_else(|| "Unknown peer".to_owned(), |p| p.display_name.clone());
			let meter = meter_for(meters, &call.call_id);
			let connected = call.phase == Phase::Connected;
			let speaking = connected && meter.is_some_and(|m| m.speaking);
			let mut subtitle = phase_label(call.phase).to_owned();
			if speaking {
				subtitle.push_str(" · speaking");
			}
			if !call.profile.is_empty() {
				subtitle.push_str(&format!(" · {}", call.profile));
			}
			CallCard {
				call_id: call.call_id.clone(),
				peer_id: call.peer_id.clone(),
				name,
				phase: call.phase,
				subtitle,
				incoming: call.phase == Phase::IncomingRinging,
				controllable: call.phase != Phase::IncomingRinging,
				speaking,
				receive_dbfs: if connected {
					meter.and_then(|m| m.receive_dbfs)
				} else {
					None
				},
				transmit: describe_transmit(call),
				call: call.clone(),
			}
		})
		.collect()
}

fn callable(state: &AppState, peer: &Peer) -> bool {
	peer.reachable
		&& !matches!(peer.trust, Trust::Blocked | Trust::KeyChanged)
		&& !state
			.calls
			.iter()
			.any(|c| c.peer_id == peer.peer_id && !c.phase.is_terminal())
}

pub fn peer_rows(state: &AppState) -> Vec<PeerRow> {
	state
		.peers
		.iter()
		.map(|p| PeerRow {
			peer_id: p.peer_id.clone(),
			name: p.display_name.clone(),
			subtitle: format!(
				"{} · {}",
				if p.reachable {
					"Online"
				} else {
					"Not reachable"
				},
				trust_label(p.trust)
			),
			can_call: callable(state, p),
		})
		.collect()
}

/// Mode buttons: label, mode, and whether it needs consent first.
pub const MODES: [(&str, Mode); 3] = [
	("Listen only", Mode::ReceiveOnly),
	("Push-to-talk", Mode::PushToTalk),
	("Open mic", Mode::OpenMic),
];

/// A mode button is enabled when it needs no consent or consent is ticked.
pub fn mode_enabled(mode: Mode, consent_checked: bool) -> bool {
	mode == Mode::ReceiveOnly || consent_checked
}
