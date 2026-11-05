//! Window-less rules of the Messages panel: counters, what the compose box
//! may do, which buttons a file offer shows, and the user-facing texts for
//! the daemon's problem codes. Everything here is pure and tested directly.

use relay_io::localtime::LocalZone;
use relay_messages::limits::{FREE_MARGIN_PERCENT, MAX_TEXT_BYTES, MIN_FREE_MARGIN_BYTES};

use super::model::{OfferStatus, OfferView, SessionView};
use super::state::Feed;

/// An inline image the daemon accepts: its control record is 16 KiB.
pub const INLINE_IMAGE_MAX_BYTES: usize = 15 * 1024;
/// Decoded pixels the UI refuses to allocate (16 x 2^20).
pub const MAX_DECODED_PIXELS: u64 = relay_messages::limits::MAX_IMAGE_PIXELS;
/// Files at or above this size get a "large file" note.
pub const LARGE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// `1.5 MiB` style sizes.
pub fn format_size(bytes: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
	if bytes < 1024 {
		return format!("{bytes} B");
	}
	let mut value = bytes as f64;
	let mut unit = 0;
	while value >= 1024.0 && unit < UNITS.len() - 1 {
		value /= 1024.0;
		unit += 1;
	}
	format!("{value:.1} {}", UNITS[unit])
}

/// First 8 hex characters of the BLAKE3 hash, enough to compare by eye.
pub fn hash_short(hash: &[u8]) -> String {
	if hash.is_empty() {
		return "none".to_owned();
	}
	let hex = hex::encode(hash);
	format!("{}…", &hex[..hex.len().min(8)])
}

/// The clock of a message row and the text shown on hover.
#[derive(Debug, PartialEq, Eq)]
pub struct RowTimes {
	/// `HH:MM:SS` in the viewer's zone, when this device received or sent it.
	pub label: String,
	pub hover: String,
}

/// Row times: the label is local time; the hover gives the full local date and
/// offset with the UTC clock, then the sender's own claim in the same form.
pub fn row_times(zone: &LocalZone, received_ms: u64, sent_ms: u64) -> RowTimes {
	RowTimes {
		label: zone.clock(received_ms),
		hover: format!(
			"{}, when this device received or sent it.\nThe sender's own clock claims {}.",
			zone.hover(received_ms),
			zone.hover(sent_ms)
		),
	}
}

/// A short, honest guess of what a file is, from its name and advisory type.
pub fn type_guess(name: &str, media_type: &str, executable_warning: bool) -> &'static str {
	if executable_warning {
		return "Program or script (runs code)";
	}
	let ext = name
		.rsplit_once('.')
		.map_or("", |(_, e)| e)
		.to_ascii_lowercase();
	match (media_type.split('/').next().unwrap_or(""), ext.as_str()) {
		("image", _) => "Image",
		("audio", _) => "Audio",
		("video", _) => "Video",
		("text", _) | (_, "txt" | "md" | "log" | "json" | "csv" | "toml" | "yaml" | "yml") => {
			"Text"
		}
		(_, "zip" | "tar" | "gz" | "7z" | "rar" | "xz" | "zst") => "Archive",
		(_, "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx" | "ppt" | "pptx") => "Document",
		_ => "Unknown type",
	}
}

/// Free space a download needs: the size plus 10 percent, at least 16 MiB.
pub fn required_space(size: u64) -> u64 {
	size.saturating_add((size / 100 * FREE_MARGIN_PERCENT).max(MIN_FREE_MARGIN_BYTES))
}

/// Warnings shown on an offer card (executable, large, over the daemon's cap).
pub fn offer_warnings(offer: &OfferView, max_offer_mib: Option<u32>) -> Vec<String> {
	let mut out = Vec::new();
	if offer.executable_warning {
		out.push("The name looks like a program or script. Relay never runs files; do not open it unless you trust the sender.".to_owned());
	}
	if offer.size >= LARGE_FILE_BYTES {
		out.push(format!(
			"Large file: downloading needs about {} of free space.",
			format_size(required_space(offer.size))
		));
	} else if !offer.outgoing && offer.status == OfferStatus::Offered {
		out.push(format!(
			"Needs about {} free in the folder you choose.",
			format_size(required_space(offer.size))
		));
	}
	if let Some(cap) = max_offer_mib.filter(|c| *c > 0)
		&& offer.size > u64::from(cap) * 1024 * 1024
	{
		out.push(format!("Over this daemon's {cap} MiB offer limit."));
	}
	out
}

