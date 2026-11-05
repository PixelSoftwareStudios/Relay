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
	/// Push-to-talk release delay in milliseconds (0 to 2000).
	pub ptt_release_delay_ms: Option<u32>,
	/// Ring for an incoming call while a client is attached.
	pub ringtone_enabled: Option<bool>,
	/// Ring volume (0.0 to 1.0).
	pub ringtone_volume: Option<f32>,
	/// Ringback while an outgoing call rings.
	pub ringback_enabled: Option<bool>,
	/// Connect and hang-up blips.
	pub call_sounds_enabled: Option<bool>,
}

impl GlobalControls {
	/// Whether any ring preference is being changed.
	pub fn has_ring(&self) -> bool {
		self.ringtone_enabled.is_some()
			|| self.ringtone_volume.is_some()
			|| self.ringback_enabled.is_some()
			|| self.call_sounds_enabled.is_some()
	}

	/// Whether anything but ring preferences is being changed.
	pub fn has_other(&self) -> bool {
		*self
			!= Self {
				muted: None,
				deafened: None,
				microphone_gain: None,
				playback_gain: None,
				ptt_release_delay_ms: None,
				..*self
			}
	}
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
