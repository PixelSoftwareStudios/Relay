//! Adapter boundary of the control actor.
//!
//! The actor owns the domain state and never performs I/O. Everything it needs
//! from the outside world goes through these traits. All methods are
//! synchronous and must not block: implementations hand the work to their own
//! thread or task and report completion back through a `ControlHandle`
//! (`system`, `peer`, `peers_changed`, `devices_changed`).

use std::sync::Arc;

use relay_config::history::{CallRecord, NewCallRecord};
use relay_core::{
	AudioGeneration, CallId, DeviceKeyId, PeerId, PeerMessage, ReceiveMix, SessionId,
};
use thiserror::Error;

use super::tones::{RingPrefs, ToneCommand};

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
	/// More than one object matches; the request must be narrower.
	#[error("ambiguous: {0}")]
	Ambiguous(String),
	/// The device at the address does not have the key the decision is about.
	#[error("the device does not have the expected key")]
	KeyMismatch,
	/// The adapter is temporarily unable to act.
	#[error("unavailable: {0}")]
	Unavailable(String),
	/// A request refused with its own stable problem code (the text is for people).
	#[error("{message}")]
	Rejected {
		/// Stable machine code, e.g. `RINGTONE_TOO_LARGE`.
		code: &'static str,
		/// Sanitized description.
		message: String,
	},
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
	/// Set when this peer presented a key other than the pinned one: the pinned key digest.
	pub key_change: Option<PeerId>,
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

/// How a request names a stored peer when it has no key to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerSelector {
	/// The peer id (the key digest).
	Id(PeerId),
	/// A hex fingerprint prefix (8 digits or more) or an exact display name.
	Query(String),
}

/// A peer whose stored trust was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgottenPeer {
	/// The peer.
	pub id: PeerId,
	/// The name it was known by.
	pub name: String,
	/// Refused keys of this contact whose key-change state went with it.
	pub key_changed: Vec<PeerId>,
}

/// A contact whose pinned key was replaced after the user verified the new key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedPeer {
	/// The new key digest, now the contact's identity.
	pub id: PeerId,
	/// The key digest that was pinned before.
	pub previous: PeerId,
	/// The name carried over.
	pub name: String,
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
	/// Smoothed round trip to the peer over the authenticated control link, if timed.
	pub rtt_ms: Option<f64>,
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
	/// Device selection; an empty id means "follow the system default".
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
	/// Whether the desktop app starts the daemon by itself.
	AutostartDaemon(bool),
	/// Inbound policy and discovery visibility.
	Privacy {
		/// Who may ring this device.
		inbound: InboundPolicy,
		/// How the device appears in discovery.
		advert: AdvertMode,
	},
	/// Ring, ringback and call-sound preferences.
	Ring(RingPrefs),
}