/// Byte counter of the compose box against the 4 KiB limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
	pub used: usize,
	pub limit: usize,
}

impl Counter {
	pub fn of(text: &str) -> Self {
		Self {
			used: text.len(),
			limit: MAX_TEXT_BYTES,
		}
	}

	pub fn over(self) -> bool {
		self.used > self.limit
	}

	/// Whether the text may be sent: not empty (after trimming) and not over.
	pub fn sendable(self, text: &str) -> bool {
		!self.over() && !text.trim().is_empty()
	}

	pub fn label(self) -> String {
		format!("{} / {} bytes", self.used, self.limit)
	}
}

/// Why the compose box is unavailable. `export_clear` says whether the
/// Export and Clear buttons belong next to the explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
	pub reason: String,
	pub export_clear: bool,
}

/// Whether text can be composed now; `None` means yes.
pub fn compose_blocked(
	feed: &Feed,
	daemon_enabled: bool,
	session: &SessionView,
) -> Option<Blocked> {
	let blocked = |reason: &str, export_clear: bool| {
		Some(Blocked {
			reason: reason.to_owned(),
			export_clear,
		})
	};
	if *feed != Feed::Live {
		return blocked(
			"Not connected to the daemon, so nothing can be sent.",
			false,
		);
	}
	if !daemon_enabled {
		return blocked(
			"Messaging is turned off in the daemon (messages.enabled = false).",
			false,
		);
	}
	if !session.is_live() {
		return blocked("This call has ended. The transcript is read-only.", true);
	}
	if !session.perms.allow_messages {
		return blocked(
			"Messages are turned off for this call. Both sides enforce this: turn it on below, and the other side must allow it too.",
			false,
		);
	}
	if session.history_full() {
		return blocked(
			"This call's message history is full. Nothing is deleted automatically: export it and clear it to continue.",
			true,
		);
	}
	None
}

/// Whether a file or image may be sent now.
pub fn files_blocked(session: &SessionView) -> Option<&'static str> {
	if !session.is_live() {
		Some("The call has ended.")
	} else if !session.perms.allow_files {
		Some("Files are turned off for this call.")
	} else {
		None
	}
}

/// The buttons a file offer card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OfferActions {
	pub download: bool,
	pub reject: bool,
	pub cancel: bool,
	pub request_preview: bool,
	pub approve_preview: bool,
}

pub fn offer_actions(offer: &OfferView, session: &SessionView) -> OfferActions {
	let mut a = OfferActions::default();
	if !session.is_live() {
		return a;
	}
	let files_on = session.perms.allow_files;
	match (offer.outgoing, offer.status) {
		(false, OfferStatus::Offered) => {
			a.download = files_on;
			a.reject = true;
			a.request_preview = files_on && !offer.preview_requested && offer.preview.is_empty();
		}
		(true, OfferStatus::Offered) => {
			a.cancel = true;
			a.approve_preview = offer.preview_approval_pending;
		}
		(_, OfferStatus::InProgress) => a.cancel = true,
		_ => {}
	}
	a
}

/// Progress 0..=1 of a transfer; unknown size counts as 0.
pub fn progress(offer: &OfferView) -> f32 {
	if offer.size == 0 {
		return 0.0;
	}
	(offer.transferred as f64 / offer.size as f64).clamp(0.0, 1.0) as f32
}

pub fn status_label(offer: &OfferView) -> String {
	let who = if offer.outgoing {
		"You offered"
	} else {
		"Offered to you"
	};
	match offer.status {
		OfferStatus::Offered => who.to_owned(),
		OfferStatus::InProgress => format!("Transferring {:.0}%", progress(offer) * 100.0),
		OfferStatus::Completed if offer.outgoing => "Sent".to_owned(),
		OfferStatus::Completed => "Saved".to_owned(),
		OfferStatus::Rejected => "Declined".to_owned(),
		OfferStatus::Failed => format!("Failed ({})", reason_word(&offer.failure)),
		OfferStatus::Cancelled => "Cancelled".to_owned(),
		OfferStatus::Expired => "Expired: the call ended, the file is gone from here".to_owned(),
	}
}

fn reason_word(failure: &str) -> &str {
	if failure.is_empty() {
		"unknown reason"
	} else {
		failure
	}
}

