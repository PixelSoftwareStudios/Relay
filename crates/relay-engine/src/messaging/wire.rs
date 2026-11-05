//! Conversions between `relay.peer.v1` session messages and `relay-messages` values.
//!
//! The control layer already validated structure (sizes, enum ranges); this only maps types.

use relay_messages::{
	AckStatus, OfferId, OfferMeta, Payload, RejectReason, Wire, sanitize::guess_media_type,
};
use relay_net::proto::session_message::Body;
use relay_net::proto::{
	CancelReason, ChatAck, ChatImage, ChatMessage, DataLinkGrant, FileOffer, OfferResponse,
	PermissionUpdate, PreviewReply, PreviewRequest, SessionMessage, TransferControl, chat_ack,
	chat_message, transfer_control,
};

/// Wraps a body.
pub fn message(body: Body) -> SessionMessage {
	SessionMessage { body: Some(body) }
}

/// The 16 wire bytes of an offer id.
pub fn offer_bytes(id: OfferId) -> [u8; 16] {
	id.0.to_be_bytes()
}

/// An offer id from its wire bytes.
pub fn offer_id(bytes: &[u8]) -> Option<OfferId> {
	let raw: [u8; 16] = bytes.try_into().ok()?;
	Some(OfferId(u128::from_be_bytes(raw)))
}

/// The wire form of a composed message.
pub fn to_wire(wire: &Wire) -> SessionMessage {
	match &wire.payload {
		Payload::Text(text) => message(Body::Message(ChatMessage {
			seq: wire.seq,
			sent_at_unix_ms: wire.sent_at_ms,
			content: Some(chat_message::Content::Text(text.clone())),
		})),
		Payload::Image(bytes) => message(Body::Message(ChatMessage {
			seq: wire.seq,
			sent_at_unix_ms: wire.sent_at_ms,
			content: Some(chat_message::Content::Image(ChatImage {
				data: bytes.to_vec(),
			})),
		})),
		Payload::Offer(meta) => message(Body::Offer(FileOffer {
			seq: wire.seq,
			sent_at_unix_ms: wire.sent_at_ms,
			offer_id: offer_bytes(meta.id).to_vec(),
			name: meta.name.clone(),
			size: meta.size,
			media_type: guess_media_type(&meta.name).to_owned(),
			blake3: meta.hash.to_vec(),
		})),
	}
}

/// What an inbound chat body carries for the store.
pub enum Inbound {
	/// A chat message or offer to store.
	Chat(Wire),
	/// An ack of one of our messages.
	Ack(u64, AckStatus),
}

/// Maps the store-bound bodies; `None` for bodies handled elsewhere.
pub fn inbound(body: &Body) -> Option<Inbound> {
	match body {
		Body::Message(m) => {
			let payload = match m.content.as_ref()? {
				chat_message::Content::Text(t) => Payload::Text(t.clone()),
				chat_message::Content::Image(i) => Payload::Image(i.data.clone().into()),
			};
			Some(Inbound::Chat(Wire {
				seq: m.seq,
				sent_at_ms: m.sent_at_unix_ms,
				payload,
			}))
		}
		Body::Offer(o) => {
			let hash: [u8; 32] = o.blake3.as_slice().try_into().ok()?;
			Some(Inbound::Chat(Wire {
				seq: o.seq,
				sent_at_ms: o.sent_at_unix_ms,
				payload: Payload::Offer(OfferMeta {
					id: offer_id(&o.offer_id)?,
					name: o.name.clone(),
					size: o.size,
					hash,
				}),
			}))
		}
		Body::Ack(a) => {
			let status = match chat_ack::Status::try_from(a.status).ok()? {
				chat_ack::Status::Delivered => AckStatus::Delivered,
				chat_ack::Status::Disabled => AckStatus::Rejected(RejectReason::Disabled),
				chat_ack::Status::TooLarge => AckStatus::Rejected(RejectReason::TooLarge),
				chat_ack::Status::Invalid => AckStatus::Rejected(RejectReason::Invalid),
				chat_ack::Status::RateLimited => AckStatus::Rejected(RejectReason::RateLimited),
				chat_ack::Status::HistoryFull => AckStatus::Rejected(RejectReason::HistoryFull),
				chat_ack::Status::Unsupported => AckStatus::Rejected(RejectReason::Unsupported),
				chat_ack::Status::Ended => AckStatus::Rejected(RejectReason::Ended),
			};
			Some(Inbound::Ack(a.seq, status))
		}
		_ => None,
	}
}

/// The sequence number carried by a chat body, to answer even when it is refused.
pub fn chat_seq(body: &Body) -> Option<u64> {
	match body {
		Body::Message(m) => Some(m.seq),
		Body::Offer(o) => Some(o.seq),
		_ => None,
	}
}

/// An ack message.
pub fn ack(seq: u64, status: AckStatus) -> SessionMessage {
	let status = match status {
		AckStatus::Delivered => chat_ack::Status::Delivered,
		AckStatus::Rejected(reason) => match reason {
			RejectReason::Disabled => chat_ack::Status::Disabled,
			RejectReason::TooLarge => chat_ack::Status::TooLarge,
			RejectReason::Invalid => chat_ack::Status::Invalid,
			RejectReason::RateLimited => chat_ack::Status::RateLimited,
			RejectReason::HistoryFull => chat_ack::Status::HistoryFull,
			RejectReason::Unsupported => chat_ack::Status::Unsupported,
			RejectReason::Ended => chat_ack::Status::Ended,
		},
	};
	message(Body::Ack(ChatAck {
		seq,
		status: status as i32,
	}))
}

/// The receiver's answer to an offer.
pub fn offer_response(id: OfferId, accepted: bool, reason: &str) -> SessionMessage {
	message(Body::OfferResponse(OfferResponse {
		offer_id: offer_bytes(id).to_vec(),
		accepted,
		reason: reason.chars().take(32).collect(),
	}))
}

/// A preview request.
pub fn preview_request(id: OfferId) -> SessionMessage {
	message(Body::PreviewRequest(PreviewRequest {
		offer_id: offer_bytes(id).to_vec(),
	}))
}

/// A preview reply; a refusal carries no text.
pub fn preview_reply(id: OfferId, text: Option<String>) -> SessionMessage {
	message(Body::PreviewReply(PreviewReply {
		offer_id: offer_bytes(id).to_vec(),
		approved: text.is_some(),
		text: text.unwrap_or_default(),
	}))
}

/// The grant that tells the sender how to dial the data link.
pub fn data_link(id: OfferId, token: &[u8; 32], port: u16, ttl_ms: u32) -> SessionMessage {
	message(Body::Transfer(TransferControl {
		offer_id: offer_bytes(id).to_vec(),
		op: Some(transfer_control::Op::DataLink(DataLinkGrant {
			token: token.to_vec(),
			port: u32::from(port),
			ttl_ms,
		})),
	}))
}

/// A cancel notice on the control link.
pub fn cancel(id: OfferId, reason: CancelReason) -> SessionMessage {
	message(Body::Transfer(TransferControl {
		offer_id: offer_bytes(id).to_vec(),
		op: Some(transfer_control::Op::Cancel(reason as i32)),
	}))
}

/// The host's permission push.
pub fn permissions(allow_messages: bool, allow_files: bool) -> SessionMessage {
	message(Body::Permissions(PermissionUpdate {
		allow_messages,
		allow_files,
	}))
}
