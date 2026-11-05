//! What entries, offers and sessions look like in text and JSON.
//!
//! Received text is displayed exactly (the daemon already sanitized it); control
//! characters are replaced anyway so nothing can drive the terminal. Links are
//! shown, never opened: the safe (ASCII) host and any lookalike warning come from
//! the same parser the daemon uses.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::transcript_entry::Body;
use relay_api::pb::{
	DeliveryState, FileOfferBody, OfferState, SessionState, SessionSummary, TranscriptEntry,
};
use relay_io::localtime::LocalZone;
use relay_messages::markdown::{self, HostWarning};
use serde::Serialize;

use crate::commands::names::hex_id;

/// Replace control characters (keeping newlines when `multiline`) so output cannot move the cursor.
pub fn plain(text: &str, multiline: bool) -> String {
	text.chars()
		.map(|c| match c {
			'\n' if multiline => '\n',
			'\t' => ' ',
			c if c.is_control() => '\u{fffd}',
			c => c,
		})
		.collect()
}

/// Delivery state name.
pub fn delivery_name(raw: i32) -> &'static str {
	match DeliveryState::try_from(raw).unwrap_or(DeliveryState::Unspecified) {
		DeliveryState::Sending => "sending",
		DeliveryState::Delivered => "delivered",
		DeliveryState::Failed => "failed",
		DeliveryState::Unspecified => "unknown",
	}
}

/// Offer state name.
pub fn offer_state_name(raw: i32) -> &'static str {
	match OfferState::try_from(raw).unwrap_or(OfferState::Unspecified) {
		OfferState::Offered => "offered",
		OfferState::InProgress => "in-progress",
		OfferState::Completed => "completed",
		OfferState::Rejected => "rejected",
		OfferState::Failed => "failed",
		OfferState::Cancelled => "cancelled",
		OfferState::Expired => "expired",
		OfferState::Unspecified => "unknown",
	}
}

/// Human readable byte count.
pub fn size(bytes: u64) -> String {
	const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
	let mut value = bytes as f64;
	let mut unit = 0;
	while value >= 1024.0 && unit + 1 < UNITS.len() {
		value /= 1024.0;
		unit += 1;
	}
	if unit == 0 {
		format!("{bytes} B")
	} else {
		format!("{value:.1} {}", UNITS[unit])
	}
}

/// A session as listed.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
	pub session: String,
	pub call: String,
	pub peer: String,
	pub state: &'static str,
	pub allow_messages: bool,
	pub allow_files: bool,
	pub host: bool,
	pub messages: u64,
	pub used_bytes: u64,
	pub limit_bytes: u64,
}

impl SessionView {
	/// From the wire summary.
	pub fn new(s: &SessionSummary) -> Self {
		let permissions = s.permissions.unwrap_or_default();
		Self {
			session: hex_id(&s.session_id),
			call: hex_id(&s.call_id),
			peer: plain(&s.peer_name, false),
			state: if s.state == SessionState::Active as i32 {
				"active"
			} else {
				"ended"
			},
			allow_messages: permissions.allow_messages,
			allow_files: permissions.allow_files,
			host: s.local_is_host,
			messages: s.message_count,
			used_bytes: s.used_bytes,
			limit_bytes: s.limit_bytes,
		}
	}

	/// Whether the transcript budget is exhausted for practical purposes.
	pub fn budget_full(&self) -> bool {
		self.limit_bytes > 0 && self.used_bytes >= self.limit_bytes
	}
}

/// One fenced code block of a message, numbered across the session.
#[derive(Debug, Clone, Serialize)]
pub struct CodeBlockView {
	pub number: usize,
	pub language: Option<String>,
	pub lines: usize,
	pub closed: bool,
}

/// One link of a message. Never opened by the CLI.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
	pub url: String,
	/// The host as it will be dialled (ASCII); show this one when `warning` is set.
	pub host: String,
	pub secure: bool,
	pub warning: Option<&'static str>,
}

/// An inline image.
#[derive(Debug, Clone, Serialize)]
pub struct ImageView {
	pub media_type: String,
	pub width: u32,
	pub height: u32,
	pub frames: u32,
	pub bytes: usize,
}

/// A file offer.
#[derive(Debug, Clone, Serialize)]
pub struct OfferView {
	pub offer: String,
	pub name: String,
	pub size: u64,
	pub state: &'static str,
	pub transferred: u64,
	pub percent: u8,
	pub outgoing: bool,
	pub executable_warning: bool,
	pub preview_requested: bool,
	pub preview_approval_pending: bool,
	pub preview: Option<String>,
	pub saved_as: Option<String>,
	pub failure: Option<String>,
}

impl OfferView {
	/// From the wire offer.
	pub fn new(o: &FileOfferBody) -> Self {
		let percent = if o.size == 0 {
			if o.state == OfferState::Completed as i32 {
				100
			} else {
				0
			}
		} else {
			u8::try_from((u128::from(o.transferred_bytes.min(o.size)) * 100) / u128::from(o.size))
				.unwrap_or(100)
		};
		let some = |s: &str| (!s.is_empty()).then(|| plain(s, true));
		Self {
			offer: hex_id(&o.offer_id),
			name: plain(&o.name, false),
			size: o.size,
			state: offer_state_name(o.state),
			transferred: o.transferred_bytes,
			percent,
			outgoing: o.outgoing,
			executable_warning: o.executable_warning,
			preview_requested: o.preview_requested,
			preview_approval_pending: o.preview_approval_pending,
			preview: some(&o.preview),
			saved_as: some(&o.saved_as).map(|s| s.replace('\n', " ")),
			failure: some(&o.failure),
		}
	}