/// Text for a failed message delivery.
pub fn delivery_failure(reason: &str) -> String {
	match reason {
		"timeout" => "Not delivered: the other side did not answer.".to_owned(),
		"disabled" => "Refused: the other side has messages turned off.".to_owned(),
		"too_large" => "Refused: too large.".to_owned(),
		"invalid" => "Refused: not valid text.".to_owned(),
		"rate_limited" => "Refused: sent too fast.".to_owned(),
		"history_full" => "Refused: the other side's history is full.".to_owned(),
		"unsupported" => "Refused: the other side does not support this.".to_owned(),
		"ended" => "Not delivered: the call ended.".to_owned(),
		_ => "Not delivered.".to_owned(),
	}
}

/// What to tell the user for a daemon problem code. Never includes content.
pub fn problem_text(code: &str) -> String {
	let text = match code {
		"MESSAGES_DISABLED" => "Messaging is turned off in the daemon.",
		"SESSION_NOT_FOUND" => "That call has no messaging session (any more).",
		"SESSION_ENDED" => "The call has ended; the transcript is read-only.",
		"SESSION_PERMISSION_OFF" => "That is turned off for this call (both sides must allow it).",
		"PEER_UNAVAILABLE" => "The other side cannot be reached right now.",
		"TEXT_INVALID" => "That text cannot be sent (control or invisible characters).",
		"TEXT_TOO_LONG" => "That message is longer than 4096 bytes.",
		"IMAGE_INVALID" | "IMAGE_UNSUPPORTED" => "That image cannot be sent.",
		"IMAGE_TOO_LARGE" => "That image is too large to send inline; offer it as a file.",
		"HISTORY_FULL" => "The message history of this call is full. Export it and clear it.",
		"RATE_LIMITED" => "Slow down: too many messages.",
		"TOO_MANY_OFFERS" => "Too many file offers in this call.",
		"MESSAGE_NOT_FOUND" | "MESSAGE_STATE" => "That message cannot be changed now.",
		"OFFER_NOT_FOUND" | "OFFER_STATE" => "That file offer cannot be changed now.",
		"FILE_REFUSED" => "The daemon refused that file (it must be a regular file, not a link).",
		"FILE_TOO_LARGE" => "That file is over the daemon's offer size limit.",
		"FILE_CHANGED" => "The file changed while it was being read; offer it again.",
		"TOO_MANY_TRANSFERS" => "Two transfers are already running; wait for one to finish.",
		"NO_SPACE" => "Not enough free space in that folder.",
		"DESTINATION_INVALID" => "That folder or file name cannot be used.",
		"PREVIEW_UNAVAILABLE" => "No text preview is possible for that file.",
		"EXPORT_TOO_LARGE" => "The export is too large to return; choose a file.",
		"EXPORT_WRITE_FAILED" => "The daemon could not write the export file.",
		"INVALID_SETTING" => "The daemon refused that setting.",
		"PermissionDenied" | "PERMISSION_DENIED" => {
			"The daemon did not grant this app the messages capability."
		}
		_ => "The daemon refused the request.",
	};
	text.to_owned()
}

/// Which session the panel shows: the one of the requested call, else the
/// current choice while it exists, else the first live one, else the newest
/// retained.
pub fn choose_session(
	sessions: &[SessionView],
	current: Option<&str>,
	focus_call: Option<&str>,
) -> Option<String> {
	if let Some(call) = focus_call
		&& let Some(s) = sessions.iter().find(|s| s.is_live() && s.call_id == call)
	{
		return Some(s.key.clone());
	}
	if let Some(cur) = current
		&& sessions.iter().any(|s| s.key == cur)
	{
		return Some(cur.to_owned());
	}
	sessions
		.iter()
		.find(|s| s.is_live())
		.or_else(|| sessions.iter().max_by_key(|s| s.ended_ms))
		.map(|s| s.key.clone())
}

/// Used history as text, `1.2 MiB of 64.0 MiB`.
pub fn usage_label(session: &SessionView) -> String {
	format!(
		"History {} of {}",
		format_size(session.used_bytes),
		format_size(session.limit_bytes)
	)
}

/// Enter without Shift sends; Shift+Enter inserts a line break. egui's own
/// shortcut matching ignores extra Shift, so the event is checked here.
pub fn is_plain_enter(event: &egui::Event) -> bool {
	matches!(
		event,
		egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift
	)
}
