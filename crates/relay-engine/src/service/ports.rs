//! Adapter boundary of the control actor.
//!
//! The actor owns the domain state and never performs I/O. Everything it needs
//! from the outside world goes through these traits. All methods are
//! synchronous and must not block: implementations hand the work to their own
//! thread or task and report completion back through a `ControlHandle`
//! (`system`, `peer`, `peers_changed`, `devices_changed`).

use std::sync::Arc;

use relay_core::{
	AudioGeneration, CallId, DeviceKeyId, PeerId, PeerMessage, ReceiveMix, SessionId,
};
use thiserror::Error;

/// Why an adapter refused or could not perform a request.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortError {
	/// No adapter is wired for this capability in this build.
	#[error("not supported: {0}")]
	Unsupported(&'static str),
	/// The request was well formed but not acceptable.
	#[error("invalid: {0}")]
	Invalid(String),
	/// The named object does not exist.
	#[error("not found: {0}")]
	NotFound(String),
	/// The adapter is temporarily unable to act.
	#[error("unavailable: {0}")]
	Unavailable(String),
}

/// How much a peer's key is trusted, as shown to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTrust {
	/// Never seen before.
	Unknown,
	/// Pinned on first use and not compared out of band.
	TofuUnverified,
	/// Compared out of band.
	Verified,
	/// The peer presented a different key than the pinned one.
	KeyChanged,
	/// Explicitly blocked.
	Blocked,
}

/// A discovered or known peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
	/// Authenticated device key digest.
	pub id: PeerId,
	/// Sanitized display name announced by the peer.
	pub display_name: String,
	/// Addresses the peer was seen on, as `ip:port` text.
	pub addresses: Vec<String>,
	/// Trust label.
	pub trust: PeerTrust,
	/// Whether the peer is currently reachable.
	pub reachable: bool,
	/// Content hash of the peer's cached avatar, if any.
	pub avatar: Option<[u8; 32]>,
}

/// Direction of an audio device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
	/// A microphone.
	Input,
	/// A speaker or headset.
	Output,
}

/// An audio device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
	/// Opaque adapter-qualified identifier.
	pub id: String,
	/// Human readable name.
	pub name: String,
	/// Input or output.
	pub direction: DeviceDirection,
	/// Whether it can be opened right now.
	pub available: bool,
	/// Whether the OS currently names it the default.
	pub system_default: bool,
	/// Sample rates the device is expected to support.
	pub rates: Vec<u32>,
}

/// A user decision about a peer's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRequest {
	/// The peer.
	pub peer: PeerId,
	/// Digest the client believes it is deciding about; must match the peer.
	pub expected_key: DeviceKeyId,
	/// The requested label.
	pub requested: PeerTrust,
	/// The user compared the safety code out of band.
	pub out_of_band_verified: bool,
}

/// The local identity advertised to peers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalProfile {
	/// Display name.
	pub display_name: String,
	/// Content hash of the local avatar.
	pub avatar: Option<[u8; 32]>,
	/// Advertise in LAN discovery without a name or key hint.
	pub hidden: bool,
}

/// Who may ring this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InboundPolicy {
	/// Any trusted peer.
	#[default]
	Anyone,
	/// Only peers proven to share a channel.
	SharedChannelOnly,
}

/// How this device appears in LAN discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdvertMode {
	/// Name and key hint are advertised.
	#[default]
	Visible,
	/// Neither is advertised.
	Hidden,
}

/// What another user needs to reach this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionDetails {
	/// Port the peer listener is bound to.
	pub listen_port: u16,
	/// Digest of the local static key.
	pub fingerprint: [u8; 32],
	/// Eligible LAN addresses, best first.
	pub addresses: Vec<std::net::IpAddr>,
	/// The best connection string, if any address is eligible.
	pub connection_string: Option<String>,
}

/// One call's live levels and counters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CallMeter {
	/// The call.
	pub call: Option<CallId>,
	/// The remote side is audibly speaking.
	pub speaking: bool,
	/// Level of the received audio, if any.
	pub receive_dbfs: Option<f32>,
	/// Packets stored in the jitter buffer.
	pub packets_received: u64,
	/// Packets refused as late, duplicate or too far.
	pub packets_late: u64,
	/// Frames the jitter buffer had to conceal.
	pub packets_missing: u64,
	/// Samples produced by concealment.
	pub concealed_samples: u64,
	/// Playout FIFO target in milliseconds.
	pub playout_target_ms: f64,
}

