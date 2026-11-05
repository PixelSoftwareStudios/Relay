//! `SessionStore`: one session's transcript with budget, pins, search, clear and export.

mod compose;
mod export;
mod query;
mod receive;

use std::collections::HashMap;
use std::sync::Arc;

pub use export::{
	EXPORT_SCHEMA, EXPORT_VERSION, Export, ExportCode, ExportContent, ExportError, ExportMessage,
	ExportOptions, ExportPeer,
};
pub use query::SearchResult;
pub use receive::{ReceiveOutcome, ack_status};

use crate::delivery::{Outbox, RetryAction};
use crate::env::Clock;
use crate::error::StoreError;
use crate::limits::*;
use crate::message::{
	AckStatus, Delivery, FailReason, Message, MessageBody, MessageId, OfferId, OfferStatus, Origin,
	PeerRef, Permissions, SessionId, Wire,
};
use crate::rate::TokenBucket;

/// Static facts about a session.
#[derive(Clone, Debug)]
pub struct SessionInfo {
	/// Session id.
	pub id: SessionId,
	/// The peer.
	pub peer: PeerRef,
	/// Start time (wall clock ms).
	pub started_at_ms: u64,
}

/// What the owner must do after [`SessionStore::poll`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreAction {
	/// Resend this message.
	Resend(Wire),
	/// This local message failed for good.
	Failed(u64),
}

/// Transcript of one session. RAM only.
pub struct SessionStore {
	pub(crate) info: SessionInfo,
	pub(crate) clock: Arc<dyn Clock>,
	pub(crate) permissions: Permissions,
	pub(crate) ended_at_ms: Option<u64>,
	pub(crate) messages: Vec<Message>,
	pub(crate) index: HashMap<MessageId, usize>,
	pub(crate) offers: HashMap<OfferId, usize>,
	pub(crate) used: u64,
	pub(crate) limit: u64,
	pub(crate) next_seq: u64,
	pub(crate) last_remote_seq: u64,
	pub(crate) outbox: Outbox,
	pub(crate) out_rate: TokenBucket,
	pub(crate) in_rate: TokenBucket,
	pub(crate) in_bytes: TokenBucket,
	pub(crate) offer_rate: TokenBucket,
	pub(crate) offer_count: usize,
}

impl SessionStore {
	/// New store with the standard 64 MiB budget.
	pub fn new(info: SessionInfo, permissions: Permissions, clock: Arc<dyn Clock>) -> Self {
		Self::with_limit(info, permissions, clock, SESSION_BUDGET_BYTES)
	}

	/// New store with a custom budget (tests, constrained devices).
	pub fn with_limit(
		info: SessionInfo,
		permissions: Permissions,
		clock: Arc<dyn Clock>,
		limit: u64,
	) -> Self {
		let now = clock.mono_ms();
		let msg = u64::from(MESSAGE_RATE_PER_SEC);
		Self {
			info,
			clock,
			permissions,
			ended_at_ms: None,
			messages: Vec::new(),
			index: HashMap::new(),
			offers: HashMap::new(),
			used: 0,
			limit,
			next_seq: 1,
			last_remote_seq: 0,
			outbox: Outbox::default(),
			out_rate: TokenBucket::new(msg, u64::from(MESSAGE_BURST), now),
			in_rate: TokenBucket::new(msg, u64::from(MESSAGE_BURST), now),
			in_bytes: TokenBucket::new(INBOUND_BYTES_PER_SEC, INBOUND_BYTES_BURST, now),
			offer_rate: TokenBucket::new(
				u64::from(OFFER_RATE_PER_SEC),
				u64::from(OFFER_BURST),
				now,
			),
			offer_count: 0,
		}
	}

	/// Session facts.
	pub fn info(&self) -> &SessionInfo {
		&self.info
	}

	/// Current permissions.
	pub fn permissions(&self) -> Permissions {
		self.permissions
	}

	/// Changes permissions. Existing messages stay; only new items are affected.
	pub fn set_permissions(&mut self, p: Permissions) {
		self.permissions = p;
	}

	/// Transcript in arrival order.
	pub fn messages(&self) -> &[Message] {
		&self.messages
	}

	/// One message.
	pub fn get(&self, id: MessageId) -> Option<&Message> {
		self.index.get(&id).map(|&i| &self.messages[i])
	}

	/// Bytes counted against the budget.
	pub fn used_bytes(&self) -> u64 {
		self.used
	}

	/// The budget.
	pub fn limit_bytes(&self) -> u64 {
		self.limit
	}

	/// True after [`Self::end`].
	pub fn is_ended(&self) -> bool {
		self.ended_at_ms.is_some()
	}

	/// When the session ended, wall clock.
	pub fn ended_at_ms(&self) -> Option<u64> {
		self.ended_at_ms
	}

	pub(crate) fn check_budget(&self, cost: u64) -> Result<(), StoreError> {
		if self.used.saturating_add(cost) > self.limit {
			Err(StoreError::BudgetExceeded {
				used: self.used,
				limit: self.limit,
			})
		} else {
			Ok(())
		}
	}

