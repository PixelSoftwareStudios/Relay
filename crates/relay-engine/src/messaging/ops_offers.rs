//! Offer operations: answering, cancelling and previews.

use relay_messages::transfer::Receiver;
use relay_messages::{MessageBody, OfferId, OfferStatus, SessionId, sanitize::sanitize_file_name};
use relay_net::identity::Fingerprint;
use relay_net::proto::CancelReason;
use relay_net::transfer::{Expectation, TransferRole};
use tokio::sync::mpsc;

use relay_api::pb::SessionOpResult;

use super::hub::Hub;
use super::ops::{Caller, Done, GRANT_TTL_MS};
use super::problem::Refusal;
use super::session::XferCmd;
use super::wire;
use super::xfer::{RecvJob, TaskCtx, run_recv};

impl Hub {
	/// Accepts (into a directory) or rejects an incoming offer.
	pub fn respond(
		&mut self,
		who: Caller,
		session: &[u8],
		offer: &[u8],
		accept: bool,
		dir: &str,
		name: &str,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.sendable(session, true)?;
			let id = self.offer_ref(sid, offer)?;
			self.expect_incoming_open(sid, id)?;
			if accept {
				self.accept_offer(sid, id, dir, name)?;
			} else {
				self.reject_offer(sid, id)?;
			}
			Ok(Done {
				session: Some(sid),
				offer: Some(id),
				..Done::default()
			})
		})();
		self.finish(&who, outcome)
	}

	fn expect_incoming_open(&self, sid: SessionId, id: OfferId) -> Result<(), Refusal> {
		let incoming = self
			.sessions
			.get(&sid)
			.and_then(|m| m.xfers.get(&id))
			.is_some_and(|x| !x.outgoing);
		if incoming && self.offer_status(sid, id) == Some(OfferStatus::Offered) {
			Ok(())
		} else {
			Err(Refusal::new(
				"OFFER_STATE",
				"the offer is not open for an answer",
			))
		}
	}

	fn reject_offer(&mut self, sid: SessionId, id: OfferId) -> Result<(), Refusal> {
		let peer = self.peer_of(sid)?;
		self.send(peer, wire::offer_response(id, false, "declined"));
		self.set_offer(sid, id, OfferStatus::Rejected);
		Ok(())
	}

	fn accept_offer(
		&mut self,
		sid: SessionId,
		id: OfferId,
		dir: &str,
		name: &str,
	) -> Result<(), Refusal> {
		let hub = self
			.env
			.transfers
			.clone()
			.ok_or_else(|| Refusal::new("FILE_REFUSED", "file transfers are not available"))?;
		if self.active_transfers(sid) >= self.env.limits.transfers {
			return Err(Refusal::new(
				"TOO_MANY_TRANSFERS",
				"the limit of transfers running at once is reached",
			));
		}
		let dir = std::path::PathBuf::from(dir);
		if !dir.is_absolute() || !dir.is_dir() {
			return Err(Refusal::new(
				"DESTINATION_INVALID",
				"the destination must be an existing absolute directory",
			));
		}
		let mut offer = self
			.store(sid)
			.and_then(|s| {
				s.messages().iter().find_map(|m| match &m.body {
					MessageBody::File(o) if o.id == id => Some(o.clone()),
					_ => None,
				})
			})
			.ok_or_else(|| Refusal::new("OFFER_NOT_FOUND", "unknown offer"))?;
		if offer.size > self.cfg().max_offer_bytes() {
			return Err(Refusal::new(
				"FILE_TOO_LARGE",
				"the file is larger than the offer limit",
			));
		}
		if !name.trim().is_empty() {
			offer.name = sanitize_file_name(name);
		}
		let mut sink = (self.env.sinks)(&dir).map_err(|_| {
			Refusal::new("DESTINATION_INVALID", "the destination cannot be written")
		})?;
		let mut receiver = Box::new(Receiver::new(&offer));
		let effects = receiver.accept(&mut *sink)?;
		let peer = self.peer_of(sid)?;
		let call = self
			.sessions
			.get(&sid)
			.map(|m| m.call.to_bytes())
			.unwrap_or_default();
		let expectation = Expectation {
			offer_id: wire::offer_bytes(id),
			session_id: call,
			peer: Fingerprint::from_bytes(*peer.as_bytes()),
			role: TransferRole::Receiver,
			size: offer.size,
		};
		let (token, pending) = hub
			.expect(
				expectation,
				Some(std::time::Duration::from_millis(u64::from(GRANT_TTL_MS))),
			)
			.map_err(|_| Refusal::new("TOO_MANY_TRANSFERS", "too many pending data links"))?;
		self.send(peer, wire::offer_response(id, true, ""));
		self.send(
			peer,
			wire::data_link(id, token.as_bytes(), hub.local_addr().port(), GRANT_TTL_MS),
		);
		let (tx, cmds) = mpsc::channel(8);
		if let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
		{
			x.active = true;
			x.task = Some(tx);
			x.dest_dir = Some(dir);
			x.dest_name = (!name.trim().is_empty()).then(|| name.to_owned());
		}
		let job = RecvJob {
			ctx: TaskCtx {
				handle: self.handle.clone(),
				session: sid,
				offer: id,
			},
			offer,
			pending: Some(pending),
			receiver,
			initial: effects,
			sink,
			cmds,
		};
		tokio::spawn(run_recv(job));
		self.set_offer(sid, id, OfferStatus::InProgress);
		Ok(())
	}

	/// Cancels a running transfer or withdraws/declines an open offer.
	pub fn cancel_transfer(
		&mut self,
		who: Caller,
		session: &[u8],
		offer: &[u8],
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.find(session)?;
			let id = self.offer_ref(sid, offer)?;
			let peer = self.peer_of(sid)?;
			let (outgoing, task, active) = self
				.sessions
				.get(&sid)
				.and_then(|m| m.xfers.get(&id))
				.map(|x| (x.outgoing, x.task.clone(), x.active))
				.ok_or_else(|| Refusal::new("OFFER_NOT_FOUND", "unknown offer"))?;
			match self.offer_status(sid, id) {
				Some(OfferStatus::Offered) if outgoing => {
					self.send(peer, wire::cancel(id, CancelReason::CancelUser));
					self.fail_offer(sid, id, OfferStatus::Cancelled, "cancelled");
				}
				Some(OfferStatus::Offered) => self.reject_offer(sid, id)?,
				Some(OfferStatus::InProgress) if active => match task {
					Some(task) => {
						let _ = task.try_send(XferCmd::Cancel);
					}
					None => {
						self.send(peer, wire::cancel(id, CancelReason::CancelUser));
						self.fail_offer(sid, id, OfferStatus::Cancelled, "cancelled");
					}
				},
				_ => return Err(Refusal::new("OFFER_STATE", "nothing to cancel")),
			}
			Ok(Done {
				session: Some(sid),
				offer: Some(id),
				..Done::default()
			})
		})();
		self.finish(&who, outcome)
	}

	/// Receiver: asks the sender for a text preview.
	pub fn request_preview(
		&mut self,
		who: Caller,
		session: &[u8],
		offer: &[u8],
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.sendable(session, true)?;
			let id = self.offer_ref(sid, offer)?;
			self.expect_incoming_open(sid, id)?;
			let has_preview = self.store(sid).is_some_and(|s| {
				s.messages().iter().any(
					|m| matches!(&m.body, MessageBody::File(o) if o.id == id && o.preview.is_some()),
				)
			});
			let peer = self.peer_of(sid)?;
			let x = self
				.sessions
				.get_mut(&sid)
				.and_then(|m| m.xfers.get_mut(&id));
			match x {
				Some(x) if !x.preview_requested && !has_preview => x.preview_requested = true,
				_ => {
					return Err(Refusal::new(
						"OFFER_STATE",
						"a preview was already requested",
					));
				}
			}
			self.send(peer, wire::preview_request(id));
			self.emit_offer(sid, id);
			Ok(Done {
				session: Some(sid),
				offer: Some(id),
				..Done::default()
			})
		})();
		self.finish(&who, outcome)
	}

	/// The path to read a preview from, once the sender's user decides to approve.
	pub fn preview_source(
		&self,
		session: &[u8],
		offer: &[u8],
	) -> Result<std::path::PathBuf, Refusal> {
		let sid = self.sendable(session, true)?;
		let id = self.offer_ref(sid, offer)?;
		let x = self.sessions.get(&sid).and_then(|m| m.xfers.get(&id));
		match x {
			Some(x) if x.outgoing && x.preview_pending => x
				.source
				.as_ref()
				.map(|s| s.path.clone())
				.ok_or_else(|| Refusal::new("PREVIEW_UNAVAILABLE", "no preview was requested")),
			_ => Err(Refusal::new(
				"PREVIEW_UNAVAILABLE",
				"no preview was requested",
			)),
		}
	}

	/// Sender: answers a preview request. `text` is the already-read preview when approving.
	pub fn answer_preview(
		&mut self,
		who: Caller,
		session: &[u8],
		offer: &[u8],
		text: Result<Option<String>, Refusal>,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.sendable(session, true)?;
			let id = self.offer_ref(sid, offer)?;
			let peer = self.peer_of(sid)?;
			let asked = self
				.sessions
				.get_mut(&sid)
				.and_then(|m| m.xfers.get_mut(&id))
				.is_some_and(|x| x.outgoing && std::mem::take(&mut x.preview_pending));
			if !asked {
				return Err(Refusal::new(
					"PREVIEW_UNAVAILABLE",
					"no preview was requested",
				));
			}
			let text = match text {
				Ok(text) => text,
				Err(refusal) => {
					self.send(peer, wire::preview_reply(id, None));
					self.emit_offer(sid, id);
					return Err(refusal);
				}
			};
			self.send(peer, wire::preview_reply(id, text));
			self.emit_offer(sid, id);
			Ok(Done {
				session: Some(sid),
				offer: Some(id),
				..Done::default()
			})
		})();
		self.finish(&who, outcome)
	}
}
