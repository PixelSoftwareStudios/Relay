//! View types of session messaging. Plain data built from the daemon's
//! protobuf by [`convert`](super::convert). Message text, file names and
//! hashes never appear in `Debug` output.

use std::fmt;
use std::sync::Arc;

use relay_messages::markdown::Document;

/// Hex of a 16-byte session id.
pub type SessionKey = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
	Sending,
	Delivered,
	Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	Active,
	Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Perms {
	pub allow_messages: bool,
	pub allow_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
	pub key: SessionKey,
	pub id: Vec<u8>,
	/// Hex of the call id; empty when unknown.
	pub call_id: String,
	pub peer_name: String,
	pub phase: Phase,
	pub perms: Perms,
	/// This side owns the permission toggles and pushes them to the guest.
	pub local_is_host: bool,
	pub started_ms: u64,
	pub ended_ms: u64,
	pub message_count: u64,
	pub next_ordinal: u64,
	pub used_bytes: u64,
	pub limit_bytes: u64,
}

impl SessionView {
	pub fn is_live(&self) -> bool {
		self.phase == Phase::Active
	}

	/// The session history has no room for another message.
	pub fn history_full(&self) -> bool {
		self.limit_bytes > 0 && self.used_bytes >= self.limit_bytes
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferStatus {
	Offered,
	InProgress,
	Completed,
	Rejected,
	Failed,
	Cancelled,
	Expired,
}

impl OfferStatus {
	pub fn is_final(self) -> bool {
		!matches!(self, Self::Offered | Self::InProgress)
	}
}

#[derive(Clone, PartialEq, Eq)]
pub struct OfferView {
	pub offer_id: Vec<u8>,
	pub name: String,
	pub size: u64,
	pub media_type: String,
	pub blake3: Vec<u8>,
	pub status: OfferStatus,
	pub transferred: u64,
	pub preview: String,
	pub preview_requested: bool,
	pub preview_approval_pending: bool,
	pub executable_warning: bool,
	pub saved_as: String,
	pub failure: String,
	pub outgoing: bool,
}

impl fmt::Debug for OfferView {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"OfferView({:?}, {} bytes, <redacted>)",
			self.status, self.size
		)
	}
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImageView {
	pub media_type: String,
	pub width: u32,
	pub height: u32,
	pub frames: u32,
	pub data: Arc<[u8]>,
}

impl fmt::Debug for ImageView {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ImageView({}x{}, <redacted>)", self.width, self.height)
	}
}

#[derive(Clone, PartialEq, Eq)]
pub struct TextView {
	pub text: String,
	/// Safe markdown parsed once when the entry arrives (off the UI thread).
	pub doc: Arc<Document>,
}

impl fmt::Debug for TextView {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "TextView(<redacted>, {} bytes)", self.text.len())
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
	Text(TextView),
	Image(ImageView),
	File(OfferView),
}

#[derive(Clone, PartialEq, Eq)]
pub struct EntryView {
	pub ordinal: u64,
	pub outgoing: bool,
	pub seq: u64,
	/// This daemon's clock; what is shown.
	pub received_ms: u64,
	/// The sender's claim; shown only on hover.
	pub sent_ms: u64,
	pub delivery: Delivery,
	pub failure: String,
	pub pinned: bool,
	pub body: Body,
}

impl fmt::Debug for EntryView {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("EntryView")
			.field("ordinal", &self.ordinal)
			.field("outgoing", &self.outgoing)
			.field("seq", &self.seq)
			.field("delivery", &self.delivery)
			.field("body", &self.body)
			.finish()
	}
}

impl EntryView {
	/// The exact stored text, for copy and search.
	pub fn text(&self) -> Option<&str> {
		match &self.body {
			Body::Text(t) => Some(&t.text),
			_ => None,
		}
	}

	/// Case-insensitive substring match over text, code and file names.
	pub fn matches(&self, needle_lower: &str) -> bool {
		if needle_lower.is_empty() {
			return true;
		}
		let hay = match &self.body {
			Body::Text(t) => t.text.to_lowercase(),
			Body::File(o) => format!("{} {}", o.name, o.preview).to_lowercase(),
			Body::Image(_) => return false,
		};
		hay.contains(needle_lower)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsView {
	pub enabled: bool,
	pub max_offer_size_mib: u32,
	pub transfer_rate_limit_mbps: u32,
}
