//! Window-less rules of the Messages panel: counters, what the compose box
//! may do, which buttons a file offer shows, and the user-facing texts for
//! the daemon's problem codes. Everything here is pure and tested directly.

use relay_io::localtime::LocalZone;
use relay_messages::limits::{FREE_MARGIN_PERCENT, MAX_TEXT_BYTES, MIN_FREE_MARGIN_BYTES};

use super::model::{EntryView, OfferStatus, OfferView, SessionView};
use super::state::{Feed, MessagesState};
use crate::timefmt;
use crate::tr;

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
		return tr!("hash-none").to_owned();
	}
	let hex = hex::encode(hash);
	format!("{}…", &hex[..hex.len().min(8)])
}

/// The time of a message and the text shown on hover.
#[derive(Debug, PartialEq, Eq)]
pub struct RowTimes {
	/// In the app-wide form of [`timefmt`](crate::timefmt): `1:54 AM`,
	/// `Yesterday at 6:51 PM` or `11/04/26, 3:33 PM`.
	pub label: String,
	pub hover: String,
}

/// Row times at `now_ms`: the label is local time; the hover gives the full local date and
/// offset with the UTC clock, then the sender's own claim in the same form.
pub fn row_times(zone: &LocalZone, now_ms: u64, received_ms: u64, sent_ms: u64) -> RowTimes {
	RowTimes {
		label: timefmt::format(zone, now_ms, received_ms),
		hover: tr!(
			"row-time-hover",
			local = zone.hover(received_ms),
			claimed = zone.hover(sent_ms)
		),
	}
}

/// A short, honest guess of what a file is, from its name and advisory type.
pub fn type_guess(name: &str, media_type: &str, executable_warning: bool) -> &'static str {
	if executable_warning {
		return tr!("type-program");
	}
	let ext = name
		.rsplit_once('.')
		.map_or("", |(_, e)| e)
		.to_ascii_lowercase();
	match (media_type.split('/').next().unwrap_or(""), ext.as_str()) {
		("image", _) => tr!("type-image"),
		("audio", _) => tr!("type-audio"),
		("video", _) => tr!("type-video"),
		("text", _) | (_, "txt" | "md" | "log" | "json" | "csv" | "toml" | "yaml" | "yml") => {
			tr!("type-text")
		}
		(_, "zip" | "tar" | "gz" | "7z" | "rar" | "xz" | "zst") => tr!("type-archive"),
		(_, "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx" | "ppt" | "pptx") => {
			tr!("type-document")
		}
		_ => tr!("type-unknown"),
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
		out.push(tr!("offer-warn-executable").to_owned());
	}
	if offer.size >= LARGE_FILE_BYTES {
		out.push(tr!(
			"offer-warn-large",
			size = format_size(required_space(offer.size))
		));
	} else if !offer.outgoing && offer.status == OfferStatus::Offered {
		out.push(tr!(
			"offer-warn-space",
			size = format_size(required_space(offer.size))
		));
	}
	if let Some(cap) = max_offer_mib.filter(|c| *c > 0)
		&& offer.size > u64::from(cap) * 1024 * 1024
	{
		out.push(tr!("offer-warn-cap", cap = cap));
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
		tr!("counter-bytes", used = self.used, limit = self.limit)
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
		return blocked(tr!("blocked-not-connected"), false);
	}
	if !daemon_enabled {
		return blocked(tr!("blocked-daemon-off"), false);
	}
	if !session.is_live() {
		return blocked(tr!("blocked-call-ended"), true);
	}
	if !session.perms.allow_messages {
		return blocked(tr!("blocked-messages-off"), false);
	}
	if session.history_full() {
		return blocked(tr!("blocked-history-full"), true);
	}
	None
}

/// Whether a file or image may be sent now.
pub fn files_blocked(session: &SessionView) -> Option<&'static str> {
	if !session.is_live() {
		Some(tr!("files-blocked-ended"))
	} else if !session.perms.allow_files {
		Some(tr!("files-blocked-off"))
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
		tr!("offer-you-offered")
	} else {
		tr!("offer-offered-to-you")
	};
	match offer.status {
		OfferStatus::Offered => who.to_owned(),
		OfferStatus::InProgress => tr!(
			"offer-transferring",
			percent = format!("{:.0}", progress(offer) * 100.0)
		),
		OfferStatus::Completed if offer.outgoing => tr!("offer-sent").to_owned(),
		OfferStatus::Completed => tr!("offer-saved").to_owned(),
		OfferStatus::Rejected => tr!("offer-declined").to_owned(),
		OfferStatus::Failed => tr!("offer-failed", reason = reason_word(&offer.failure)),
		OfferStatus::Cancelled => tr!("offer-cancelled").to_owned(),
		OfferStatus::Expired => tr!("offer-expired").to_owned(),
	}
}

fn reason_word(failure: &str) -> &str {
	if failure.is_empty() {
		tr!("offer-unknown-reason")
	} else {
		failure
	}
}

