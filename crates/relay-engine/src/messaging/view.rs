//! Domain values to API messages.

use relay_api::pb;
use relay_messages::sanitize::looks_executable;
use relay_messages::{
	Delivery, FailReason, FileOffer, Message, MessageBody, MessageId, OfferStatus, Origin,
	RejectReason, SessionStore,
};

use super::session::{Meta, Xfer};

/// Lowercase label of a rejection.
pub fn reject_label(reason: RejectReason) -> &'static str {
	match reason {
		RejectReason::Disabled => "disabled",
		RejectReason::TooLarge => "too_large",
		RejectReason::Invalid => "invalid",
		RejectReason::RateLimited => "rate_limited",
		RejectReason::HistoryFull => "history_full",
		RejectReason::Unsupported => "unsupported",
		RejectReason::Ended => "ended",
	}
}

/// Delivery state and failure label.
pub fn delivery(d: Delivery) -> (pb::DeliveryState, &'static str) {
	match d {
		Delivery::Sending => (pb::DeliveryState::Sending, ""),
		Delivery::Delivered => (pb::DeliveryState::Delivered, ""),
		Delivery::Failed(FailReason::Timeout) => (pb::DeliveryState::Failed, "timeout"),
		Delivery::Failed(FailReason::Rejected(r)) => (pb::DeliveryState::Failed, reject_label(r)),
	}
}

fn offer_state(status: OfferStatus) -> pb::OfferState {
	match status {
		OfferStatus::Offered => pb::OfferState::Offered,
		OfferStatus::Rejected => pb::OfferState::Rejected,
		OfferStatus::InProgress => pb::OfferState::InProgress,
		OfferStatus::Completed => pb::OfferState::Completed,
		OfferStatus::Failed => pb::OfferState::Failed,
		OfferStatus::Cancelled => pb::OfferState::Cancelled,
		OfferStatus::Expired => pb::OfferState::Expired,
	}
}

/// A file offer with its transfer facts.
pub fn offer_body(offer: &FileOffer, outgoing: bool, xfer: Option<&Xfer>) -> pb::FileOfferBody {
	let (transferred, requested, pending, saved, failure) =
		xfer.map_or((0, false, false, "", ""), |x| {
			(
				if offer.status == OfferStatus::Completed {
					offer.size
				} else {
					x.transferred
				},
				x.preview_requested,
				x.preview_pending,
				x.saved_as.as_str(),
				x.failure.as_str(),
			)
		});
	pb::FileOfferBody {
		offer_id: super::wire::offer_bytes(offer.id).to_vec(),
		name: offer.name.as_str().to_owned(),
		size: offer.size,
		media_type: offer.media_type.to_owned(),
		blake3: offer.hash.to_vec(),
		state: offer_state(offer.status) as i32,
		transferred_bytes: transferred,
		preview: offer.preview.clone().unwrap_or_default(),
		preview_requested: requested,
		preview_approval_pending: pending,
		executable_warning: looks_executable(offer.name.as_str()),
		saved_as: saved.to_owned(),
		failure: failure.to_owned(),
		outgoing,
	}
}

/// A transcript entry.
pub fn entry(ordinal: u64, m: &Message, meta: &Meta) -> pb::TranscriptEntry {
	let (state, failure) = delivery(m.delivery);
	let outgoing = m.id.origin == Origin::Local;
	let body = match &m.body {
		MessageBody::Text(t) => pb::transcript_entry::Body::Text(t.clone()),
		MessageBody::Image(i) => pb::transcript_entry::Body::Image(pb::ImageBody {
			media_type: i.info.format.media_type().to_owned(),
			width: i.info.width,
			height: i.info.height,
			frames: i.info.frames,
			data: i.bytes.to_vec(),
		}),
		MessageBody::File(o) => {
			pb::transcript_entry::Body::File(offer_body(o, outgoing, meta.xfers.get(&o.id)))
		}
	};
	pb::TranscriptEntry {
		ordinal,
		outgoing,
		seq: m.id.seq,
		sent_at_unix_ms: m.sent_at_ms,
		received_at_unix_ms: m.received_at_ms,
		delivery: state as i32,
		failure: failure.to_owned(),
		pinned: m.pinned,
		body: Some(body),
	}
}

/// The ordinal of the message at transcript index `index`.
pub fn ordinal(meta: &Meta, index: usize) -> u64 {
	meta.base + index as u64 + 1
}

/// Position of a message in the transcript.
pub fn index_of(store: &SessionStore, id: MessageId) -> Option<usize> {
	store.messages().iter().position(|m| m.id == id)
}

/// Session summary.
pub fn summary(meta: &Meta, store: &SessionStore) -> pb::SessionSummary {
	let permissions = store.permissions();
	pb::SessionSummary {
		session_id: meta.id.0.to_vec(),
		call_id: meta.call.to_bytes().to_vec(),
		peer_id: meta.peer.as_bytes().to_vec(),
		peer_name: store.info().peer.display_name.clone(),
		state: if store.is_ended() {
			pb::SessionState::Ended
		} else {
			pb::SessionState::Active
		} as i32,
		permissions: Some(pb::SessionPermissions {
			allow_messages: permissions.allow_messages,
			allow_files: permissions.allow_files,
		}),
		local_is_host: meta.host,
		started_at_unix_ms: store.info().started_at_ms,
		ended_at_unix_ms: store.ended_at_ms().unwrap_or(0),
		message_count: store.messages().len() as u64,
		next_ordinal: meta.base + store.messages().len() as u64 + 1,
		used_bytes: store.used_bytes(),
		limit_bytes: store.limit_bytes(),
	}
}
