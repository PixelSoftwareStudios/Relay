//! Display text and small pure helpers shared by the views.

use crate::model::{Call, Link, Phase};

pub const MAX_AVATAR_BYTES: usize = 1024 * 1024;

/// Slider text for the push-to-talk release delay.
pub fn release_delay_label(millis: u32) -> String {
	if millis == 0 {
		"Off (closes at once)".to_owned()
	} else {
		format!("{millis} ms")
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
		Link::Starting => "Starting the Relay daemon",
		Link::Connecting => "Connecting to the Relay daemon",
		Link::Connected => "Connected",
		Link::Reconnecting => "Daemon connection lost, reconnecting",
		Link::Offline => "Daemon not running",
	}
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
		Some("The image must be 1 MiB or smaller.")
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
	if t.effective_transmitting && t.releasing {
		return status(
			"Releasing",
			"The key is up; the microphone stays live briefly so the end of a sentence is not cut."
				.into(),
			Tone::Live,
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

/// Ids of the calls this app owns that are still live. Quitting the app hangs
/// these up; other clients' calls are not touched.
pub fn own_live_calls(calls: &[Call]) -> Vec<String> {
	calls
		.iter()
		.filter(|c| c.controlled_by_me)
		.filter(|c| {
			!matches!(
				c.phase,
				Phase::Ending | Phase::Ended | Phase::Failed | Phase::Declined | Phase::Cancelled
			)
		})
		.map(|c| c.call_id.clone())
		.collect()
}

/// What closing the window does, and what quitting the app does to calls.
pub fn close_note(has_tray: bool, own_calls: usize) -> String {
	let close = if has_tray {
		"Closing the window hides it to the tray; calls keep going."
	} else {
		"There is no tray icon on this platform, so closing the window quits this app."
	};
	let quit = "Quitting this app hangs up the calls it started or accepted and leaves the Relay daemon running. This app cannot stop the daemon.";
	let live = match own_calls {
		0 => String::new(),
		1 => " One call is active.".to_owned(),
		n => format!(" {n} calls are active."),
	};
	format!("{close} {quit}{live}")
}

/// The confirmation shown before quitting while calls are live.
pub fn quit_prompt(own_calls: usize) -> String {
	match own_calls {
		0 => "Quit this app?".to_owned(),
		1 => "Quit this app? Your active call will be hung up.".to_owned(),
		n => format!("Quit this app? Your {n} active calls will be hung up."),
	}
}
