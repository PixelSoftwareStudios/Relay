//! Display text and small pure helpers shared by the views.

use crate::model::{Call, Link, Phase};
use crate::tr;

pub const MAX_AVATAR_BYTES: usize = 1024 * 1024;

/// Slider text for the push-to-talk release delay.
pub fn release_delay_label(millis: u32) -> String {
	if millis == 0 {
		tr!("release-delay-off").to_owned()
	} else {
		tr!("duration-ms", millis = millis)
	}
}

pub fn format_dbfs(dbfs: Option<f32>) -> String {
	match dbfs {
		Some(v) if v.is_finite() => tr!("level-dbfs", value = format!("{v:.0}")),
		_ => tr!("level-no-data").to_owned(),
	}
}

pub fn phase_label(phase: Phase) -> &'static str {
	match phase {
		Phase::Preparing => tr!("phase-preparing"),
		Phase::OutgoingRinging => tr!("phase-ringing"),
		Phase::IncomingRinging => tr!("phase-incoming"),
		Phase::Negotiating => tr!("phase-connecting"),
		Phase::Connected => tr!("phase-connected"),
		Phase::Interrupted => tr!("phase-interrupted"),
		Phase::Ending => tr!("phase-ending"),
		Phase::Ended => tr!("phase-ended"),
		Phase::Failed => tr!("phase-failed"),
		Phase::Declined => tr!("phase-declined"),
		Phase::Cancelled => tr!("phase-cancelled"),
	}
}

pub fn link_label(link: Link) -> &'static str {
	match link {
		Link::Starting => tr!("link-starting"),
		Link::Connecting => tr!("link-connecting"),
		Link::Connected => tr!("link-connected"),
		Link::Reconnecting => tr!("link-reconnecting"),
		Link::Offline => tr!("link-offline"),
	}
}

pub fn gain_percent(gain: f32) -> String {
	tr!(
		"gain-percent",
		value = format!("{}", (gain * 100.0).round())
	)
}

/// The UI's own wording of the avatar size rule, shown before the engine is asked.
pub fn avatar_size_refusal() -> &'static str {
	tr!("avatar-size-refusal")
}

/// Size gate, cheap enough to run before the file is read. The content check
/// ([`crate::imaging::avatar_check`]) follows once the bytes are in memory; the engine stays
/// the authority.
pub fn avatar_problem(size: usize) -> Option<&'static str> {
	(size == 0 || size > MAX_AVATAR_BYTES).then(avatar_size_refusal)
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
pub fn hotkey_note() -> &'static str {
	tr!("hotkey-note")
}

/// Honest transmit text. Only the engine's `effective_transmitting` ever
/// claims the microphone is live; a requested mode alone never does.
pub fn describe_transmit(call: &Call) -> TransmitStatus {
	use crate::model::Mode;
	let t = &call.transmit;
	let reason = t.blocked_reason.as_str();
	let status = |label, detail: String, tone| TransmitStatus {
		label,
		detail,
		tone,
	};
	if t.mode == Mode::ReceiveOnly {
		return status(
			tr!("transmit-listening"),
			tr!("transmit-listening-detail").to_owned(),
			Tone::Idle,
		);
	}
	if !t.consent_granted {
		return status(
			tr!("transmit-no-consent"),
			blocked(tr!("transmit-no-consent-detail"), reason),
			Tone::Warn,
		);
	}
	if t.effective_transmitting && t.releasing {
		return status(
			tr!("transmit-releasing"),
			tr!("transmit-releasing-detail").to_owned(),
			Tone::Live,
		);
	}
	if t.effective_transmitting {
		let how = if t.mode == Mode::OpenMic {
			tr!("transmit-how-open")
		} else {
			tr!("transmit-how-key")
		};
		return status(
			tr!("transmitting"),
			tr!("transmit-live-detail", how = how),
			Tone::Live,
		);
	}
	if t.mode == Mode::OpenMic {
		return status(
			tr!("transmit-open-armed"),
			blocked(tr!("transmit-open-armed-detail"), reason),
			Tone::Warn,
		);
	}
	status(
		tr!("transmit-ptt-armed"),
		blocked(tr!("transmit-ptt-armed-detail"), reason),
		Tone::Idle,
	)
}

/// A detail line with the engine's reason appended when it gave one. The
/// reason stays as the engine worded it.
fn blocked(detail: &str, reason: &str) -> String {
	if reason.is_empty() {
		detail.to_owned()
	} else {
		format!("{detail} {}", tr!("engine-reason", reason = reason))
	}
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
		tr!("close-tray")
	} else {
		tr!("close-no-tray")
	};
	let quit = tr!("quit-note");
	let live = if own_calls == 0 {
		String::new()
	} else {
		tr!("calls-active", count = own_calls)
	};
	format!("{close} {quit} {live}").trim_end().to_owned()
}

/// The confirmation shown before quitting while calls are live.
pub fn quit_prompt(own_calls: usize) -> String {
	if own_calls == 0 {
		tr!("quit-prompt").to_owned()
	} else {
		tr!("quit-prompt-calls", count = own_calls)
	}
}
