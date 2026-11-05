//! All transfers of one session, with the concurrency limit.

use std::collections::HashMap;

use super::receiver::Receiver;
use super::sender::Sender;
use super::sink::FileSink;
use super::types::*;
use crate::error::TransferError;
use crate::limits::MAX_CONCURRENT_TRANSFERS;
use crate::message::{FileOffer, OfferId};

enum Entry {
	Send(Sender),
	Recv(Box<Receiver>),
}

impl Entry {
	fn state(&self) -> TransferState {
		match self {
			Self::Send(s) => s.state(),
			Self::Recv(r) => r.state(),
		}
	}
}

/// Transfer registry. At most two active transfers (accepted, transferring or verifying).
#[derive(Default)]
pub struct Transfers {
	entries: HashMap<OfferId, Entry>,
}

impl Transfers {
	/// Empty registry.
	pub fn new() -> Self {
		Self::default()
	}

	/// Transfers counting toward the limit.
	pub fn active(&self) -> usize {
		self.entries
			.values()
			.filter(|e| e.state().is_active())
			.count()
	}

	/// State of one transfer.
	pub fn state(&self, id: OfferId) -> Option<TransferState> {
		self.entries.get(&id).map(Entry::state)
	}

	/// Registers an offer we made.
	pub fn offer_sent(&mut self, offer: &FileOffer) {
		self.entries
			.insert(offer.id, Entry::Send(Sender::new(offer)));
	}

	/// Registers an offer we received.
	pub fn offer_received(&mut self, offer: &FileOffer) {
		self.entries
			.insert(offer.id, Entry::Recv(Box::new(Receiver::new(offer))));
	}

	/// Receiver accepts. Refused with `TooManyTransfers` at the limit; the offer stays `Offered`.
	pub fn accept(
		&mut self,
		id: OfferId,
		sink: &mut dyn FileSink,
	) -> Result<Vec<Effect>, TransferError> {
		if self.active() >= MAX_CONCURRENT_TRANSFERS {
			return Err(TransferError::TooManyTransfers);
		}
		match self.entries.get_mut(&id) {
			Some(Entry::Recv(r)) => r.accept(sink),
			Some(_) => Err(TransferError::WrongState),
			None => Err(TransferError::Unknown),
		}
	}

	/// Receiver rejects.
	pub fn reject(&mut self, id: OfferId) -> Result<Vec<Effect>, TransferError> {
		match self.entries.get_mut(&id) {
			Some(Entry::Recv(r)) => r.reject(),
			Some(_) => Err(TransferError::WrongState),
			None => Err(TransferError::Unknown),
		}
	}

	/// Cancels one transfer on either side.
	pub fn cancel(
		&mut self,
		id: OfferId,
		sink: Option<&mut dyn FileSink>,
	) -> Result<Vec<Effect>, TransferError> {
		match (self.entries.get_mut(&id), sink) {
			(Some(Entry::Send(s)), _) => Ok(s.cancel()),
			(Some(Entry::Recv(r)), Some(k)) => Ok(r.cancel(k)),
			(Some(Entry::Recv(_)), None) => Err(TransferError::WrongState),
			(None, _) => Err(TransferError::Unknown),
		}
	}

	/// Message from the peer. `sink` is required for receiving transfers.
	pub fn on_message(
		&mut self,
		id: OfferId,
		msg: TransferMsg,
		sink: Option<&mut dyn FileSink>,
	) -> Result<Vec<Effect>, TransferError> {
		let active_others = self
			.entries
			.iter()
			.filter(|(k, e)| **k != id && e.state().is_active())
			.count();
		match (self.entries.get_mut(&id), sink) {
			(Some(Entry::Send(s)), _) => {
				if msg == TransferMsg::Accept && active_others >= MAX_CONCURRENT_TRANSFERS {
					return Ok(s.cancel());
				}
				Ok(s.on_message(msg))
			}
			(Some(Entry::Recv(r)), Some(k)) => Ok(r.on_message(msg, k)),
			(Some(Entry::Recv(_)), None) => Err(TransferError::WrongState),
			(None, _) => Err(TransferError::Unknown),
		}
	}

	/// Sender: outcome of the source check.
	pub fn source_checked(
		&mut self,
		id: OfferId,
		unchanged: bool,
	) -> Result<Vec<Effect>, TransferError> {
		self.sender(id).map(|s| s.source_checked(unchanged))
	}

	/// Sender: data for the pending read.
	pub fn chunk_read(&mut self, id: OfferId, data: Vec<u8>) -> Result<Vec<Effect>, TransferError> {
		self.sender(id).map(|s| s.chunk_read(data))
	}

	/// Sender: read failure.
	pub fn read_failed(&mut self, id: OfferId) -> Result<Vec<Effect>, TransferError> {
		self.sender(id).map(Sender::read_failed)
	}

	fn sender(&mut self, id: OfferId) -> Result<&mut Sender, TransferError> {
		match self.entries.get_mut(&id) {
			Some(Entry::Send(s)) => Ok(s),
			Some(_) => Err(TransferError::WrongState),
			None => Err(TransferError::Unknown),
		}
	}

	/// Ids of non-terminal transfers (to cancel when files are disabled or the session ends).
	pub fn open_ids(&self) -> Vec<OfferId> {
		self.entries
			.iter()
			.filter(|(_, e)| !e.state().is_terminal())
			.map(|(k, _)| *k)
			.collect()
	}
}
