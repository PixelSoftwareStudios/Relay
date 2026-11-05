//! Inbound handling: permission, validation, ordering, rate limit, budget.

use super::SessionStore;
use crate::error::StoreError;
use crate::image;
use crate::limits::MAX_OFFERS_PER_SESSION;
use crate::message::*;
use crate::sanitize::sanitize_text;
use crate::transfer::offer_from_meta;

/// Result of a successful receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveOutcome {
	/// Stored as this message.
	Stored(MessageId),
	/// Sequence already seen; acknowledge again, nothing stored.
	Duplicate,
}

/// The ack a receiver should send for a receive result.
pub fn ack_status(r: &Result<ReceiveOutcome, StoreError>) -> AckStatus {
	match r {
		Ok(_) => AckStatus::Delivered,
		Err(e) => AckStatus::Rejected(e.reject_reason()),
	}
}

impl SessionStore {
	/// Handles a message from the peer. Both daemons enforce permissions, so a disabled
	/// kind is refused here regardless of what the sender's UI allowed.
	pub fn receive(&mut self, w: Wire) -> Result<ReceiveOutcome, StoreError> {
		if self.is_ended() {
			return Err(StoreError::Ended);
		}
		if w.seq == 0 {
			return Err(StoreError::BadSequence(0));
		}
		if w.seq <= self.last_remote_seq {
			return Ok(ReceiveOutcome::Duplicate);
		}
		// The sequence is consumed even when the content is refused, so gaps are legal.
		self.last_remote_seq = w.seq;
		let files = !matches!(w.payload, Payload::Text(_));
		let allowed = if files {
			self.permissions.allow_files
		} else {
			self.permissions.allow_messages
		};
		if !allowed {
			return Err(StoreError::Disabled);
		}
		let body = match &w.payload {
			Payload::Text(t) => MessageBody::Text(sanitize_text(t)?),
			Payload::Image(b) => {
				let info = image::inspect(b)?;
				MessageBody::Image(ImageBody {
					bytes: b.clone(),
					info,
				})
			}
			Payload::Offer(meta) => {
				if self.offer_count >= MAX_OFFERS_PER_SESSION {
					return Err(StoreError::TooManyOffers);
				}
				MessageBody::File(offer_from_meta(meta)?)
			}
		};
		self.check_budget(body.cost())?;
		let now = self.clock.mono_ms();
		if !self.in_rate.try_take(1, now) || !self.in_bytes.try_take(body.payload_bytes(), now) {
			return Err(StoreError::RateLimited);
		}
		if matches!(body, MessageBody::File(_)) && !self.offer_rate.try_take(1, now) {
			return Err(StoreError::RateLimited);
		}
		let id = MessageId {
			origin: Origin::Remote,
			seq: w.seq,
		};
		self.push(Message {
			id,
			direction: Direction::Incoming,
			sent_at_ms: w.sent_at_ms,
			received_at_ms: self.clock.wall_ms(),
			body,
			delivery: Delivery::Delivered,
			pinned: false,
		});
		Ok(ReceiveOutcome::Stored(id))
	}
}
