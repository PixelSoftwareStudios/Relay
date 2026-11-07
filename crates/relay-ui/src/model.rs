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
	Muted,
	PushToTalk,
	Unmuted,
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
	/// Peer listener port of its first address; zero when unknown.
	pub port: u32,
	/// Last time the engine saw it by discovery or link (unix ms); zero when never.
	pub last_seen_unix_ms: u64,
	/// Hex ids of channels with proven shared membership.
	pub shared_channels: Vec<String>,
	/// On the Calls page's favorites grid (at most 6 trusted peers).
	pub favorite: bool,
	/// 0-based place among the favorites; zero when not a favorite.
	pub favorite_position: u32,
	/// The device presented a key other than the pinned one and was refused. `peer_id` and
	/// `fingerprint_short` are then those of the new key.
	pub key_changed: bool,
	/// Hex digest of the pinned key; empty unless `key_changed`.
	pub previous_peer_id: String,
	/// Short form of the pinned key; empty unless `key_changed`.
	pub previous_fingerprint_short: String,
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
	/// Engine wall clock (unix ms) when the call became connected; zero while it is not. Show
	/// the duration as now minus this, so both ends and every client agree.
	pub connected_at_unix_ms: u64,
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
	/// File name of the custom ringtone on this device; empty for the built-in chime.
	pub ringtone_name: String,
	/// Finished calls are recorded in the call history.
	pub history_enabled: bool,
	/// Changes whenever the engine adds to or clears the call history.
	pub history_revision: u64,
	/// The mode a call starts in when it connects (`calls.default_transmit_mode`).
	pub default_transmit_mode: Mode,
	/// Git commit of the running daemon (with `-dirty` or `-dev` markers).
	pub daemon_build_revision: String,
	/// API revision of the running daemon.
	pub daemon_api_revision: u32,
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
	/// Build of the daemon (git commit, `-dirty`, or `<version>-dev`).
	pub build_revision: String,
	/// API revision of the daemon.
	pub api_revision: u32,
}

/// A background service that is running but has not opened the system's secure storage:
/// it waits for the user's permission, or that failed and it waits for a choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
	/// The attempt failed (denied, locked, unreadable); false while it is still waiting.
	pub failed: bool,
	/// What is stored cannot be read at all (not a matter of permission).
	pub unreadable: bool,
	/// The system's own text, for details.
	pub detail: String,
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
	/// Set while the background service answers but is not ready (see [`Startup`]).
	pub startup: Option<Startup>,
	pub notice: Option<String>,
	/// This client's id on the engine; used to tell who holds a call.
	#[serde(skip)]
	pub client_id: Vec<u8>,
}

impl AppState {
	/// The running daemon speaks another API revision than this app: it is a stale background
	/// service and should be restarted. Only the API revision decides (the build text may differ
	/// between binaries of one tree). False until settings arrive.
	pub fn daemon_out_of_date(&self) -> bool {
		self.settings
			.as_ref()
			.is_some_and(|s| s.daemon_api_revision != relay_api::API_REVISION)
	}
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
			startup: None,
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

/// Where the UDP media path of a call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaPath {
	/// Waiting for the other device to answer the path challenge.
	Waiting,
	Connected,
	/// Gave up; `CallStats::path_reason` says why.
	Failed,
}

/// Live measurements of one call, about once a second, for a debug panel. All latency figures
/// are estimates (`docs/engine.md`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallStats {
	pub call_id: String,
	pub path: MediaPath,
	/// Why the path waits or failed; empty when connected.
	pub path_reason: String,
	/// A packet was sent in the last 500 ms.
	pub sending: bool,
	/// A packet was received in the last 500 ms.
	pub receiving: bool,
	pub packets_sent: u64,
	pub packets_received: u64,
	pub mic_level_dbfs: Option<f32>,
	pub rtt_ms: Option<f64>,
	pub rtt_min_ms: Option<f64>,
	pub rtt_max_ms: Option<f64>,
	pub jitter_ms: Option<f64>,
	pub loss_percent_5s: f64,
	pub loss_percent_total: f64,
	pub jitter_buffer_target_ms: f64,
	pub jitter_buffer_depth_ms: f64,
	pub playout_delay_ms: f64,
	pub mouth_to_ear_ms: Option<f64>,
	pub codec_bitrate_bps: u32,
	pub packet_bytes: u32,
	pub local_addr: String,
	pub remote_addr: String,
	/// `lan` or `overlay`.
	pub transport: String,
	pub clock_drift_ppm: Option<f64>,
	pub capture_buffer_frames: u32,
	pub capture_buffer_ms: f64,
	pub playback_buffer_frames: u32,
	pub playback_buffer_ms: f64,
}
