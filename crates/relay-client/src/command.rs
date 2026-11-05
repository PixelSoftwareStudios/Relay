//! The request vocabulary both clients speak. Values are already validated by
//! the caller; call-bound commands carry the controller generation to use.

use std::fmt;

use relay_api::pb::{AdvertMode, InboundPolicy, TransmitMode, TrustState};

/// A secret string (channel password, invite token). `Debug` never prints it,
/// so commands can be logged or stored in view models without leaking.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
	/// Wrap secret text.
	pub fn new(text: impl Into<String>) -> Self {
		Self(text.into())
	}

	/// The secret text; call only where it goes on the wire.
	pub fn expose(&self) -> &str {
		&self.0
	}
}

impl fmt::Debug for Secret {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("Secret(<redacted>)")
	}
}

/// How a channel is advertised and joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelOptions {
	/// Advertise the channel name to members' devices.
	pub listed: bool,
	/// Require a password to join.
	pub password: bool,
	/// Require an invite token to join.
	pub invite_only: bool,
}

/// Who to call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
	/// A discovered peer by 32-byte id.
	Peer(Vec<u8>),
	/// A manual `ip:port` on the local network.
	Address(String),
	/// A pasted `relay://ip:port#fingerprint` string; the engine validates it.
	ConnectionString(String),
}

/// Lifecycle verbs on an existing call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
	/// Accept an incoming call.
	Accept,
	/// Decline an incoming call.
	Decline,
	/// End a call.
	End,
}

/// One mutation of the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	/// Start a call.
	StartCall(CallTarget),
	/// Accept, decline or end a call.
	CallAction {
		/// Which verb.
		action: CallAction,
		/// Call id (16 bytes).
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
	},
	/// Set the transmit route. Microphone use needs `consent` in this request.
	SetTransmit {
		/// Call id.
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
		/// Requested mode.
		mode: TransmitMode,
		/// Explicit microphone consent given for this request.
		consent: bool,
	},
	/// Mute, deafen or set the receive gain of one call; `None` is unchanged.
	SetCallControls {
		/// Call id.
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
		/// Mute the microphone for this call.
		muted: Option<bool>,
		/// Deafen this call.
		deafened: Option<bool>,
		/// Receive gain.
		receive_gain: Option<f32>,
	},
	/// Global mute, deafen and gains; `None` is unchanged.
	SetGlobalControls {
		/// Global mute.
		muted: Option<bool>,
		/// Global deafen.
		deafened: Option<bool>,
		/// Microphone gain.
		microphone_gain: Option<f32>,
		/// Playback gain.
		playback_gain: Option<f32>,
	},
	/// Select input and output devices.
	SelectDevices {
		/// Input device id.
		input_id: String,
		/// Output device id.
		output_id: String,
	},
	/// Select a noise suppression backend.
	SetNoiseSuppression {
		/// Qualified backend id.
		backend: String,
	},
	/// Change the display name.
	SetProfile {
		/// New name.
		display_name: String,
	},
	/// Upload an avatar as raw encoded bytes; the engine validates the image.
	UploadAvatar {
		/// Encoded image.
		bytes: Vec<u8>,
		/// Declared media type.
		media_type: String,
	},
	/// Start the microphone test.
	StartMicTest {
		/// Play the microphone back locally.
		local_playback: bool,
		/// The feedback warning was confirmed.
		confirmed_feedback: bool,
	},
	/// Stop the microphone test.
	StopMicTest,
	/// Record a trust decision about a peer.
	DecideTrust {
		/// Peer id (32 bytes).
		peer_id: Vec<u8>,
		/// Public key SHA-256 the user compared; empty when forgetting.
		public_key_sha256: Vec<u8>,
		/// Requested state.
		requested: TrustState,
		/// Whether the fingerprint was compared out of band.
		out_of_band_verified: bool,
	},
	/// Create a channel owned by this device.
	CreateChannel {
		/// Channel name.
		name: String,
		/// Listing and join requirements.
		options: ChannelOptions,
		/// Password, when `options.password` is set.
		password: Secret,
	},
	/// Join a channel by id and password, or by invite token.
	JoinChannel {
		/// Channel id (16 bytes); empty when joining by invite alone.
		channel_id: Vec<u8>,
		/// Channel password, or empty.
		password: Secret,
		/// Invite token, or empty.
		invite: Secret,
	},
	/// Leave a channel and delete its local secrets.
	LeaveChannel {
		/// Channel id (16 bytes).
		channel_id: Vec<u8>,
	},
	/// Revoke a member by identity fingerprint.
	RevokeMember {
		/// Channel id (16 bytes).
		channel_id: Vec<u8>,
		/// Member fingerprint.
		fingerprint: Vec<u8>,
	},
	/// Change inbound policy and/or advertisement mode; `None` is unchanged.
	SetPrivacy {
		/// Who may ring this device.
		inbound: Option<InboundPolicy>,
		/// Whether this device is advertised.
		advert: Option<AdvertMode>,
	},
}
