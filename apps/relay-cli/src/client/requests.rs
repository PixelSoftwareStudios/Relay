//! Plain request values shared by [`super::Control`] and its callers.

/// Per-call controls; `None` leaves a value unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CallControls {
	/// Mute the microphone for this call.
	pub muted: Option<bool>,
	/// Deafen this call.
	pub deafened: Option<bool>,
	/// Receive gain.
	pub receive_gain: Option<f32>,
}

/// Global controls; `None` leaves a value unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlobalControls {
	/// Global mute.
	pub muted: Option<bool>,
	/// Global deafen.
	pub deafened: Option<bool>,
	/// Microphone gain.
	pub microphone_gain: Option<f32>,
	/// Playback gain.
	pub playback_gain: Option<f32>,
}

/// A trust change for one peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustDecision {
	/// Peer id (32 bytes).
	pub peer_id: Vec<u8>,
	/// Public key SHA-256 the user compared; empty when forgetting.
	pub public_key_sha256: Vec<u8>,
	/// Requested state.
	pub requested: relay_api::pb::TrustState,
	/// Whether the fingerprint was compared out of band.
	pub out_of_band_verified: bool,
}

/// What a push-to-talk message does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttAction {
	/// Start transmitting.
	Press,
	/// Keep the lease alive.
	Renew,
	/// Stop transmitting.
	Release,
}

/// One push-to-talk message for a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PttRequest {
	/// Call being controlled.
	pub call_id: Vec<u8>,
	/// Generation of the call's transmit route as last seen.
	pub route_generation: u64,
	/// Press, renew or release.
	pub action: PttAction,
}
