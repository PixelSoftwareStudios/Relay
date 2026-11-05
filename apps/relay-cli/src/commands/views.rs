//! Serializable views of engine state with a text rendering for humans.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::{Call, Device, Peer, Settings, Snapshot};
use serde::Serialize;

use super::names;
use super::table::{self, Layout};

fn short(id: &str) -> String {
	id.chars().take(8).collect()
}

/// One discovered peer.
#[derive(Debug, Serialize)]
pub struct PeerView {
	pub id: String,
	pub name: String,
	pub trust: &'static str,
	pub reachable: bool,
	pub addresses: Vec<String>,
}

impl From<&Peer> for PeerView {
	fn from(p: &Peer) -> Self {
		Self {
			id: names::hex_id(&p.peer_id),
			name: p.display_name.clone(),
			trust: names::trust_state(p.trust),
			reachable: p.reachable,
			addresses: p.local_addresses.clone(),
		}
	}
}

/// All peers.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct PeerList(pub Vec<PeerView>);

impl PeerList {
	pub fn from_snapshot(snapshot: &Snapshot) -> Self {
		Self(snapshot.peers.iter().map(PeerView::from).collect())
	}
}

impl PeerList {
	/// Text table, or the empty-result message.
	pub fn render(&self, layout: Layout) -> String {
		if self.0.is_empty() {
			return "no peers found".into();
		}
		let mut t = table::with_header(layout, &["ID", "NAME", "TRUST", "ONLINE", "ADDRESSES"]);
		for p in &self.0 {
			t.add_row([
				short(&p.id),
				p.name.clone(),
				p.trust.to_owned(),
				if p.reachable { "yes" } else { "no" }.to_owned(),
				p.addresses.join(", "),
			]);
		}
		table::render(t)
	}
}

impl Display for PeerList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

/// One call with its transmit state.
#[derive(Debug, Serialize)]
pub struct CallView {
	pub id: String,
	pub peer_id: String,
	pub peer_name: Option<String>,
	pub state: &'static str,
	pub mode: &'static str,
	pub microphone_consent: bool,
	pub transmitting: bool,
	pub blocked_reason: Option<String>,
	pub muted: bool,
	pub deafened: bool,
	pub receive_gain: f32,
}

impl CallView {
	pub fn new(call: &Call, snapshot: &Snapshot) -> Self {
		let transmit = call.transmit.clone().unwrap_or_default();
		let peer_name = snapshot
			.peers
			.iter()
			.find(|p| p.peer_id == call.peer_id)
			.map(|p| p.display_name.clone());
		Self {
			id: names::hex_id(&call.call_id),
			peer_id: names::hex_id(&call.peer_id),
			peer_name,
			state: names::call_state(call.state),
			mode: names::transmit_mode(transmit.mode),
			microphone_consent: transmit.consent_granted,
			transmitting: transmit.effective_transmitting,
			blocked_reason: Some(transmit.blocked_reason).filter(|r| !r.is_empty()),
			muted: call.user_muted,
			deafened: call.user_deafened,
			receive_gain: call.receive_gain,
		}
	}
}

impl CallView {
	fn row(&self) -> [String; 6] {
		[
			short(&self.id),
			self.peer_name
				.as_deref()
				.unwrap_or(&self.peer_id)
				.to_owned(),
			self.state.to_owned(),
			self.mode.to_owned(),
			if self.transmitting {
				"transmitting"
			} else {
				"closed"
			}
			.to_owned(),
			self.blocked_reason.clone().unwrap_or_default(),
		]
	}
}

fn call_table(calls: &[CallView], layout: Layout) -> String {
	let mut t = table::with_header(layout, &["CALL", "PEER", "STATE", "MODE", "MIC", "BLOCKED"]);
	for call in calls {
		t.add_row(call.row());
	}
	table::render(t)
}

/// One audio device, marked when it is the selected one.
#[derive(Debug, Serialize)]
pub struct DeviceView {
	pub id: String,
	pub name: String,
	pub direction: &'static str,
	pub available: bool,
	pub system_default: bool,
	pub selected: bool,
}

/// All devices.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct DeviceList(pub Vec<DeviceView>);

impl DeviceList {
	pub fn from_snapshot(snapshot: &Snapshot) -> Self {
		let settings = snapshot.settings.clone().unwrap_or_default();
		Self(
			snapshot
				.devices
				.iter()
				.map(|d| device_view(d, &settings))
				.collect(),
		)
	}
}

