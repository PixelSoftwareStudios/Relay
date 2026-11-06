//! Stable lowercase names for protocol enums, shared by text, JSON and the TUI.

use relay_api::pb::{CallState, DeviceDirection, TransmitMode, TrustState};

/// Name of a raw `CallState` value.
pub fn call_state(raw: i32) -> &'static str {
	match CallState::try_from(raw).unwrap_or(CallState::Unspecified) {
		CallState::Unspecified => "unknown",
		CallState::OutgoingRinging => "outgoing-ringing",
		CallState::IncomingRinging => "incoming-ringing",
		CallState::Negotiating => "negotiating",
		CallState::Connected => "connected",
		CallState::Interrupted => "interrupted",
		CallState::Ending => "ending",
		CallState::Ended => "ended",
		CallState::Failed => "failed",
		CallState::Preparing => "preparing",
		CallState::Declined => "declined",
		CallState::Cancelled => "cancelled",
	}
}

/// Whether a call in this state still needs attention or carries audio.
pub fn call_is_live(raw: i32) -> bool {
	!matches!(
		CallState::try_from(raw).unwrap_or(CallState::Unspecified),
		CallState::Ended | CallState::Failed | CallState::Declined | CallState::Cancelled
	)
}

/// Name of a raw `TransmitMode` value.
pub fn transmit_mode(raw: i32) -> &'static str {
	match TransmitMode::try_from(raw).unwrap_or(TransmitMode::Unspecified) {
		TransmitMode::Unspecified => "unknown",
		TransmitMode::Muted => "muted",
		TransmitMode::PushToTalk => "push-to-talk",
		TransmitMode::Unmuted => "unmuted",
	}
}

/// Name of a raw `TrustState` value.
pub fn trust_state(raw: i32) -> &'static str {
	match TrustState::try_from(raw).unwrap_or(TrustState::Unspecified) {
		TrustState::Unspecified => "unknown",
		TrustState::Unknown => "unknown",
		TrustState::TofuUnverified => "unverified",
		TrustState::Verified => "verified",
		TrustState::KeyChanged => "key-changed",
		TrustState::Blocked => "blocked",
	}
}

/// Name of a raw `DeviceDirection` value.
pub fn direction(raw: i32) -> &'static str {
	match DeviceDirection::try_from(raw).unwrap_or(DeviceDirection::Unspecified) {
		DeviceDirection::Input => "input",
		DeviceDirection::Output => "output",
		DeviceDirection::Unspecified => "unknown",
	}
}

/// Lowercase hex of an id.
pub fn hex_id(id: &[u8]) -> String {
	hex::encode(id)
}

/// Short form of an id for tables: the first 8 hex characters.
pub fn short_id(id: &[u8]) -> String {
	let full = hex::encode(id);
	full.chars().take(8).collect()
}