/// Text for a failed message delivery.
pub fn delivery_failure(reason: &str) -> String {
	match reason {
		"timeout" => tr!("delivery-timeout").to_owned(),
		"disabled" => tr!("delivery-disabled").to_owned(),
		"too_large" => tr!("delivery-too-large").to_owned(),
		"invalid" => tr!("delivery-invalid").to_owned(),
		"rate_limited" => tr!("delivery-rate-limited").to_owned(),
		"history_full" => tr!("delivery-history-full").to_owned(),
		"unsupported" => tr!("delivery-unsupported").to_owned(),
		"ended" => tr!("delivery-ended").to_owned(),
		_ => tr!("delivery-default").to_owned(),
	}
}

/// What to tell the user for a daemon problem code. Never includes content.
pub fn problem_text(code: &str) -> String {
	let text = match code {
		"MESSAGES_DISABLED" => tr!("problem-messages-disabled"),
		"SESSION_NOT_FOUND" => tr!("problem-session-not-found"),
		"SESSION_ENDED" => tr!("problem-session-ended"),
		"SESSION_PERMISSION_OFF" => tr!("problem-session-permission-off"),
		"PEER_UNAVAILABLE" => tr!("problem-peer-unavailable"),
		"TEXT_INVALID" => tr!("problem-text-invalid"),
		"TEXT_TOO_LONG" => tr!("problem-text-too-long"),
		"IMAGE_INVALID" | "IMAGE_UNSUPPORTED" => tr!("problem-image-invalid"),
		"IMAGE_TOO_LARGE" => tr!("problem-image-too-large"),
		"HISTORY_FULL" => tr!("problem-history-full"),
		"RATE_LIMITED" => tr!("problem-rate-limited"),
		"TOO_MANY_OFFERS" => tr!("problem-too-many-offers"),
		"MESSAGE_NOT_FOUND" | "MESSAGE_STATE" => tr!("problem-message-not-found"),
		"OFFER_NOT_FOUND" | "OFFER_STATE" => tr!("problem-offer-not-found"),
		"FILE_REFUSED" => tr!("problem-file-refused"),
		"FILE_TOO_LARGE" => tr!("problem-file-too-large"),
		"FILE_CHANGED" => tr!("problem-file-changed"),
		"TOO_MANY_TRANSFERS" => tr!("problem-too-many-transfers"),
		"NO_SPACE" => tr!("problem-no-space"),
		"DESTINATION_INVALID" => tr!("problem-destination-invalid"),
		"PREVIEW_UNAVAILABLE" => tr!("problem-preview-unavailable"),
		"EXPORT_TOO_LARGE" => tr!("problem-export-too-large"),
		"EXPORT_WRITE_FAILED" => tr!("problem-export-write-failed"),
		"INVALID_SETTING" => tr!("problem-invalid-setting"),
		"PermissionDenied" | "PERMISSION_DENIED" => tr!("problem-permission-denied"),
		_ => tr!("problem-default"),
	};
	text.to_owned()
}

/// The session of a call: its live one, else the most recently ended one.
/// `None` when the daemon holds no session for that call id (hex).
pub fn session_of_call<'a>(sessions: &'a [SessionView], call_id: &str) -> Option<&'a SessionView> {
	if call_id.is_empty() {
		return None;
	}
	let mut of_call = sessions.iter().filter(|s| s.call_id == call_id);
	let first = of_call.next()?;
	Some(of_call.fold(first, |best, s| {
		let better = (s.is_live(), s.ended_ms) > (best.is_live(), best.ended_ms);
		if better { s } else { best }
	}))
}

/// Whether the daemon still holds the transcript of a past call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
	/// It does, with this many messages.
	Yes(u64),
	/// It does not (messages are memory-only: a restart or an eviction drops them).
	No,
	/// The messaging feed is not live, so nothing can be said yet.
	Unknown,
}

pub fn transcript_kept(state: &MessagesState, call_id: &str) -> Kept {
	if state.feed != Feed::Live {
		return Kept::Unknown;
	}
	match session_of_call(&state.sessions, call_id) {
		Some(s) => Kept::Yes(s.message_count),
		None => Kept::No,
	}
}

/// Received messages after the last one the user saw (`seen` is the highest
/// ordinal shown; `None` when the transcript never was). Walks from the newest
/// entry and stops at the first seen one, so it stays cheap.
pub fn unread_count(entries: &[EntryView], seen: Option<u64>) -> usize {
	entries
		.iter()
		.rev()
		.take_while(|e| seen.is_none_or(|s| e.ordinal > s))
		.filter(|e| !e.outgoing)
		.count()
}

/// Used history as text, `1.2 MiB of 64.0 MiB`.
pub fn usage_label(session: &SessionView) -> String {
	tr!(
		"history-usage",
		used = format_size(session.used_bytes),
		limit = format_size(session.limit_bytes)
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