/// Live media measurements, read without blocking the audio threads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaTelemetry {
	/// Microphone level while capture is open.
	pub microphone_dbfs: Option<f32>,
	/// Frames the capture callback dropped because the ring was full.
	pub capture_overruns: u64,
	/// Frames the playback callback filled with silence.
	pub render_underruns: u64,
	/// A microphone test is running.
	pub mic_test_active: bool,
	/// Per call.
	pub calls: Vec<CallMeter>,
}

/// A preference change that must be persisted.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsChange {
	/// New display name.
	DisplayName(String),
	/// Explicit device selection; empty strings mean "none selected".
	Devices {
		/// Input device id.
		input: String,
		/// Output device id.
		output: String,
	},
	/// Qualified noise suppression backend id.
	NoiseBackend(String),
	/// Push-to-talk release delay in milliseconds.
	PttReleaseDelay(u32),
	/// Inbound policy and discovery visibility.
	Privacy {
		/// Who may ring this device.
		inbound: InboundPolicy,
		/// How the device appears in discovery.
		advert: AdvertMode,
	},
}

/// Persisted preferences the engine needs at start-up.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoredSettings {
	/// Display name.
	pub display_name: String,
	/// Selected input device id (empty when none).
	pub input_device: String,
	/// Selected output device id (empty when system default).
	pub output_device: String,
	/// Noise suppression backend id.
	pub noise_backend: String,
	/// Microphone gain.
	pub microphone_gain: f32,
	/// Playback gain.
	pub playback_gain: f32,
	/// Push-to-talk release delay in milliseconds (0 closes at once).
	pub ptt_release_delay_ms: u32,
	/// Local avatar hash.
	pub avatar: Option<[u8; 32]>,
	/// Who may ring this device.
	pub inbound_policy: InboundPolicy,
	/// How the device appears in discovery.
	pub advert_mode: AdvertMode,
}

/// Capture, playback and per-call media resources.
pub trait MediaControl: Send + Sync {
	/// Opens the shared capture path. Complete with `SystemEvent::CaptureReady`
	/// or `CaptureFailed` echoing `generation`.
	fn open_capture(&self, generation: AudioGeneration) -> Result<(), PortError>;
	/// Closes the shared capture path.
	fn close_capture(&self, generation: AudioGeneration);
	/// Allocates media for a call. Complete with `SystemEvent::MediaPrepared`.
	fn prepare_media(&self, call: CallId, session: SessionId) -> Result<(), PortError>;
	/// Retires media for a call. Complete with `SystemEvent::MediaStopped`.
	fn stop_media(&self, call: CallId, session: SessionId);
	/// Updates the receive mix of a call.
	fn set_receive_mix(&self, mix: ReceiveMix);
	/// Sets the microphone gain (finite, 0.0 to 4.0).
	fn set_microphone_gain(&self, _gain: f32) -> Result<(), PortError> {
		Err(PortError::Unsupported("microphone gain"))
	}
	/// A snapshot of levels and counters. Reads atomics only.
	fn telemetry(&self) -> MediaTelemetry {
		MediaTelemetry::default()
	}
	/// Selects a noise suppression backend.
	fn set_noise_suppression(&self, backend: &str) -> Result<(), PortError>;
	/// Starts a local-only microphone test: the level is metered and, with
	/// `local_playback`, heard on the user's own output. Nothing is sent to
	/// any peer and the test ends by itself.
	fn start_mic_test(&self, local_playback: bool) -> Result<(), PortError>;
	/// Stops a running microphone test.
	fn stop_mic_test(&self) -> Result<(), PortError>;
}

/// Discovery, secure links and call signaling.
pub trait PeerControl: Send + Sync {
	/// Establishes (or reuses) the secure link. Report `PeerEvent::LinkUp`
	/// or `LinkDown`.
	fn ensure_link(&self, peer: PeerId) -> Result<(), PortError>;
	/// Sends a signaling message over an up link.
	fn send(&self, peer: PeerId, message: PeerMessage) -> Result<(), PortError>;
	/// Turns a manual `ip:port` into a peer identity. May block on a
	/// handshake: the service calls it from a blocking thread, never the actor.
	fn resolve_address(&self, address: &str) -> Result<PeerId, PortError>;
	/// Currently known peers.
	fn peers(&self) -> Vec<PeerInfo>;
	/// Applies a trust decision; the adapter owns the trust store.
	fn decide_trust(&self, request: TrustRequest) -> Result<(), PortError>;
	/// The advertised local profile changed.
	fn profile_changed(&self, profile: &LocalProfile);
	/// What a remote user needs to reach this device.
	fn connection_info(&self) -> Result<ConnectionDetails, PortError> {
		Err(PortError::Unsupported("connection info"))
	}
}

