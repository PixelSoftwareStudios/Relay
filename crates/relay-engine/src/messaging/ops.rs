//! API operations on the messaging state. Every mutation answers with a final
//! [`SessionOpResult`]; refusals carry stable problem codes and never private content.

use relay_api::pb::{self, OperationState, SessionOpResult};
use relay_messages::{MessageId, OfferId, OfferMeta, Origin, SessionId};

use super::hub::Hub;
use super::problem::Refusal;
use super::session::{SourceInfo, Xfer};
use super::source::Prepared;
use super::wire;

/// Largest inline image: a control record is 16 KiB and must also hold the envelope.
pub const MAX_INLINE_IMAGE_BYTES: usize = relay_net::control::session_msg::MAX_INLINE_IMAGE_BYTES;
/// Validity of the one-time data-link token.
pub(super) const GRANT_TTL_MS: u32 = 30_000;
/// Largest export returned inline.
pub const EXPORT_INLINE_MAX: usize = relay_api::EXPORT_INLINE_MAX_BYTES;

/// Who is calling, for idempotency.
#[derive(Debug, Clone, Copy)]
pub struct Caller {
	/// Client id bytes.
	pub client: [u8; 16],
	/// Operation id bytes.
	pub operation: [u8; 16],
	/// Fingerprint of the request body.
	pub print: [u8; 32],
}

pub(super) type Outcome = Result<Done, Refusal>;

/// What a successful operation reports.
#[derive(Default)]
pub struct Done {
	pub(super) session: Option<SessionId>,
	pub(super) message: Option<pb::MessageRef>,
	pub(super) offer: Option<OfferId>,
}

impl Done {
	pub(super) fn session(sid: SessionId) -> Self {
		Self {
			session: Some(sid),
			..Self::default()
		}
	}
}

impl Hub {
	pub(super) fn result(&self, who: &Caller, outcome: Outcome) -> SessionOpResult {
		let mut operation = pb::Operation {
			operation_id: who.operation.to_vec(),
			state: OperationState::Applied as i32,
			revision: self.feed.revision(),
			call_id: Vec::new(),
			problem: None,
		};
		match outcome {
			Ok(done) => SessionOpResult {
				operation: Some(operation),
				session_id: done.session.map(|s| s.0.to_vec()).unwrap_or_default(),
				message: done.message,
				offer_id: done
					.offer
					.map(|o| wire::offer_bytes(o).to_vec())
					.unwrap_or_default(),
			},
			Err(refusal) => {
				operation.state = OperationState::Failed as i32;
				operation.problem = Some(refusal.problem());
				SessionOpResult {
					operation: Some(operation),
					..SessionOpResult::default()
				}
			}
		}
	}

	/// The stored outcome of a repeated operation id. An id reused for a different request
	/// answers `OPERATION_ID_REUSED` (the API layer turns that into `ALREADY_EXISTS`).
	pub fn replay(&self, who: &Caller) -> Option<SessionOpResult> {
		match self.idem.get((who.client, who.operation)) {
			None => None,
			Some((print, stored)) if *print == who.print => Some(stored.clone()),
			Some(_) => Some(self.result(
				who,
				Err(Refusal::new(
					"OPERATION_ID_REUSED",
					"operation id reused with a different request",
				)),
			)),
		}
	}

	/// A refusal that is not remembered (checks made before the work starts).
	pub fn refused(&self, who: &Caller, refusal: Refusal) -> SessionOpResult {
		self.result(who, Err(refusal))
	}

	/// Before a file is hashed: the stored outcome if there is one, else whether an offer is
	/// possible at all and the size limit to hash against.
	pub fn begin_offer(&self, who: &Caller, session: &[u8]) -> Result<u64, Box<SessionOpResult>> {
		if let Some(done) = self.replay(who) {
			return Err(Box::new(done));
		}
		self.precheck_offer(session)
			.map_err(|r| Box::new(self.refused(who, r)))
	}

	pub(super) fn finish(&mut self, who: &Caller, outcome: Outcome) -> SessionOpResult {
		let result = self.result(who, outcome);
		self.idem
			.put((who.client, who.operation), who.print, result.clone());
		result
	}

	// ----- lookups --------------------------------------------------------------------

