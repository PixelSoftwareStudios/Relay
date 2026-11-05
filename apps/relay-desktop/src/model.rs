//! View model sent to the webview. Plain data, camelCase JSON, hex ids.

use serde::{Deserialize, Serialize};

/// Upper bound for each collection; the engine's real limits are far lower.
pub const MAX_ITEMS: usize = 256;
/// Warnings kept for display.
pub const MAX_WARNINGS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Link {
	Connecting,
	Connected,
	Reconnecting,
	Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Trust {
	Unknown,
	TofuUnverified,
	Verified,
	KeyChanged,
	Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
	Preparing,
	OutgoingRinging,
	IncomingRinging,
	Negotiating,
	Connected,
	Interrupted,
	Ending,
	Ended,
	Failed,
	Declined,
	Cancelled,
}

impl Phase {
	/// Whether the call is over and only kept for display.
	pub fn is_terminal(self) -> bool {
		matches!(
			self,
			Self::Ended | Self::Failed | Self::Declined | Self::Cancelled
		)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
	ReceiveOnly,
	PushToTalk,
	OpenMic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
	pub code: String,
	pub message: String,
	pub retryable: bool,
	pub recovery_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
	pub peer_id: String,
	pub display_name: String,
	pub addresses: Vec<String>,
	pub trust: Trust,
	pub reachable: bool,
	pub has_avatar: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transmit {
	pub mode: Mode,
	pub consent_granted: bool,
	pub effective_transmitting: bool,
	pub blocked_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
	pub call_id: String,
	pub peer_id: String,
	pub phase: Phase,
	pub transmit: Transmit,
	pub user_muted: bool,
	pub user_deafened: bool,
	pub receive_gain: f32,
	pub profile: String,
	pub problem: Option<Problem>,
	pub controlled_by_me: bool,
	/// Lease generation needed for call mutations; not shown in the UI.
	#[serde(skip)]
	pub controller_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
	Input,
	Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
	pub device_id: String,
	pub name: String,
	pub direction: Direction,
	pub available: bool,
	pub system_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
	pub input_device_id: String,
	pub output_device_id: String,
	pub global_muted: bool,
	pub global_deafened: bool,
	pub microphone_gain: f32,
	pub playback_gain: f32,
	/// Whether the local profile has an avatar (the engine reports a hash).
	pub has_avatar: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
	pub version: String,
	pub api_major: u32,
	pub api_minor: u32,
}

/// Structural state; small enough to resend whole on every change.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
	pub seq: u64,
	pub link: Link,
	pub mock: bool,
	pub engine: Option<EngineInfo>,
	pub revision: u64,
	pub peers: Vec<Peer>,
	pub calls: Vec<Call>,
	pub devices: Vec<Device>,
	pub settings: Option<Settings>,
	pub warnings: Vec<Problem>,
	pub link_error: Option<String>,
	pub notice: Option<String>,
	/// This client's id on the engine; used to tell who holds a call.
	#[serde(skip)]
	pub client_id: Vec<u8>,
}

impl Default for AppState {
	fn default() -> Self {
		Self {
			seq: 0,
			link: Link::Connecting,
			mock: false,
			engine: None,
			revision: 0,
			peers: Vec::new(),
			calls: Vec::new(),
			devices: Vec::new(),
			settings: None,
			warnings: Vec::new(),
			link_error: None,
			notice: None,
			client_id: Vec::new(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallMeter {
	pub call_id: String,
	pub speaking: bool,
	pub receive_dbfs: Option<f32>,
	pub rtt_ms: Option<f64>,
}

/// High-rate readings, emitted separately from structural state.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meters {
	pub seq: u64,
	pub microphone_dbfs: Option<f32>,
	pub calls: Vec<CallMeter>,
}