	pub(crate) fn push(&mut self, m: Message) {
		self.used += m.body.cost();
		let i = self.messages.len();
		self.index.insert(m.id, i);
		if let MessageBody::File(o) = &m.body {
			self.offers.insert(o.id, i);
			self.offer_count += 1;
		}
		self.messages.push(m);
	}

	/// Applies the peer's ack to a local message.
	pub fn on_ack(&mut self, seq: u64, status: AckStatus) -> Result<(), StoreError> {
		let id = MessageId {
			origin: Origin::Local,
			seq,
		};
		let i = *self.index.get(&id).ok_or(StoreError::NotFound)?;
		self.outbox.acked(seq);
		let m = &mut self.messages[i];
		if m.delivery != Delivery::Sending {
			return Ok(());
		}
		match status {
			AckStatus::Delivered => m.delivery = Delivery::Delivered,
			AckStatus::Rejected(r) => {
				m.delivery = Delivery::Failed(FailReason::Rejected(r));
				if let MessageBody::File(o) = &mut m.body {
					o.status = OfferStatus::Failed;
				}
			}
		}
		Ok(())
	}

	/// Due resends and give-ups. Call at or after [`Self::next_deadline`].
	pub fn poll(&mut self) -> Vec<StoreAction> {
		let now = self.clock.mono_ms();
		let mut out = Vec::new();
		for a in self.outbox.poll(now) {
			match a {
				RetryAction::Resend(seq) => {
					if let Some(w) = self.wire_for(seq) {
						out.push(StoreAction::Resend(w));
					}
				}
				RetryAction::GiveUp(seq) => {
					if let Some(&i) = self.index.get(&MessageId {
						origin: Origin::Local,
						seq,
					}) {
						self.messages[i].delivery = Delivery::Failed(FailReason::Timeout);
					}
					out.push(StoreAction::Failed(seq));
				}
			}
		}
		out
	}

	/// Earliest monotonic time at which [`Self::poll`] has work.
	pub fn next_deadline(&self) -> Option<u64> {
		self.outbox.next_deadline()
	}

	/// User retry of a failed local message: same sequence, fresh attempt budget.
	pub fn retry(&mut self, seq: u64) -> Result<Wire, StoreError> {
		if self.is_ended() {
			return Err(StoreError::Ended);
		}
		let i = *self
			.index
			.get(&MessageId {
				origin: Origin::Local,
				seq,
			})
			.ok_or(StoreError::NotFound)?;
		if !matches!(self.messages[i].delivery, Delivery::Failed(_)) {
			return Err(StoreError::WrongState);
		}
		self.messages[i].delivery = Delivery::Sending;
		if let MessageBody::File(o) = &mut self.messages[i].body {
			o.status = OfferStatus::Offered;
		}
		self.outbox.sent(seq, self.clock.mono_ms());
		self.wire_for(seq).ok_or(StoreError::NotFound)
	}

	/// Ends the session: stops traffic, expires offers, keeps the transcript readable.
	pub fn end(&mut self) {
		if self.is_ended() {
			return;
		}
		self.ended_at_ms = Some(self.clock.wall_ms());
		self.outbox.clear();
		for m in &mut self.messages {
			if m.delivery == Delivery::Sending {
				m.delivery = Delivery::Failed(FailReason::Timeout);
			}
			if let MessageBody::File(o) = &mut m.body {
				o.status = match o.status {
					OfferStatus::Offered => OfferStatus::Expired,
					OfferStatus::InProgress => OfferStatus::Cancelled,
					s => s,
				};
			}
		}
	}

	/// Local clear: removes the transcript on this side only. Sequences keep counting.
	pub fn clear(&mut self) {
		self.messages = Vec::new();
		self.index.clear();
		self.offers.clear();
		self.outbox.clear();
		self.used = 0;
		self.offer_count = 0;
	}

	/// Updates a file offer's status (driven by the transfer machine).
	pub fn set_offer_status(&mut self, id: OfferId, status: OfferStatus) -> Result<(), StoreError> {
		let i = *self.offers.get(&id).ok_or(StoreError::NotFound)?;
		match &mut self.messages[i].body {
			MessageBody::File(o) => {
				o.status = if self.ended_at_ms.is_some() && !status.is_terminal() {
					return Err(StoreError::Ended);
				} else {
					status
				};
				Ok(())
			}
			_ => Err(StoreError::NotFound),
		}
	}

	/// Attaches an approved text preview; counts toward the budget.
	pub fn attach_preview(&mut self, id: OfferId, text: &str) -> Result<(), StoreError> {
		if self.is_ended() {
			return Err(StoreError::Ended);
		}
		if !self.permissions.allow_files {
			return Err(StoreError::Disabled);
		}
		let text = crate::transfer::validate_preview(text)?;
		let i = *self.offers.get(&id).ok_or(StoreError::NotFound)?;
		self.check_budget(text.len() as u64)?;
		match &mut self.messages[i].body {
			MessageBody::File(o) if o.preview.is_none() => {
				self.used += text.len() as u64;
				o.preview = Some(text);
				Ok(())
			}
			_ => Err(StoreError::WrongState),
		}
	}
}