	pub(super) fn find(&self, raw: &[u8]) -> Result<SessionId, Refusal> {
		let id: [u8; 16] = raw
			.try_into()
			.map_err(|_| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		let sid = SessionId(id);
		if self.sessions.contains_key(&sid) && self.history.get(sid).is_some() {
			Ok(sid)
		} else {
			Err(Refusal::new("SESSION_NOT_FOUND", "unknown session"))
		}
	}

	/// A live session that may send now.
	pub(super) fn sendable(&self, raw: &[u8], files: bool) -> Result<SessionId, Refusal> {
		if !self.cfg().enabled {
			return Err(Refusal::new("MESSAGES_DISABLED", "messaging is turned off"));
		}
		let sid = self.find(raw)?;
		let store = self
			.store(sid)
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		if store.is_ended() {
			return Err(Refusal::new("SESSION_ENDED", "the call has ended"));
		}
		let perms = store.permissions();
		if (files && !perms.allow_files) || (!files && !perms.allow_messages) {
			return Err(Refusal::new(
				"SESSION_PERMISSION_OFF",
				"this kind of item is turned off for the session",
			));
		}
		Ok(sid)
	}

	pub(super) fn peer_of(&self, sid: SessionId) -> Result<relay_core::PeerId, Refusal> {
		self.sessions
			.get(&sid)
			.map(|m| m.peer)
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))
	}

	pub(super) fn offer_ref(&self, sid: SessionId, raw: &[u8]) -> Result<OfferId, Refusal> {
		let id =
			wire::offer_id(raw).ok_or_else(|| Refusal::new("OFFER_NOT_FOUND", "unknown offer"))?;
		if self
			.sessions
			.get(&sid)
			.is_some_and(|m| m.xfers.contains_key(&id))
		{
			Ok(id)
		} else {
			Err(Refusal::new("OFFER_NOT_FOUND", "unknown offer"))
		}
	}

	pub(super) fn message_ref(&self, sid: SessionId, id: MessageId) -> Option<pb::MessageRef> {
		let store = self.history.get(sid)?;
		let meta = self.sessions.get(&sid)?;
		let index = super::view::index_of(store, id)?;
		Some(pb::MessageRef {
			ordinal: super::view::ordinal(meta, index),
			outgoing: id.origin == Origin::Local,
			seq: id.seq,
		})
	}

	// ----- sending --------------------------------------------------------------------

	pub(super) fn precheck_offer(&self, session: &[u8]) -> Result<u64, Refusal> {
		self.sendable(session, true)?;
		if self.env.transfers.is_none() {
			return Err(Refusal::new(
				"FILE_REFUSED",
				"file transfers are not available",
			));
		}
		Ok(self.cfg().max_offer_bytes())
	}

	/// Sends text.
	pub fn send_text(&mut self, who: Caller, session: &[u8], text: &str) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = self.compose(session, false, |store| store.compose_text(text));
		self.finish(&who, outcome)
	}

	/// Sends an inline image.
	pub fn send_image(&mut self, who: Caller, session: &[u8], image: Vec<u8>) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = if image.len() > MAX_INLINE_IMAGE_BYTES {
			Err(Refusal::new(
				"IMAGE_TOO_LARGE",
				"inline images are limited to 15 KiB; offer larger images as files",
			))
		} else {
			self.compose(session, true, |store| store.compose_image(image.into()))
		};
		self.finish(&who, outcome)
	}

	/// Composes with the store, puts the message on the link and publishes it.
	fn compose(
		&mut self,
		session: &[u8],
		files: bool,
		make: impl FnOnce(
			&mut relay_messages::SessionStore,
		) -> Result<relay_messages::Wire, relay_messages::StoreError>,
	) -> Outcome {
		let sid = self.sendable(session, files)?;
		let peer = self.peer_of(sid)?;
		let store = self
			.history
			.get_mut(sid)
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		let wire = make(store)?;
		self.send(peer, wire::to_wire(&wire));
		self.emit_last_entry(sid);
		let id = MessageId {
			origin: Origin::Local,
			seq: wire.seq,
		};
		Ok(Done {
			session: Some(sid),
			message: self.message_ref(sid, id),
			offer: None,
		})
	}

	/// Retries a failed local message.
	pub fn retry(&mut self, who: Caller, session: &[u8], seq: u64) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.sendable(session, false)?;
			let peer = self.peer_of(sid)?;
			let store = self
				.history
				.get_mut(sid)
				.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
			let wire = store.retry(seq)?;
			self.send(peer, wire::to_wire(&wire));
			self.emit_delivery(sid, seq);
			Ok(Done {
				session: Some(sid),
				message: self.message_ref(
					sid,
					MessageId {
						origin: Origin::Local,
						seq,
					},
				),
				offer: None,
			})
		})();
		self.finish(&who, outcome)
	}

	/// Offers a hashed file.
	pub fn offer_prepared(
		&mut self,
		who: Caller,
		session: &[u8],
		file: Prepared,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.sendable(session, true)?;
			if file.size > self.cfg().max_offer_bytes() {
				return Err(Refusal::new(
					"FILE_TOO_LARGE",
					"the file is larger than the offer limit",
				));
			}
			let peer = self.peer_of(sid)?;
			let id = OfferId(relay_io::entropy::OsEntropy.next_u128());
			let meta = OfferMeta {
				id,
				name: file.name,
				size: file.size,
				hash: file.hash,
			};
			let store = self
				.history
				.get_mut(sid)
				.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
			let wire = store.compose_offer(meta)?;
			let mut xfer = Xfer::new(true);
			xfer.source = Some(SourceInfo {
				path: file.path,
				facts: file.facts,
			});
			if let Some(m) = self.sessions.get_mut(&sid) {
				m.xfers.insert(id, xfer);
			}
			self.send(peer, wire::to_wire(&wire));
			self.emit_last_entry(sid);
			Ok(Done {
				session: Some(sid),
				message: self.message_ref(
					sid,
					MessageId {
						origin: Origin::Local,
						seq: wire.seq,
					},
				),
				offer: Some(id),
			})
		})();
		self.finish(&who, outcome)
	}
}
