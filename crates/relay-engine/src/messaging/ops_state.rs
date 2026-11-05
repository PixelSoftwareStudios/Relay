//! Session state operations: permissions, pins, clear, export and preferences.

use relay_api::pb::{self, SessionOpResult, session_event};
use relay_messages::store::ExportOptions;
use relay_messages::{MessageId, Origin, SessionId};

use super::hub::Hub;
use super::ops::{Caller, Done};
use super::problem::Refusal;
use super::wire;

impl Hub {
	/// Changes the session permissions; the host pushes them to the guest.
	pub fn set_permissions(
		&mut self,
		who: Caller,
		session: &[u8],
		messages: Option<bool>,
		files: Option<bool>,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.find(session)?;
			if self
				.store(sid)
				.is_none_or(relay_messages::SessionStore::is_ended)
			{
				return Err(Refusal::new("SESSION_ENDED", "the call has ended"));
			}
			self.apply_permissions(sid, messages, files, false);
			let perms = self
				.store(sid)
				.map(relay_messages::SessionStore::permissions);
			let host = self.sessions.get(&sid).is_some_and(|m| m.host);
			if let (true, Some(p), Ok(peer)) = (host, perms, self.peer_of(sid)) {
				self.send(peer, wire::permissions(p.allow_messages, p.allow_files));
			}
			Ok(Done::session(sid))
		})();
		self.finish(&who, outcome)
	}

	/// Pins or unpins one message locally.
	pub fn pin(
		&mut self,
		who: Caller,
		session: &[u8],
		outgoing: bool,
		seq: u64,
		pinned: bool,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.find(session)?;
			let id = MessageId {
				origin: if outgoing {
					Origin::Local
				} else {
					Origin::Remote
				},
				seq,
			};
			let store = self
				.history
				.get_mut(sid)
				.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
			store.set_pinned(id, pinned)?;
			let message = self.message_ref(sid, id);
			self.emit(
				sid,
				session_event::Event::Pin(pb::PinChanged {
					session_id: sid.0.to_vec(),
					message,
					pinned,
				}),
			);
			Ok(Done {
				session: Some(sid),
				message,
				offer: None,
			})
		})();
		self.finish(&who, outcome)
	}

	/// Clears the transcript (active) or removes it (ended). Local only.
	pub fn clear(&mut self, who: Caller, session: &[u8]) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let sid = self.find(session)?;
			let ended = self
				.store(sid)
				.is_none_or(relay_messages::SessionStore::is_ended);
			self.cancel_session_transfers(sid, false);
			if ended {
				self.history.clear(sid);
				self.sessions.remove(&sid);
				self.emit(
					sid,
					session_event::Event::Cleared(pb::SessionCleared {
						session_id: sid.0.to_vec(),
						removed: true,
						next_ordinal: 0,
					}),
				);
			} else {
				let len = self.store(sid).map_or(0, |s| s.messages().len() as u64);
				self.history.clear(sid);
				let mut next = 1;
				if let Some(meta) = self.sessions.get_mut(&sid) {
					meta.base += len;
					meta.xfers.retain(|_, x| x.active);
					next = meta.base + 1;
				}
				self.emit(
					sid,
					session_event::Event::Cleared(pb::SessionCleared {
						session_id: sid.0.to_vec(),
						removed: false,
						next_ordinal: next,
					}),
				);
				self.emit_summary(sid);
			}
			Ok(Done::session(sid))
		})();
		self.finish(&who, outcome)
	}

	/// The export document; never written by the daemon here.
	pub fn export(&self, session: &[u8], embed_images: bool) -> Result<String, Refusal> {
		let sid = self.find(session)?;
		let store = self
			.store(sid)
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		Ok(store.export_json(ExportOptions { embed_images }))
	}

	/// Applies new live preferences.
	pub fn set_settings(
		&mut self,
		who: Caller,
		enabled: Option<bool>,
		max_offer_mib: Option<u32>,
		rate_mbps: Option<u32>,
	) -> SessionOpResult {
		if let Some(done) = self.replay(&who) {
			return done;
		}
		let outcome = (|| {
			let mut next = self.cfg();
			next.enabled = enabled.unwrap_or(next.enabled);
			next.max_offer_size_mib = max_offer_mib.unwrap_or(next.max_offer_size_mib);
			next.transfer_rate_limit_mbps = rate_mbps.unwrap_or(next.transfer_rate_limit_mbps);
			next.validate()
				.map_err(|m| Refusal::new("INVALID_SETTING", m))?;
			self.env.store.save(next).map_err(|_| {
				Refusal::new("INVALID_SETTING", "the preferences could not be saved")
			})?;
			self.env.config.store(std::sync::Arc::new(next));
			self.rate.send_replace(next.rate_bytes_per_sec());
			if !next.enabled {
				let ids: Vec<SessionId> = self.by_call.values().copied().collect();
				for sid in ids {
					self.cancel_session_transfers(sid, false);
				}
			}
			Ok(Done::default())
		})();
		self.finish(&who, outcome)
	}
}
