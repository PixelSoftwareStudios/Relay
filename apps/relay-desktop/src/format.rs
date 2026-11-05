//! Display text and small pure helpers shared by the views.

use crate::model::{Call, Link, Phase};

const FLOOR_DBFS: f32 = -60.0;
pub const MAX_AVATAR_BYTES: usize = 256 * 1024;

/// Map a dBFS reading to a 0..1 bar fill; unknown readings render empty.
pub fn level_fraction(dbfs: Option<f32>) -> f32 {
	match dbfs {
		Some(v) if v.is_finite() => ((v - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0),
		_ => 0.0,
	}
}

pub fn format_dbfs(dbfs: Option<f32>) -> String {
	match dbfs {
		Some(v) if v.is_finite() => format!("{v:.0} dBFS"),
		_ => "no signal data".to_owned(),
	}
}

pub fn phase_label(phase: Phase) -> &'static str {
	match phase {
		Phase::Preparing => "Preparing",
		Phase::OutgoingRinging => "Ringing",
		Phase::IncomingRinging => "Incoming call",
		Phase::Negotiating => "Connecting",
		Phase::Connected => "Connected",
		Phase::Interrupted => "Interrupted",
		Phase::Ending => "Ending",
		Phase::Ended => "Ended",
		Phase::Failed => "Failed",
		Phase::Declined => "Declined",
		Phase::Cancelled => "Cancelled",
	}
}

pub fn link_label(link: Link) -> &'static str {
	match link {
		Link::Connecting => "Connecting to the Relay daemon",
		Link::Connected => "Connected",
		Link::Reconnecting => "Daemon connection lost, reconnecting",
		Link::Offline => "Daemon not running",
	}
}

/// Up to two leading letters of a display name.
pub fn initials(name: &str) -> String {
	let letters: String = name
		.split_whitespace()
		.take(2)
		.filter_map(|word| word.chars().next())
		.flat_map(char::to_uppercase)
		.collect();
	if letters.is_empty() {
		"?".to_owned()
	} else {
		letters
	}
}

/// Stable hue (0..360) derived from an id so each peer keeps its colour.
pub fn hue_from_id(id: &str) -> u32 {
	id.chars()
		.fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32))
		% 360
}

pub fn gain_percent(gain: f32) -> String {
	format!("{}%", (gain * 100.0).round())
}

/// Client-side pre-check only; the engine decodes and validates for real.
pub fn avatar_problem(size: usize, media_type: &str) -> Option<&'static str> {
	if !matches!(media_type, "image/png" | "image/jpeg") {
		Some("Use a PNG or JPEG image.")
	} else if size == 0 {
		Some("The file is empty.")
	} else if size > MAX_AVATAR_BYTES {
		Some("The image must be 256 KiB or smaller.")
	} else {
		None
	}
}

/// Media type from a file name, by extension only.
pub fn media_type_for(path: &str) -> &'static str {
	let lower = path.to_ascii_lowercase();
	if lower.ends_with(".png") {
		"image/png"
	} else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
		"image/jpeg"
	} else {
		"application/octet-stream"
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
	Idle,
	Live,
	Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmitStatus {
	pub label: &'static str,
	pub detail: String,
	pub tone: Tone,
}

/// What this client can and cannot do about push-to-talk.
pub const HOTKEY_NOTE: &str = "This app registers no global hotkey and has no on-screen talk button yet. \
Push-to-talk only transmits while an engine-side input source (for example the CLI) holds the key.";

/// Honest transmit text. Only the engine's `effective_transmitting` ever
/// claims the microphone is live; a requested mode alone never does.
pub fn describe_transmit(call: &Call) -> TransmitStatus {
	use crate::model::Mode;
	let t = &call.transmit;
	let blocked = if t.blocked_reason.is_empty() {
		String::new()
	} else {
		format!(" Engine reason: {}.", t.blocked_reason)
	};
	let status = |label, detail: String, tone| TransmitStatus {
		label,
		detail,
		tone,
	};
	if t.mode == Mode::ReceiveOnly {
		return status(
			"Listening only",
			"Your microphone is not sent.".into(),
			Tone::Idle,
		);
	}
	if !t.consent_granted {
		return status(
			"Microphone not allowed",
			format!("Transmit consent has not been given for this call.{blocked}"),
			Tone::Warn,
		);
	}
	if t.effective_transmitting {
		let how = if t.mode == Mode::OpenMic {
			"open microphone"
		} else {
			"push-to-talk key held"
		};
		return status(
			"Transmitting",
			format!("Microphone is live ({how})."),
			Tone::Live,
		);
	}
	if t.mode == Mode::OpenMic {
		return status(
			"Open mic armed, not transmitting",
			format!("The engine is not sending audio right now.{blocked}"),
			Tone::Warn,
		);
	}
	status(
		"Push-to-talk armed",
		format!("Waiting for a key press from an engine-side input source.{blocked}"),
		Tone::Idle,
	)
}
