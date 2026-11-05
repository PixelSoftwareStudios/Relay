//! Idempotency for channel mutations: the same operation id with the same request
//! returns the stored outcome; the same id with a different request is refused.
//!
//! The request print is keyed with a per-process secret, so a stored print does not
//! let anyone with a memory dump test password guesses offline.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use relay_api::pb::{CreateInviteResponse, Operation};
use relay_core::{ClientId, OperationId};

const PER_CLIENT: usize = 256;
const GLOBAL: usize = 4096;
const RETENTION: Duration = Duration::from_secs(600);

/// A stored outcome.
#[derive(Debug, Clone)]
pub struct Stored {
	/// The operation as answered.
	pub operation: Operation,
	/// The invite answer, kept only so an identical retry gets the same invite.
	pub invite: Option<CreateInviteResponse>,
}

/// What a lookup found.
#[derive(Debug)]
pub enum Replay {
	/// Never seen or expired: run it.
	Fresh,
	/// Seen with this request.
	Same(Box<Stored>),
	/// Seen with a different request.
	Conflict,
}

struct Entry {
	print: [u8; 32],
	stored: Stored,
	at: Duration,
}

pub(super) struct Ledger {
	key: [u8; 32],
	records: BTreeMap<(ClientId, OperationId), Entry>,
	order: VecDeque<(ClientId, OperationId)>,
}

impl Ledger {
	pub fn new() -> Self {
		Self {
			key: rand::random(),
			records: BTreeMap::new(),
			order: VecDeque::new(),
		}
	}

	/// Keyed print of an encoded request.
	pub fn print(&self, encoded: &[u8]) -> [u8; 32] {
		*blake3::keyed_hash(&self.key, encoded).as_bytes()
	}

	pub fn lookup(
		&mut self,
		client: ClientId,
		id: OperationId,
		print: &[u8; 32],
		now: Duration,
	) -> Replay {
		self.expire(now);
		match self.records.get(&(client, id)) {
			None => Replay::Fresh,
			Some(entry) if entry.print == *print => Replay::Same(Box::new(entry.stored.clone())),
			Some(_) => Replay::Conflict,
		}
	}

	pub fn get(&self, client: ClientId, id: OperationId) -> Option<Operation> {
		self.records
			.get(&(client, id))
			.map(|e| e.stored.operation.clone())
	}

	pub fn record(
		&mut self,
		client: ClientId,
		id: OperationId,
		print: [u8; 32],
		stored: Stored,
		now: Duration,
	) {
		let mine = self.records.keys().filter(|(c, _)| *c == client).count();
		if mine >= PER_CLIENT {
			self.evict(|(c, _)| *c == client);
		}
		if self.records.len() >= GLOBAL {
			self.evict(|_| true);
		}
		self.order.push_back((client, id));
		self.records.insert(
			(client, id),
			Entry {
				print,
				stored,
				at: now,
			},
		);
	}

	fn evict(&mut self, wanted: impl Fn(&(ClientId, OperationId)) -> bool) {
		if let Some(index) = self.order.iter().position(wanted)
			&& let Some(key) = self.order.remove(index)
		{
			self.records.remove(&key);
		}
	}

	fn expire(&mut self, now: Duration) {
		while let Some(key) = self.order.front().copied() {
			let old = self
				.records
				.get(&key)
				.is_none_or(|e| now.saturating_sub(e.at) >= RETENTION);
			if !old {
				break;
			}
			self.records.remove(&key);
			self.order.pop_front();
		}
	}
}