fn device_view(d: &Device, settings: &Settings) -> DeviceView {
	let selected =
		d.device_id == settings.input_device_id || d.device_id == settings.output_device_id;
	DeviceView {
		id: d.device_id.clone(),
		name: d.name.clone(),
		direction: names::direction(d.direction),
		available: d.available,
		system_default: d.system_default,
		selected,
	}
}

impl DeviceList {
	/// Text table, or the empty-result message.
	pub fn render(&self, layout: Layout) -> String {
		if self.0.is_empty() {
			return "no audio devices reported".into();
		}
		let mut t = table::with_header(layout, &["", "DIRECTION", "NAME", "ID", "NOTES"]);
		for d in &self.0 {
			let mut notes = Vec::new();
			if d.system_default {
				notes.push("default");
			}
			if !d.available {
				notes.push("unavailable");
			}
			t.add_row([
				if d.selected { "*" } else { "" }.to_owned(),
				d.direction.to_owned(),
				d.name.clone(),
				d.id.clone(),
				notes.join(", "),
			]);
		}
		table::render(t)
	}
}

impl Display for DeviceList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

/// Result of a mutation.
#[derive(Debug, Serialize)]
pub struct Outcome {
	pub operation_id: String,
	pub state: &'static str,
	pub revision: u64,
	pub call_id: Option<String>,
}

impl Display for Outcome {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.state)?;
		if let Some(call) = &self.call_id {
			write!(f, " call {}", short(call))?;
		}
		Ok(())
	}
}

/// Engine and call status.
#[derive(Debug, Serialize)]
pub struct StatusView {
	pub engine_version: String,
	pub api: String,
	pub revision: u64,
	pub peers_total: usize,
	pub peers_online: usize,
	pub input_device: String,
	pub output_device: String,
	pub noise_suppression: String,
	pub muted: bool,
	pub deafened: bool,
	pub microphone_gain: f32,
	pub playback_gain: f32,
	pub calls: Vec<CallView>,
	pub warnings: Vec<String>,
}

impl StatusView {
	pub fn new(info: &relay_api::pb::EngineInfo, snapshot: &Snapshot) -> Self {
		let s = snapshot.settings.clone().unwrap_or_default();
		Self {
			engine_version: info.application_version.clone(),
			api: format!("{}.{}", info.api_major, info.api_minor),
			revision: snapshot.revision,
			peers_total: snapshot.peers.len(),
			peers_online: snapshot.peers.iter().filter(|p| p.reachable).count(),
			input_device: s.input_device_id,
			output_device: s.output_device_id,
			noise_suppression: s.ns_backend_id,
			muted: s.global_muted,
			deafened: s.global_deafened,
			microphone_gain: s.microphone_gain,
			playback_gain: s.playback_gain,
			calls: snapshot
				.calls
				.iter()
				.filter(|c| names::call_is_live(c.state))
				.map(|c| CallView::new(c, snapshot))
				.collect(),
			warnings: snapshot
				.warnings
				.iter()
				.map(|w| format!("{}: {}", w.code, w.message))
				.collect(),
		}
	}
}

fn or_dash(s: &str) -> &str {
	if s.is_empty() { "-" } else { s }
}

impl StatusView {
	/// Settings as `label  value` rows, then the live calls table.
	pub fn render(&self, layout: Layout) -> String {
		let audio = format!(
			"mic x{:.2}{} playback x{:.2}{}",
			self.microphone_gain,
			if self.muted { " (muted)" } else { "" },
			self.playback_gain,
			if self.deafened { " (deafened)" } else { "" },
		);
		let mut rows = vec![
			(
				"engine",
				format!("{} (api {})", self.engine_version, self.api),
			),
			(
				"peers",
				format!("{} online of {}", self.peers_online, self.peers_total),
			),
			("input", or_dash(&self.input_device).to_owned()),
			("output", or_dash(&self.output_device).to_owned()),
			("ns", or_dash(&self.noise_suppression).to_owned()),
			("audio", audio),
		];
		if self.calls.is_empty() {
			rows.push(("calls", "none".into()));
		}
		for w in &self.warnings {
			rows.push(("warning", w.clone()));
		}
		let mut text = table::render(table::key_values(layout, &rows));
		if !self.calls.is_empty() {
			text.push_str("\n\n");
			text.push_str(&call_table(&self.calls, layout));
		}
		text
	}
}

impl Display for StatusView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}