	/// Short id for tables.
	pub fn short(&self) -> &str {
		&self.offer[..self.offer.len().min(8)]
	}
}

/// A transcript entry with everything a reader needs.
#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
	pub message: u64,
	pub seq: u64,
	pub direction: &'static str,
	/// UTC instant; the only time in `--json`.
	pub time_unix_ms: u64,
	/// `HH:MM:SS` in the viewer's zone, for the text form only.
	#[serde(skip)]
	pub local_clock: String,
	pub delivery: &'static str,
	pub failure: Option<String>,
	pub pinned: bool,
	pub kind: &'static str,
	pub text: Option<String>,
	pub code_blocks: Vec<CodeBlockView>,
	pub links: Vec<LinkView>,
	pub image: Option<ImageView>,
	pub offer: Option<OfferView>,
}

fn warning_name(w: HostWarning) -> &'static str {
	match w {
		HostWarning::Idn => "international-domain",
		HostWarning::MixedScript => "mixed-script-lookalike",
		HostWarning::InvalidPunycode => "invalid-punycode",
	}
}

impl EntryView {
	/// Build the view; `blocks_before` is how many code blocks earlier entries of the
	/// session hold, so numbers are stable across the whole transcript.
	pub fn new(entry: &TranscriptEntry, blocks_before: usize, zone: &LocalZone) -> Self {
		let mut view = Self {
			message: entry.ordinal,
			seq: entry.seq,
			direction: if entry.outgoing { "out" } else { "in" },
			time_unix_ms: entry.received_at_unix_ms,
			local_clock: zone.clock(entry.received_at_unix_ms),
			delivery: delivery_name(entry.delivery),
			failure: Some(entry.failure.clone()).filter(|f| !f.is_empty()),
			pinned: entry.pinned,
			kind: "text",
			text: None,
			code_blocks: Vec::new(),
			links: Vec::new(),
			image: None,
			offer: None,
		};
		match &entry.body {
			Some(Body::Text(text)) => {
				let document = markdown::parse(text);
				view.code_blocks = document
					.code_blocks()
					.enumerate()
					.map(|(i, b)| CodeBlockView {
						number: blocks_before + i + 1,
						language: b.language.as_deref().map(|l| plain(l, false)),
						lines: b.text.lines().count(),
						closed: b.closed,
					})
					.collect();
				view.links = document
					.links()
					.into_iter()
					.map(|l| LinkView {
						url: plain(&l.url, false),
						host: l.host.ascii.clone(),
						secure: l.secure,
						warning: l.host.warning.map(warning_name),
					})
					.collect();
				view.text = Some(plain(text, true));
			}
			Some(Body::Image(image)) => {
				view.kind = "image";
				view.image = Some(ImageView {
					media_type: plain(&image.media_type, false),
					width: image.width,
					height: image.height,
					frames: image.frames,
					bytes: image.data.len(),
				});
			}
			Some(Body::File(offer)) => {
				view.kind = "offer";
				view.offer = Some(OfferView::new(offer));
			}
			None => view.kind = "empty",
		}
		view
	}

	/// Number of code blocks in the entry.
	pub fn block_count(&self) -> usize {
		self.code_blocks.len()
	}
}

impl Display for EntryView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"#{}  {}  {:<3}  {}",
			self.message, self.local_clock, self.direction, self.delivery
		)?;
		if let Some(reason) = &self.failure {
			write!(f, " ({reason})")?;
		}
		if self.pinned {
			f.write_str("  [pinned]")?;
		}
		if let Some(text) = &self.text {
			for line in text.lines() {
				write!(f, "\n    {line}")?;
			}
		}
		for block in &self.code_blocks {
			write!(
				f,
				"\n    [code {}: {}, {} line{}{}]",
				block.number,
				block.language.as_deref().unwrap_or("no language"),
				block.lines,
				if block.lines == 1 { "" } else { "s" },
				if block.closed { "" } else { ", not closed" }
			)?;
		}
		for link in &self.links {
			write!(f, "\n    [link {} host {}", link.url, link.host)?;
			if let Some(warning) = link.warning {
				write!(f, " WARNING {warning}: check the host before trusting it")?;
			}
			f.write_str(" (not opened)]")?;
		}
		if let Some(image) = &self.image {
			write!(
				f,
				"\n    [image {} {}x{} {}]",
				image.media_type,
				image.width,
				image.height,
				size(image.bytes as u64)
			)?;
		}
		if let Some(offer) = &self.offer {
			write!(f, "\n    {}", OfferLine(offer))?;
		}
		Ok(())
	}
}

/// One line describing an offer inside a transcript or an event.
pub struct OfferLine<'a>(pub &'a OfferView);

impl Display for OfferLine<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let o = self.0;
		write!(
			f,
			"[{} offer {} \"{}\" {} {}",
			if o.outgoing { "outgoing" } else { "incoming" },
			o.short(),
			o.name,
			size(o.size),
			o.state
		)?;
		if o.state == "in-progress" {
			write!(f, " {}%", o.percent)?;
		}
		if o.executable_warning {
			f.write_str(" WARNING: looks executable")?;
		}
		if o.preview_approval_pending {
			f.write_str(" preview requested by the peer")?;
		}
		if let Some(saved) = &o.saved_as {
			write!(f, " saved as \"{saved}\"")?;
		}
		if let Some(reason) = &o.failure {
			write!(f, " ({reason})")?;
		}
		f.write_str("]")
	}
}
