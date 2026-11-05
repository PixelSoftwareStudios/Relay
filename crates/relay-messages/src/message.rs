//! Message model and wire-level payloads (the crate's view of the draft `peer.proto`).

use std::fmt;
use std::sync::Arc;

use crate::error::RejectReason;
use crate::image::ImageInfo;
use crate::limits::MESSAGE_OVERHEAD_BYTES;
use crate::sanitize::SafeFileName;

/// Identifies one session; supplied by the engine.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub [u8; 16]);

/// Peer key fingerprint (BLAKE3 of the static key, as in relay-net).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerFingerprint(pub [u8; 32]);

fn hex(b: &[u8]) -> String {
	b.iter().map(|x| format!("{x:02x}")).collect()
}

impl SessionId {
	/// Lowercase hex.
	pub fn to_hex(&self) -> String {
		hex(&self.0)
	}
}
impl PeerFingerprint {
	/// Lowercase hex.
	pub fn to_hex(&self) -> String {
		hex(&self.0)
	}
}
impl fmt::Debug for SessionId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "SessionId({})", self.to_hex())
	}
}
impl fmt::Debug for PeerFingerprint {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "PeerFingerprint({})", &self.to_hex()[..16])
	}
}

/// The peer a session talks to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRef {
	/// Key fingerprint.
	pub fingerprint: PeerFingerprint,
	/// Sanitized display name.
	pub display_name: String,
}

/// Who numbered a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Origin {
	/// This side.
	Local,
	/// The peer.
	Remote,
}

/// Per-session message id: the numbering side and its sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageId {
	/// Numbering side.
	pub origin: Origin,
	/// Sender's sequence, starting at 1.
	pub seq: u64,
}

/// Message direction relative to this side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
	/// Sent by us.
	Outgoing,
	/// Received.
	Incoming,
}

/// Why a local message failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
	/// No ack after all retries.
	Timeout,
	/// The peer refused it.
	Rejected(RejectReason),
}

/// Delivery state of a message (always `Delivered` for incoming ones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
	/// Awaiting the peer's ack.
	Sending,
	/// Stored by the peer.
	Delivered,
	/// Not delivered.
	Failed(FailReason),
}

/// File offer identifier (random, from [`crate::IdSource`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OfferId(pub u128);

impl OfferId {
	/// Lowercase hex.
	pub fn to_hex(&self) -> String {
		format!("{:032x}", self.0)
	}
}
impl fmt::Debug for OfferId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "OfferId({})", self.to_hex())
	}
}

/// Status of a file offer as retained in the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfferStatus {
	/// Waiting for the receiver.
	Offered,
	/// Receiver said no.
	Rejected,
	/// Accepted or transferring.
	InProgress,
	/// Verified and visible at the receiver.
	Completed,
	/// Failed.
	Failed,
	/// Cancelled by either side or by the session ending mid-transfer.
	Cancelled,
	/// The session ended before it was accepted; metadata only.
	Expired,
}

impl OfferStatus {
	/// True when no further change can happen.
	pub fn is_terminal(self) -> bool {
		!matches!(self, Self::Offered | Self::InProgress)
	}
}

/// Retained file offer: metadata only, never bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct FileOffer {
	/// Identifier.
	pub id: OfferId,
	/// Sanitized name.
	pub name: SafeFileName,
	/// Size in bytes.
	pub size: u64,
	/// Advisory media type derived from the name.
	pub media_type: &'static str,
	/// BLAKE3 over the sender's snapshot.
	pub hash: [u8; 32],
	/// Approved text preview, if any.
	pub preview: Option<String>,
	/// Status.
	pub status: OfferStatus,
}

impl FileOffer {
	/// Download is only possible while the session is live and the offer is open.
	pub fn download_available(&self, session_active: bool) -> bool {
		session_active && self.status == OfferStatus::Offered
	}
}