/// Device enumeration and selection.
pub trait DeviceControl: Send + Sync {
	/// Currently known devices (cached; must not block on the OS).
	fn devices(&self) -> Vec<DeviceInfo>;
	/// Validates and applies an explicit selection.
	fn select(&self, input: &str, output: &str) -> Result<(), PortError>;
}

/// Persistence of preferences and the local avatar.
pub trait SettingsStore: Send + Sync {
	/// Loads the persisted settings.
	fn load(&self) -> StoredSettings;
	/// Persists a change.
	fn save(&self, change: &SettingsChange) -> Result<(), PortError>;
	/// Validates, normalizes and stores an avatar, returning its content hash.
	fn store_avatar(&self, encoded: &[u8]) -> Result<[u8; 32], PortError>;
	/// The canonical PNG stored under `hash` (local or received), if cached.
	fn avatar(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError>;
}

/// The full set of adapters the control actor drives.
#[derive(Clone)]
pub struct Ports {
	/// Media adapter.
	pub media: Arc<dyn MediaControl>,
	/// Peer adapter.
	pub peers: Arc<dyn PeerControl>,
	/// Device adapter.
	pub devices: Arc<dyn DeviceControl>,
	/// Preferences adapter.
	pub settings: Arc<dyn SettingsStore>,
}

impl Ports {
	/// Ports whose every capability reports [`PortError::Unsupported`]. Used
	/// until the real adapters are wired in.
	pub fn unwired() -> Self {
		let unwired = Arc::new(Unwired);
		Self {
			media: unwired.clone(),
			peers: unwired.clone(),
			devices: unwired.clone(),
			settings: unwired,
		}
	}
}

/// Adapter that supports nothing.
#[derive(Debug, Clone, Copy)]
pub struct Unwired;

impl MediaControl for Unwired {
	fn open_capture(&self, _: AudioGeneration) -> Result<(), PortError> {
		Err(PortError::Unsupported("media"))
	}
	fn close_capture(&self, _: AudioGeneration) {}
	fn prepare_media(&self, _: CallId, _: SessionId) -> Result<(), PortError> {
		Err(PortError::Unsupported("media"))
	}
	fn stop_media(&self, _: CallId, _: SessionId) {}
	fn set_receive_mix(&self, _: ReceiveMix) {}
	fn set_noise_suppression(&self, _: &str) -> Result<(), PortError> {
		Err(PortError::Unsupported("noise suppression"))
	}
	fn start_mic_test(&self, _: bool) -> Result<(), PortError> {
		Err(PortError::Unsupported("microphone test"))
	}
	fn stop_mic_test(&self) -> Result<(), PortError> {
		Err(PortError::Unsupported("microphone test"))
	}
}

impl PeerControl for Unwired {
	fn ensure_link(&self, _: PeerId) -> Result<(), PortError> {
		Err(PortError::Unsupported("peer links"))
	}
	fn send(&self, _: PeerId, _: PeerMessage) -> Result<(), PortError> {
		Err(PortError::Unsupported("peer links"))
	}
	fn resolve_address(&self, _: &str) -> Result<PeerId, PortError> {
		Err(PortError::Unsupported("manual addresses"))
	}
	fn peers(&self) -> Vec<PeerInfo> {
		Vec::new()
	}
	fn decide_trust(&self, _: TrustRequest) -> Result<(), PortError> {
		Err(PortError::Unsupported("peer trust"))
	}
	fn profile_changed(&self, _: &LocalProfile) {}
}

impl DeviceControl for Unwired {
	fn devices(&self) -> Vec<DeviceInfo> {
		Vec::new()
	}
	fn select(&self, _: &str, _: &str) -> Result<(), PortError> {
		Err(PortError::Unsupported("device selection"))
	}
}

impl SettingsStore for Unwired {
	fn load(&self) -> StoredSettings {
		StoredSettings {
			display_name: "My device".to_owned(),
			microphone_gain: 1.0,
			playback_gain: 1.0,
			..StoredSettings::default()
		}
	}
	fn save(&self, _: &SettingsChange) -> Result<(), PortError> {
		Ok(())
	}
	fn store_avatar(&self, _: &[u8]) -> Result<[u8; 32], PortError> {
		Err(PortError::Unsupported("avatars"))
	}
	fn avatar(&self, _: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError> {
		Ok(None)
	}
}
