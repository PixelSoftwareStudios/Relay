//! Local sending: validate, budget, rate-limit, number, store.

use std::sync::Arc;

use super::SessionStore;
use crate::error::StoreError;
use crate::image;
use crate::limits::MAX_OFFERS_PER_SESSION;
use crate::message::*;
use crate::sanitize::sanitize_text;
use crate::transfer::{offer_from_meta, offer_to_meta};

impl SessionStore {
	fn guard_send(&self, files: bool) -> Result<(), StoreError> {
		if self.is_ended() {
			return Err(StoreError::Ended);
		}
		let allowed = if files {
			self.permissions.allow_files
		} else {
			self.permissions.allow_messages
		};
		if allowed {
			Ok(())
		} else {
			Err(StoreError::Disabled)
		}
	}

	fn commit_local(
		&mut self,
		body: MessageBody,
		payload: impl FnOnce(&MessageBody) -> Payload,
	) -> Wire {
		let seq = self.next_seq;
		self.next_seq += 1;
		let now_wall = self.clock.wall_ms();
		let wire = Wire {
			seq,
			sent_at_ms: now_wall,
			payload: payload(&body),
		};
		self.push(Message {
			id: MessageId {
				origin: Origin::Local,
				seq,
			},
			direction: Direction::Outgoing,
			sent_at_ms: now_wall,
			received_at_ms: now_wall,
			body,
			delivery: Delivery::Sending,
			pinned: false,
		});
		self.outbox.sent(seq, self.clock.mono_ms());
		wire
	}

	/// Composes a text message.
	pub fn compose_text(&mut self, text: &str) -> Result<Wire, StoreError> {
		self.guard_send(false)?;
		let text = sanitize_text(text)?;
		let body = MessageBody::Text(text);
		self.check_budget(body.cost())?;
		if !self.out_rate.try_take(1, self.clock.mono_ms()) {
			return Err(StoreError::RateLimited);
		}
		Ok(self.commit_local(body, |b| match b {
			MessageBody::Text(t) => Payload::Text(t.clone()),
			_ => unreachable!("text body"),
		}))
	}

	/// Composes an inline image. Header is validated; bytes are kept as given.
	pub fn compose_image(&mut self, bytes: Arc<[u8]>) -> Result<Wire, StoreError> {
		self.guard_send(true)?;
		let info = image::inspect(&bytes)?;
		let body = MessageBody::Image(ImageBody { bytes, info });
		self.check_budget(body.cost())?;
		if !self.out_rate.try_take(1, self.clock.mono_ms()) {
			return Err(StoreError::RateLimited);
		}
		Ok(self.commit_local(body, |b| match b {
			MessageBody::Image(i) => Payload::Image(i.bytes.clone()),
			_ => unreachable!("image body"),
		}))
	}

	/// Composes a file offer (metadata only).
	pub fn compose_offer(&mut self, meta: OfferMeta) -> Result<Wire, StoreError> {
		self.guard_send(true)?;
		if self.offer_count >= MAX_OFFERS_PER_SESSION {
			return Err(StoreError::TooManyOffers);
		}
		let offer = offer_from_meta(&meta)?;
		let body = MessageBody::File(offer);
		self.check_budget(body.cost())?;
		let now = self.clock.mono_ms();
		if !self.offer_rate.try_take(1, now) {
			return Err(StoreError::RateLimited);
		}
		Ok(self.commit_local(body, |b| match b {
			MessageBody::File(o) => Payload::Offer(offer_to_meta(o)),
			_ => unreachable!("file body"),
		}))
	}

	/// Rebuilds the wire form of a stored local message (for resends).
	pub(crate) fn wire_for(&self, seq: u64) -> Option<Wire> {
		let m = self.get(MessageId {
			origin: Origin::Local,
			seq,
		})?;
		let payload = match &m.body {
			MessageBody::Text(t) => Payload::Text(t.clone()),
			MessageBody::Image(i) => Payload::Image(i.bytes.clone()),
			MessageBody::File(o) => Payload::Offer(offer_to_meta(o)),
		};
		Some(Wire {
			seq,
			sent_at_ms: m.sent_at_ms,
			payload,
		})
	}
}