/// An inline image: original compressed bytes plus header facts.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageBody {
	/// Original bytes.
	pub bytes: Arc<[u8]>,
	/// Header facts.
	pub info: ImageInfo,
}

/// Stored content.
#[derive(Clone, PartialEq, Eq)]
pub enum MessageBody {
	/// Sanitized text.
	Text(String),
	/// Inline image.
	Image(ImageBody),
	/// File offer.
	File(FileOffer),
}

impl fmt::Debug for MessageBody {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Text(t) => write!(f, "Text(<{} bytes redacted>)", t.len()),
			Self::Image(i) => write!(f, "Image(<{} bytes>)", i.bytes.len()),
			Self::File(o) => write!(f, "File(<{} bytes offered>)", o.size),
		}
	}
}

impl MessageBody {
	/// Payload bytes counted toward the budget.
	pub fn payload_bytes(&self) -> u64 {
		match self {
			Self::Text(t) => t.len() as u64,
			Self::Image(i) => i.bytes.len() as u64,
			Self::File(o) => {
				(o.name.as_str().len() + o.preview.as_ref().map_or(0, String::len) + 32 + 8) as u64
			}
		}
	}

	/// Budget cost including the overhead allowance.
	pub fn cost(&self) -> u64 {
		self.payload_bytes() + MESSAGE_OVERHEAD_BYTES
	}
}

/// One transcript entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
	/// Id.
	pub id: MessageId,
	/// Direction.
	pub direction: Direction,
	/// Sender's wall clock claim (untrusted, display only).
	pub sent_at_ms: u64,
	/// Local wall clock when composed or received.
	pub received_at_ms: u64,
	/// Content.
	pub body: MessageBody,
	/// Delivery state.
	pub delivery: Delivery,
	/// Local pin.
	pub pinned: bool,
}

/// Content-free description for logs.
pub struct Redacted<'a>(pub &'a Message);

impl fmt::Display for Redacted<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let kind = match self.0.body {
			MessageBody::Text(_) => "text",
			MessageBody::Image(_) => "image",
			MessageBody::File(_) => "file",
		};
		write!(
			f,
			"message {:?}#{} ({kind}, {} bytes)",
			self.0.id.origin,
			self.0.id.seq,
			self.0.body.payload_bytes()
		)
	}
}

/// Offer metadata as it travels (untrusted until validated).
#[derive(Clone, PartialEq, Eq)]
pub struct OfferMeta {
	/// Identifier chosen by the sender.
	pub id: OfferId,
	/// Raw name, re-sanitized by the receiver.
	pub name: String,
	/// Size.
	pub size: u64,
	/// BLAKE3 hash.
	pub hash: [u8; 32],
}

impl fmt::Debug for OfferMeta {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "OfferMeta({:?}, {} bytes)", self.id, self.size)
	}
}

/// Chat payload on the wire.
#[derive(Clone, PartialEq, Eq)]
pub enum Payload {
	/// Text.
	Text(String),
	/// Image bytes.
	Image(Arc<[u8]>),
	/// File offer.
	Offer(OfferMeta),
}

impl fmt::Debug for Payload {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Text(t) => write!(f, "Text(<{} bytes redacted>)", t.len()),
			Self::Image(b) => write!(f, "Image(<{} bytes>)", b.len()),
			Self::Offer(o) => write!(f, "Offer({o:?})"),
		}
	}
}

/// A numbered message on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
	/// Sender's sequence.
	pub seq: u64,
	/// Sender's wall clock claim.
	pub sent_at_ms: u64,
	/// Content.
	pub payload: Payload,
}

/// Ack status (draft `ChatAck`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckStatus {
	/// Stored.
	Delivered,
	/// Refused.
	Rejected(RejectReason),
}

/// Per-session permissions, enforced by both daemons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
	/// Text messages.
	pub allow_messages: bool,
	/// Images and file offers.
	pub allow_files: bool,
}

impl Default for Permissions {
	fn default() -> Self {
		Self {
			allow_messages: true,
			allow_files: true,
		}
	}
}
