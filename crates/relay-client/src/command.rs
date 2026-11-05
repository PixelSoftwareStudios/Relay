//! The request vocabulary both clients speak. Values are already validated by
//! the caller; call-bound commands carry the controller generation to use.

use relay_api::pb::{TransmitMode, TrustState};

/// Who to call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
	/// A discovered peer by 32-byte id.
	Peer(Vec<u8>),
	/// A manual `ip:port` on the local network.
	Address(String),
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
}