/// Persisted preferences the engine needs at start-up.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoredSettings {
	/// Display name.
	pub display_name: String,
	/// Pinned input device id (empty when following the system default).
	pub input_device: String,
	/// Pinned output device id (empty when following the system default).
	pub output_device: String,
	/// Noise suppression backend id.
	pub noise_backend: String,
	/// Microphone gain.
	pub microphone_gain: f32,
	/// Playback gain.
	pub playback_gain: f32,
	/// Push-to-talk release delay in milliseconds (0 closes at once).
	pub ptt_release_delay_ms: u32,
	/// The desktop app starts and restarts the daemon by itself.
	pub autostart_daemon: bool,
	/// Local avatar hash.
	pub avatar: Option<[u8; 32]>,
	/// Who may ring this device.
	pub inbound_policy: InboundPolicy,
	/// How the device appears in discovery.
	pub advert_mode: AdvertMode,
	/// Ring, ringback and call-sound preferences.
	pub ring: RingPrefs,
	/// File name of the custom ringtone; empty for the built-in chime.
	pub ringtone_name: String,
	/// Finished calls are recorded in the call history.
	pub history_enabled: bool,
	/// The mode a call starts in when it connects.
	pub default_transmit_mode: relay_core::RouteMode,
	/// Push-to-talk may be chosen and used.
	pub ptt_enabled: bool,
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
	/// Live diagnostics of one call: path, packet counters, latency figures. Reads atomics
	/// only. `None` when the call has no media.
	fn call_stats(&self, _call: CallId) -> Option<super::call_stats::CallRaw> {
		None
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
	/// Drives the tone player: ring, ringback and blips are mixed into the
	/// local playback only and never reach a peer.
	fn tone(&self, _command: ToneCommand) {}
	/// Replaces the incoming-call ring with processed samples (48 kHz mono, faded, with its
	/// pause), or restores the built-in chime with `None`. Never blocks the caller on audio.
	fn set_ringtone(&self, _ring: Option<Arc<[f32]>>) {}
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
	/// Dials a scanned `relay://host:port#fingerprint` string with that key pinned: the
	/// handshake refuses any other device before our key is revealed. Only when the
	/// authenticated key matches is the peer pinned as verified (out of band); any failure
	/// stores nothing.
	/// May block: the service calls it from a blocking thread, never the actor.
	fn connect_pinned(&self, _text: &str) -> Result<PeerId, super::ConnectFailure> {
		Err(super::ConnectFailure::Unreachable(
			"dialling is not available".to_owned(),
		))
	}
	/// Currently known peers.
	fn peers(&self) -> Vec<PeerInfo>;
	/// Stores a trust decision, reaching the device again when no link is held, and
	/// reports why it could not. May block: the service calls it from a blocking
	/// thread, never the actor. [`Self::decide_trust`] runs afterwards in the actor.
	fn confirm_trust(&self, _request: &TrustRequest) -> Result<(), PortError> {
		Ok(())
	}
	/// Applies a trust decision; the adapter owns the trust store.
	fn decide_trust(&self, request: TrustRequest) -> Result<(), PortError>;
	/// Removes the stored trust decision of the one peer `selector` names, cuts its link and
	/// stops reconnecting.
	fn forget_peer(&self, _selector: &PeerSelector) -> Result<ForgottenPeer, PortError> {
		Err(PortError::Unsupported("forgetting peers"))
	}
	/// Replaces the pinned key behind the key-changed device `peer` (the refused key) by that
	/// key, verified. Needs a remembered key change, `verified`, and `fingerprint` equal to the
	/// full fingerprint of `peer`; otherwise nothing changes. `via` is a connection string whose
	/// address is dialled afterwards.
	fn replace_pinned_key(
		&self,
		_peer: PeerId,
		_fingerprint: &str,
		_verified: bool,
		_via: Option<&str>,
	) -> Result<ReplacedPeer, PortError> {
		Err(PortError::Unsupported("replacing a pinned key"))
	}
	/// The refused key a scanned connection string names, when that device is a key-changed
	/// contact.
	fn key_change_for(&self, _text: &str) -> Option<PeerId> {
		None
	}
	/// The advertised local profile changed.
	fn profile_changed(&self, profile: &LocalProfile);
	/// Somebody is (or is no longer) attached: advertise only while true.
	fn presence_changed(&self, _attached: bool) {}
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
	/// Validates and saves flat `section.key` entries together (one write for all that pass),
	/// in order. One answer per entry: `None` when saved, else why it was refused. The whole
	/// call fails only if the store cannot be written.
	fn apply(&self, _entries: &[(String, String)]) -> Result<Vec<Option<String>>, PortError> {
		Err(PortError::Unsupported("settings by key"))
	}
	/// Every preference as flat `section.key` text, sorted.
	fn all(&self) -> Result<std::collections::BTreeMap<String, String>, PortError> {
		Err(PortError::Unsupported("settings by key"))
	}
	/// Appends a finished call to the history; `now_ms` is the wall clock used for pruning.
	fn record_call(&self, _entry: &NewCallRecord, _now_ms: u64) -> Result<(), PortError> {
		Err(PortError::Unsupported("call history"))
	}
	/// Newest first; `before` continues after that entry id.
	fn call_history(
		&self,
		_limit: usize,
		_before: Option<u64>,
	) -> Result<Vec<CallRecord>, PortError> {
		Err(PortError::Unsupported("call history"))
	}
	/// Deletes every history entry.
	fn clear_call_history(&self) -> Result<(), PortError> {
		Err(PortError::Unsupported("call history"))
	}
	/// Bytes the avatar cache holds now, when the store knows (shown next to its limit).
	fn avatar_cache_bytes(&self) -> Option<u64> {
		None
	}
	/// Decodes, levels and stores an uploaded ringtone for this device and returns its
	/// processed samples. Refused with `PortError::Rejected` (`RINGTONE_*`) when it cannot be one.
	fn store_ringtone(&self, _name: &str, _data: &[u8]) -> Result<Arc<[f32]>, PortError> {
		Err(PortError::Unsupported("ringtones"))
	}
	/// Deletes the stored ringtone (idempotent).
	fn clear_ringtone(&self) -> Result<(), PortError> {
		Err(PortError::Unsupported("ringtones"))
	}
	/// The stored ringtone: the name of the file it came from and its processed samples.
	fn ringtone(&self) -> Option<(String, Arc<[f32]>)> {
		None
	}
	/// Removes the local avatar (it is no longer pinned in the cache).
	fn clear_avatar(&self) -> Result<(), PortError>;
	/// The canonical PNG stored under `hash` (local or received), if cached.
	fn avatar(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError>;
	/// The favorites in order (trusted peers only, at most 6).
	fn favorites(&self) -> Vec<PeerId> {
		Vec::new()
	}
	/// Replaces the favorites. Refused with a reason (`PortError::Invalid`) when the list has
	/// more than 6 peers, repeats one or names a peer that is not trusted.
	fn set_favorites(&self, _ordered: &[PeerId]) -> Result<(), PortError> {
		Err(PortError::Unsupported("favorites"))
	}
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
			autostart_daemon: true,
			microphone_gain: 1.0,
			playback_gain: 1.0,
			history_enabled: true,
			..StoredSettings::default()
		}
	}
	fn save(&self, _: &SettingsChange) -> Result<(), PortError> {
		Ok(())
	}
	fn store_avatar(&self, _: &[u8]) -> Result<[u8; 32], PortError> {
		Err(PortError::Unsupported("avatars"))
	}
	fn clear_avatar(&self) -> Result<(), PortError> {
		Ok(())
	}
	fn avatar(&self, _: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError> {
		Ok(None)
	}
}
