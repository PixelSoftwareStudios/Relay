//! Protobuf to view model. Parsing the markdown here keeps the UI thread free
//! of per-frame parsing; the daemon sends only the stored text.

use std::sync::Arc;

use relay_api::pb::{self, transcript_entry};
use relay_messages::markdown;

use super::model::{
	Body, Delivery, EntryView, ImageView, OfferStatus, OfferView, Perms, Phase, SessionView,
	SettingsView, TextView,
};

pub fn session(s: &pb::SessionSummary) -> SessionView {
	let phase = if s.state == pb::SessionState::Ended as i32 {
		Phase::Ended
	} else {
		Phase::Active
	};
	let perms = s.permissions.unwrap_or_default();
	SessionView {
		key: hex::encode(&s.session_id),
		id: s.session_id.clone(),
		call_id: hex::encode(&s.call_id),
		peer_name: s.peer_name.clone(),
		phase,
		perms: Perms {
			allow_messages: perms.allow_messages,
			allow_files: perms.allow_files,
		},
		local_is_host: s.local_is_host,
		started_ms: s.started_at_unix_ms,
		ended_ms: s.ended_at_unix_ms,
		message_count: s.message_count,
		next_ordinal: s.next_ordinal,
		used_bytes: s.used_bytes,
		limit_bytes: s.limit_bytes,
	}
}

pub fn settings(s: &pb::MessagingSettings) -> SettingsView {
	SettingsView {
		enabled: s.enabled,
		max_offer_size_mib: s.max_offer_size_mib,
		transfer_rate_limit_mbps: s.transfer_rate_limit_mbps,
	}
}

fn status(state: i32) -> OfferStatus {
	match pb::OfferState::try_from(state).unwrap_or(pb::OfferState::Unspecified) {
		pb::OfferState::InProgress => OfferStatus::InProgress,
		pb::OfferState::Completed => OfferStatus::Completed,
		pb::OfferState::Rejected => OfferStatus::Rejected,
		pb::OfferState::Failed => OfferStatus::Failed,
		pb::OfferState::Cancelled => OfferStatus::Cancelled,
		pb::OfferState::Expired => OfferStatus::Expired,
		pb::OfferState::Offered | pb::OfferState::Unspecified => OfferStatus::Offered,
	}
}

pub fn offer(o: &pb::FileOfferBody) -> OfferView {
	OfferView {
		offer_id: o.offer_id.clone(),
		name: o.name.clone(),
		size: o.size,
		media_type: o.media_type.clone(),
		blake3: o.blake3.clone(),
		status: status(o.state),
		transferred: o.transferred_bytes,
		preview: o.preview.clone(),
		preview_requested: o.preview_requested,
		preview_approval_pending: o.preview_approval_pending,
		executable_warning: o.executable_warning,
		saved_as: o.saved_as.clone(),
		failure: o.failure.clone(),
		outgoing: o.outgoing,
	}
}

pub fn delivery(state: i32) -> Delivery {
	match pb::DeliveryState::try_from(state).unwrap_or(pb::DeliveryState::Unspecified) {
		pb::DeliveryState::Sending => Delivery::Sending,
		pb::DeliveryState::Failed => Delivery::Failed,
		pb::DeliveryState::Delivered | pb::DeliveryState::Unspecified => Delivery::Delivered,
	}
}

/// `None` for an entry without a body (a protocol violation, ignored).
pub fn entry(e: &pb::TranscriptEntry) -> Option<EntryView> {
	let body = match e.body.as_ref()? {
		transcript_entry::Body::Text(text) => Body::Text(TextView {
			doc: Arc::new(markdown::parse(text)),
			text: text.clone(),
		}),
		transcript_entry::Body::Image(i) => Body::Image(ImageView {
			media_type: i.media_type.clone(),
			width: i.width,
			height: i.height,
			frames: i.frames,
			data: Arc::from(i.data.as_slice()),
		}),
		transcript_entry::Body::File(f) => Body::File(offer(f)),
	};
	Some(EntryView {
		ordinal: e.ordinal,
		outgoing: e.outgoing,
		seq: e.seq,
		received_ms: e.received_at_unix_ms,
		sent_ms: e.sent_at_unix_ms,
		delivery: delivery(e.delivery),
		failure: e.failure.clone(),
		pinned: e.pinned,
		body,
	})
}
