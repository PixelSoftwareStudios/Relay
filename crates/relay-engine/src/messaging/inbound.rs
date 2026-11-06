//! Session traffic from a peer. The peer is the authenticated owner of the link the message
//! arrived on; a message can only reach the session that peer is in.

use relay_api::pb::{self, session_event};
use relay_core::PeerId;
use relay_messages::store::ReceiveOutcome;
use relay_messages::{AckStatus, MessageBody, OfferStatus, Payload, RejectReason, SessionId};
use relay_net::proto::session_message::Body;
use relay_net::proto::{
	CancelReason, OfferResponse, PermissionUpdate, PreviewReply, SessionMessage, TransferControl,
	transfer_control,
};

use super::hub::Hub;
use super::session::{Xfer, XferCmd};
use super::wire::{self, Inbound};
use crate::peers::SessionEvent;

/// Acks sent to peers that talk while no session exists, per second.
const STRAY_PER_SEC: u64 = 4;
const STRAY_BURST: u64 = 8;

impl Hub {
	/// Entry for everything the peer layer reports.
	pub(super) fn on_peer(&mut self, event: SessionEvent) {
		match event {
			SessionEvent::LinkUp { peer, remote_ip } => {
				self.links.insert(peer, remote_ip);
			}
			SessionEvent::LinkDown { peer } => {
				self.links.remove(&peer);
			}
			SessionEvent::Message { peer, message } => self.on_message(peer, *message),
		}
	}

	fn on_message(&mut self, peer: PeerId, message: SessionMessage) {
		let Some(body) = message.body else {
			return;
		};
		let Some(&sid) = self.by_peer.get(&peer) else {
			self.stray_message(peer, &body);
			return;
		};
		match &body {
			Body::Message(_) | Body::Offer(_) => self.on_chat(sid, peer, &body),
			Body::Ack(_) => self.on_ack(sid, &body),
			Body::OfferResponse(r) => self.on_offer_response(sid, peer, r),
			Body::PreviewRequest(r) => self.on_preview_request(sid, &r.offer_id),
			Body::PreviewReply(r) => self.on_preview_reply(sid, r),
			Body::Transfer(t) => self.on_transfer_control(sid, peer, t),
			Body::Permissions(p) => self.on_permissions(sid, p),
		}
	}

	/// A peer with no session sent chat traffic: tell it so (bounded) and store nothing.
	fn stray_message(&mut self, peer: PeerId, body: &Body) {
		let Some(seq) = wire::chat_seq(body) else {
			return;
		};
		let now = self.mono_ms();
		let bucket = self.stray.entry(peer).or_insert_with(|| {
			relay_messages::rate::TokenBucket::new(STRAY_PER_SEC, STRAY_BURST, now)
		});
		if bucket.try_take(1, now) {
			self.send(
				peer,
				wire::ack(seq, AckStatus::Rejected(RejectReason::Ended)),
			);
		}
		if self.stray.len() > 256 {
			self.stray.clear();
		}
	}

	pub(super) fn mono_ms(&self) -> u64 {
		relay_messages::Clock::mono_ms(&*self.clock)
	}

	fn on_chat(&mut self, sid: SessionId, peer: PeerId, body: &Body) {
		let Some(seq) = wire::chat_seq(body) else {
			return;
		};
		let reject =
			|hub: &Hub, reason| hub.send(peer, wire::ack(seq, AckStatus::Rejected(reason)));
		let config = self.cfg();
		if !config.enabled {
			reject(self, RejectReason::Disabled);
			return;
		}
		let Some(Inbound::Chat(chat)) = wire::inbound(body) else {
			reject(self, RejectReason::Invalid);
			return;
		};
		if let Payload::Offer(meta) = &chat.payload
			&& meta.size > config.max_offer_bytes()
		{
			reject(self, RejectReason::TooLarge);
			return;
		}
		let is_offer = matches!(chat.payload, Payload::Offer(_));
		let Some(store) = self.history.get_mut(sid) else {
			return;
		};
		let result = store.receive(chat);
		let ack = relay_messages::store::ack_status(&result);
		self.send(peer, wire::ack(seq, ack));
		if matches!(result, Ok(ReceiveOutcome::Stored(_))) {
			if is_offer
				&& let Some(MessageBody::File(offer)) = self
					.history
					.get(sid)
					.and_then(|s| s.messages().last())
					.map(|m| m.body.clone())
				&& let Some(meta) = self.sessions.get_mut(&sid)
			{
				meta.xfers.insert(offer.id, Xfer::new(false));
			}
			self.emit_last_entry(sid);
		}
	}

	fn on_ack(&mut self, sid: SessionId, body: &Body) {
		let Some(Inbound::Ack(seq, status)) = wire::inbound(body) else {
			return;
		};
		let Some(store) = self.history.get_mut(sid) else {
			return;
		};
		if store.on_ack(seq, status).is_err() {
			return;
		}
		self.emit_delivery(sid, seq);
		let offer = self
			.history
			.get(sid)
			.and_then(|s| {
				s.get(relay_messages::MessageId {
					origin: relay_messages::Origin::Local,
					seq,
				})
			})
			.and_then(|m| match &m.body {
				MessageBody::File(o) => Some(o.id),
				_ => None,
			});
		if let Some(offer) = offer {
			self.emit_offer(sid, offer);
		}
	}

