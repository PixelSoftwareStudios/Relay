//! Validation of `SessionMessage` bodies and the per-link flood guard.
//!
//! Structure only: sizes, lengths and enum ranges. Content policy (character classes,
//! permissions, history budget, per-sender limits) belongs to `relay-messages`, which answers
//! with a `ChatAck` instead of tearing the link down. Limits come from `relay_messages::limits`.

use relay_messages::limits::{MAX_FILE_BYTES, MAX_TEXT_BYTES};

use super::ValidationError;
use crate::proto::session_message::Body;
use crate::proto::{
	CancelReason, ChatAck, ChatMessage, DataLinkGrant, FileOffer, OfferResponse, PreviewReply,
	PreviewRequest, SessionMessage, TransferControl, TransferResult, chat_ack, chat_message,
	transfer_control,
};

/// Offer, preview and transfer identifiers are 128-bit.
pub const OFFER_ID_BYTES: usize = 16;
/// BLAKE3 digest length.
pub const BLAKE3_BYTES: usize = 32;
/// One-time data-link token length.
pub const TRANSFER_TOKEN_BYTES: usize = 32;
/// Largest inline image: a control record is 16 KiB and must also hold the envelope.
pub const MAX_INLINE_IMAGE_BYTES: usize = 15 * 1024;
/// Largest text preview on the wire (same reason).
pub const MAX_PREVIEW_WIRE_BYTES: usize = 15 * 1024;
/// Longest raw file name on the wire; honest senders send at most 120 sanitized bytes.
pub const MAX_OFFER_NAME_BYTES: usize = 255;
/// Longest advisory media type.
pub const MAX_MEDIA_TYPE_BYTES: usize = 127;
/// Longest refusal reason.
pub const MAX_REASON_BYTES: usize = 64;
/// Longest advisory token validity.
pub const MAX_GRANT_TTL_MS: u32 = 120_000;

fn fail(reason: &'static str) -> ValidationError {
	ValidationError::new(reason)
}

fn offer_id(id: &[u8]) -> Result<(), ValidationError> {
	if id.len() == OFFER_ID_BYTES {
		Ok(())
	} else {
		Err(fail("offer id must be 16 bytes"))
	}
}

fn seq(value: u64) -> Result<(), ValidationError> {
	if value == 0 {
		Err(fail("sequence numbers start at 1"))
	} else {
		Ok(())
	}
}

/// Checks one session message; used on send and on receive.
pub fn validate(message: &SessionMessage) -> Result<(), ValidationError> {
	match message
		.body
		.as_ref()
		.ok_or_else(|| fail("session message has no body"))?
	{
		Body::Message(m) => chat(m),
		Body::Ack(m) => ack(m),
		Body::Offer(m) => offer(m),
		Body::OfferResponse(m) => response(m),
		Body::PreviewRequest(PreviewRequest { offer_id: id }) => offer_id(id),
		Body::PreviewReply(m) => preview(m),
		Body::Transfer(m) => transfer(m),
		Body::Permissions(_) => Ok(()),
	}
}

fn chat(m: &ChatMessage) -> Result<(), ValidationError> {
	seq(m.seq)?;
	match m
		.content
		.as_ref()
		.ok_or_else(|| fail("chat message has no content"))?
	{
		chat_message::Content::Text(text) => {
			if text.is_empty() {
				return Err(fail("empty text"));
			}
			if text.len() > MAX_TEXT_BYTES {
				return Err(fail("text too long"));
			}
		}
		chat_message::Content::Image(image) => {
			if image.data.is_empty() {
				return Err(fail("empty image"));
			}
			if image.data.len() > MAX_INLINE_IMAGE_BYTES {
				return Err(fail("inline image too large"));
			}
		}
	}
	Ok(())
}

fn ack(m: &ChatAck) -> Result<(), ValidationError> {
	seq(m.seq)?;
	chat_ack::Status::try_from(m.status)
		.map(|_| ())
		.map_err(|_| fail("unknown ack status"))
}

fn offer(m: &FileOffer) -> Result<(), ValidationError> {
	seq(m.seq)?;
	offer_id(&m.offer_id)?;
	if m.name.is_empty() || m.name.len() > MAX_OFFER_NAME_BYTES {
		return Err(fail("offer name length out of bounds"));
	}
	if m.size > MAX_FILE_BYTES {
		return Err(fail("offered file too large"));
	}
	if m.media_type.len() > MAX_MEDIA_TYPE_BYTES {
		return Err(fail("media type too long"));
	}
	if m.blake3.len() != BLAKE3_BYTES {
		return Err(fail("offer hash must be 32 bytes"));
	}
	Ok(())
}

fn response(m: &OfferResponse) -> Result<(), ValidationError> {
	offer_id(&m.offer_id)?;
	if m.reason.len() > MAX_REASON_BYTES {
		return Err(fail("reason too long"));
	}
	Ok(())
}

fn preview(m: &PreviewReply) -> Result<(), ValidationError> {
	offer_id(&m.offer_id)?;
	if m.text.len() > MAX_PREVIEW_WIRE_BYTES {
		return Err(fail("preview too large"));
	}
	if !m.approved && !m.text.is_empty() {
		return Err(fail("refused preview carries text"));
	}
	Ok(())
}

fn transfer(m: &TransferControl) -> Result<(), ValidationError> {
	offer_id(&m.offer_id)?;
	match m
		.op
		.as_ref()
		.ok_or_else(|| fail("transfer control has no operation"))?
	{
		transfer_control::Op::DataLink(grant) => grant_shape(grant),
		transfer_control::Op::Result(v) => TransferResult::try_from(*v)
			.map(|_| ())
			.map_err(|_| fail("unknown transfer result")),
		transfer_control::Op::Cancel(v) => CancelReason::try_from(*v)
			.map(|_| ())
			.map_err(|_| fail("unknown cancel reason")),
	}
}

fn grant_shape(g: &DataLinkGrant) -> Result<(), ValidationError> {
	if g.token.len() != TRANSFER_TOKEN_BYTES {
		return Err(fail("transfer token must be 32 bytes"));
	}
	if g.port == 0 || g.port > u32::from(u16::MAX) {
		return Err(fail("data link port out of range"));
	}
	if g.ttl_ms == 0 || g.ttl_ms > MAX_GRANT_TTL_MS {
		return Err(fail("data link ttl out of range"));
	}
	Ok(())
}
