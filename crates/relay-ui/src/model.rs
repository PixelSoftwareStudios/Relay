//! View model sent to the webview. Plain data, camelCase JSON, hex ids.

use serde::{Deserialize, Serialize};

/// Upper bound for each collection; the engine's real limits are far lower.
pub const MAX_ITEMS: usize = 256;
/// Warnings kept for display.
pub const MAX_WARNINGS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Link {
	/// The app is starting the daemon itself.
	Starting,
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
	/// Hex SHA-256 of the avatar image; empty when the peer has none.
	pub avatar_hash: String,
	/// Short key digest (`A1B2-C3D4-E5F6-0718-293A`): a hint to tell devices apart, never a
	/// verification string. Empty when the engine sent none.
	pub fingerprint_short: String,
	/// Hex ids of channels with proven shared membership.
	pub shared_channels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transmit {
	pub mode: Mode,
	pub consent_granted: bool,
	pub effective_transmitting: bool,
	pub blocked_reason: String,
	/// The key is already up; the gate stays open for the release delay.
	pub releasing: bool,
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
	/// Pinned microphone id; empty when the system default is followed.
	pub input_device_id: String,
	/// Pinned playback device id; empty when the system default is followed.
	pub output_device_id: String,
	/// No microphone is pinned.
	pub input_is_default: bool,
	/// No playback device is pinned.
	pub output_is_default: bool,
	/// The microphone the engine opens (empty when the system has none).
	pub input_device_name: String,
	/// The playback device the engine opens (empty when the system has none).
	pub output_device_name: String,
	/// The pinned microphone is absent; the system default is used instead.
	pub input_pinned_missing: bool,
	/// The pinned playback device is absent; the system default is used instead.
	pub output_pinned_missing: bool,
	pub global_muted: bool,
	pub global_deafened: bool,
	pub microphone_gain: f32,
	pub playback_gain: f32,
	/// Push-to-talk release delay in milliseconds (0 to 2000).
	pub ptt_release_delay_ms: u32,
	/// The app starts the daemon by itself when none runs.
	pub autostart_daemon: bool,
	/// Ring for an incoming call while an app is attached.
	pub ringtone_enabled: bool,
	/// Ring volume, 0.0 to 1.0.
	pub ringtone_volume: f32,
	/// Ringback while an outgoing call rings.
	pub ringback_enabled: bool,
	/// Connect and hang-up blips.
	pub call_sounds_enabled: bool,
	/// Finished calls are recorded in the call history.
	pub history_enabled: bool,
	/// Changes whenever the engine adds to or clears the call history.
	pub history_revision: u64,
	pub display_name: String,
	/// Hex SHA-256 of the local avatar; empty when none is set.
	pub avatar_hash: String,
	pub inbound: Inbound,
	pub advert: Advert,
}

/// Which way a past call went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryDirection {
	Incoming,
	Outgoing,
}

/// How a past call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryOutcome {
	Completed,
	Missed,
	Declined,
	/// An outgoing call that never reached the peer.
	Failed,
	Cancelled,
}

/// One past call, as the engine's call history lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
	pub id: u64,
	pub call_id: String,
	pub peer_id: String,
	/// The name the peer had when the call happened.
	pub peer_name: String,
	pub fingerprint_short: String,
	pub direction: HistoryDirection,
	pub outcome: HistoryOutcome,
	pub started_at_ms: u64,
	/// Connected time; zero when the call never connected.
	pub duration_ms: u64,
}

/// Who may ring this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Inbound {
	Anyone,
	SharedChannelOnly,
}

/// Whether this device is advertised on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Advert {
	Visible,
	Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
	pub channel_id: String,
	pub name: String,
	pub listed: bool,
	pub password: bool,
	pub invite_only: bool,
	/// `owner`, `admin` or `member`.
	pub role: String,
	pub shared_peers: u32,
	pub cert_expires_unix: u64,
}

impl Channel {
	/// Owners and admins may invite and revoke.
	pub fn can_admin(&self) -> bool {
		matches!(self.role.as_str(), "owner" | "admin")
	}
}

/// What a remote user needs to reach this device. Public information only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
	pub addresses: Vec<String>,
	pub port: u32,
	pub fingerprint: String,
	pub connection_string: String,
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
	pub channels: Vec<Channel>,
	pub settings: Option<Settings>,
	/// Past calls, newest first. Kept across a lost connection.
	pub history: Vec<HistoryEntry>,
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
			channels: Vec::new(),
			settings: None,
			history: Vec::new(),
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