	/// The receiver answered one of our offers.
	fn on_offer_response(&mut self, sid: SessionId, peer: PeerId, r: &OfferResponse) {
		let Some(id) = wire::offer_id(&r.offer_id) else {
			return;
		};
		let Some(meta) = self.sessions.get(&sid) else {
			return;
		};
		if !meta.xfers.get(&id).is_some_and(|x| x.outgoing)
			|| self.offer_status(sid, id) != Some(OfferStatus::Offered)
		{
			return;
		}
		if !r.accepted {
			self.set_offer(sid, id, OfferStatus::Rejected);
			return;
		}
		let files = self
			.history
			.get(sid)
			.is_some_and(|s| s.permissions().allow_files);
		if !files {
			self.send(peer, wire::cancel(id, CancelReason::CancelDisabled));
			self.fail_offer(sid, id, OfferStatus::Cancelled, "disabled");
			return;
		}
		if self.active_transfers(sid) >= self.env.limits.transfers {
			self.send(peer, wire::cancel(id, CancelReason::CancelError));
			self.fail_offer(sid, id, OfferStatus::Cancelled, "too_many_transfers");
			return;
		}
		if let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
		{
			x.accepted = true;
			x.active = true;
		}
		self.set_offer(sid, id, OfferStatus::InProgress);
		self.expect_grant(sid, id);
	}

	fn on_preview_request(&mut self, sid: SessionId, raw: &[u8]) {
		let Some(id) = wire::offer_id(raw) else {
			return;
		};
		let files = self
			.history
			.get(sid)
			.is_some_and(|s| s.permissions().allow_files);
		let open = self.offer_status(sid, id) == Some(OfferStatus::Offered);
		if let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
			&& x.outgoing
			&& files && open
		{
			x.preview_pending = true;
			self.emit_offer(sid, id);
		}
	}

	fn on_preview_reply(&mut self, sid: SessionId, r: &PreviewReply) {
		let Some(id) = wire::offer_id(&r.offer_id) else {
			return;
		};
		let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
		else {
			return;
		};
		if x.outgoing || !std::mem::take(&mut x.preview_requested) {
			return;
		}
		if r.approved
			&& let Some(store) = self.history.get_mut(sid)
		{
			// The store validates length, lines and characters again.
			let _ = store.attach_preview(id, &r.text);
		}
		self.emit_offer(sid, id);
	}

	fn on_transfer_control(&mut self, sid: SessionId, peer: PeerId, t: &TransferControl) {
		let Some(id) = wire::offer_id(&t.offer_id) else {
			return;
		};
		match t.op.as_ref() {
			Some(transfer_control::Op::DataLink(grant)) => {
				let Ok(token) = <[u8; 32]>::try_from(grant.token.as_slice()) else {
					return;
				};
				let Ok(port) = u16::try_from(grant.port) else {
					return;
				};
				self.on_grant(sid, peer, id, token, port);
			}
			Some(transfer_control::Op::Cancel(_)) => self.on_peer_cancel(sid, id),
			// The data link carries the authoritative result.
			Some(transfer_control::Op::Result(_)) | None => {}
		}
	}

	fn on_peer_cancel(&mut self, sid: SessionId, id: relay_messages::OfferId) {
		let task = self
			.sessions
			.get(&sid)
			.and_then(|m| m.xfers.get(&id))
			.and_then(|x| x.task.clone());
		if let Some(task) = task {
			let _ = task.try_send(XferCmd::PeerCancel);
			return;
		}
		if self.offer_status(sid, id) == Some(OfferStatus::Offered)
			|| self
				.sessions
				.get(&sid)
				.and_then(|m| m.xfers.get(&id))
				.is_some_and(|x| x.accepted && x.task.is_none())
		{
			self.fail_offer(sid, id, OfferStatus::Cancelled, "cancelled_by_peer");
		}
	}

	/// The host's permission push; only the host may send it.
	fn on_permissions(&mut self, sid: SessionId, p: &PermissionUpdate) {
		if self.sessions.get(&sid).is_none_or(|m| m.host) {
			return;
		}
		self.apply_permissions(sid, Some(p.allow_messages), Some(p.allow_files), true);
	}

	pub(super) fn apply_permissions(
		&mut self,
		sid: SessionId,
		messages: Option<bool>,
		files: Option<bool>,
		by_peer: bool,
	) {
		let Some(store) = self.history.get_mut(sid) else {
			return;
		};
		let mut perms = store.permissions();
		perms.allow_messages = messages.unwrap_or(perms.allow_messages);
		perms.allow_files = files.unwrap_or(perms.allow_files);
		store.set_permissions(perms);
		if !perms.allow_files {
			self.cancel_session_transfers(sid, false);
		}
		self.emit(
			sid,
			session_event::Event::Permissions(pb::PermissionsChanged {
				session_id: sid.0.to_vec(),
				permissions: Some(pb::SessionPermissions {
					allow_messages: perms.allow_messages,
					allow_files: perms.allow_files,
				}),
				by_peer,
			}),
		);
	}
}
