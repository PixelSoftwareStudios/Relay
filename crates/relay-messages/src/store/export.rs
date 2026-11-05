//! Versioned JSON export: the only thing that ever persists.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use super::SessionStore;
use crate::markdown;
use crate::message::{Delivery, Direction, FailReason, MessageBody, OfferStatus};

/// Schema identifier.
pub const EXPORT_SCHEMA: &str = "relay.session-export";
/// Current schema version.
pub const EXPORT_VERSION: u32 = 1;

/// Export options.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExportOptions {
	/// Embed image bytes as base64 (otherwise only references).
	pub embed_images: bool,
}

/// Exported peer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPeer {
	/// Display name.
	pub display_name: String,
	/// Fingerprint, hex.
	pub fingerprint: String,
}

/// One exported code block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportCode {
	/// Language label.
	pub language: Option<String>,
	/// Exact content.
	pub text: String,
}

/// Exported content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportContent {
	/// Text with its code blocks.
	Text {
		/// The message text.
		text: String,
		/// Parsed code blocks.
		code_blocks: Vec<ExportCode>,
	},
	/// Image.
	Image {
		/// Media type.
		media_type: String,
		/// Width.
		width: u32,
		/// Height.
		height: u32,
		/// Frames.
		frames: u32,
		/// Compressed length.
		bytes_len: u64,
		/// Base64 bytes when embedding was requested.
		data_base64: Option<String>,
	},
	/// File offer metadata.
	FileOffer {
		/// Offer id, hex.
		offer_id: String,
		/// Sanitized name.
		name: String,
		/// Size.
		size: u64,
		/// Media type guess.
		media_type: String,
		/// BLAKE3, hex.
		blake3: String,
		/// Status.
		status: OfferStatus,
		/// Approved preview.
		preview: Option<String>,
	},
}

/// One exported message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportMessage {
	/// Sender's sequence.
	pub seq: u64,
	/// Direction.
	pub direction: Direction,
	/// Sender's claim.
	pub sent_at_ms: u64,
	/// Local receive time.
	pub received_at_ms: u64,
	/// Delivery description.
	pub delivery: String,
	/// Pinned locally.
	pub pinned: bool,
	/// Content.
	pub content: ExportContent,
}

/// The export document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
	/// Always [`EXPORT_SCHEMA`].
	pub schema: String,
	/// Schema version.
	pub version: u32,
	/// Export time.
	pub exported_at_ms: u64,
	/// Session id, hex.
	pub session_id: String,
	/// Session start.
	pub started_at_ms: u64,
	/// Peer.
	pub peer: ExportPeer,
	/// Messages in transcript order.
	pub messages: Vec<ExportMessage>,
}

/// Export parsing failure.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
	/// Not valid JSON for this schema.
	#[error("invalid export: {0}")]
	Json(#[from] serde_json::Error),
	/// Wrong schema name or unsupported version.
	#[error("unsupported export schema or version")]
	Version,
}

impl Export {
	/// Parses and checks an export document.
	pub fn from_json(s: &str) -> Result<Self, ExportError> {
		let e: Self = serde_json::from_str(s)?;
		if e.schema != EXPORT_SCHEMA || e.version != EXPORT_VERSION {
			return Err(ExportError::Version);
		}
		Ok(e)
	}

	/// Decodes an embedded image back to bytes.
	pub fn decode_image(content: &ExportContent) -> Option<Vec<u8>> {
		match content {
			ExportContent::Image {
				data_base64: Some(d),
				..
			} => STANDARD.decode(d).ok(),
			_ => None,
		}
	}
}

fn delivery_text(d: Delivery) -> String {
	match d {
		Delivery::Sending => "sending".into(),
		Delivery::Delivered => "delivered".into(),
		Delivery::Failed(FailReason::Timeout) => "failed:timeout".into(),
		Delivery::Failed(FailReason::Rejected(r)) => format!("failed:{r:?}").to_lowercase(),
	}
}

impl SessionStore {
	/// Builds the export document.
	pub fn export(&self, opts: ExportOptions) -> Export {
		let messages = self
			.messages
			.iter()
			.map(|m| {
				let content = match &m.body {
					MessageBody::Text(t) => ExportContent::Text {
						text: t.clone(),
						code_blocks: markdown::parse(t)
							.code_blocks()
							.map(|c| ExportCode {
								language: c.language.clone(),
								text: c.text.clone(),
							})
							.collect(),
					},
					MessageBody::Image(i) => ExportContent::Image {
						media_type: i.info.format.media_type().to_owned(),
						width: i.info.width,
						height: i.info.height,
						frames: i.info.frames,
						bytes_len: i.bytes.len() as u64,
						data_base64: opts.embed_images.then(|| STANDARD.encode(&i.bytes)),
					},
					MessageBody::File(o) => ExportContent::FileOffer {
						offer_id: o.id.to_hex(),
						name: o.name.as_str().to_owned(),
						size: o.size,
						media_type: o.media_type.to_owned(),
						blake3: hex::encode(o.hash),
						status: o.status,
						preview: o.preview.clone(),
					},
				};
				ExportMessage {
					seq: m.id.seq,
					direction: m.direction,
					sent_at_ms: m.sent_at_ms,
					received_at_ms: m.received_at_ms,
					delivery: delivery_text(m.delivery),
					pinned: m.pinned,
					content,
				}
			})
			.collect();
		Export {
			schema: EXPORT_SCHEMA.to_owned(),
			version: EXPORT_VERSION,
			exported_at_ms: self.clock.wall_ms(),
			session_id: self.info.id.to_hex(),
			started_at_ms: self.info.started_at_ms,
			peer: ExportPeer {
				display_name: self.info.peer.display_name.clone(),
				fingerprint: self.info.peer.fingerprint.to_hex(),
			},
			messages,
		}
	}

	/// Export as pretty JSON text.
	pub fn export_json(&self, opts: ExportOptions) -> String {
		serde_json::to_string_pretty(&self.export(opts)).unwrap_or_default()
	}
}
