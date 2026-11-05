//! Bounded record of service-level operations (everything that does not go
//! through the domain core, which keeps its own idempotency cache).

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use relay_api::pb::Operation;
use relay_core::{ClientId, MonoTime, OperationId};

/// Hash of the full request message; equal requests have equal prints.
pub type Fingerprint = [u8; 32];

/// What a lookup of an operation found.
#[derive(Debug)]
pub enum Lookup<'a> {
	/// Never seen, or evicted.
	Miss,
	/// Same id, same request: the stored outcome.
	Replay(&'a Operation),
	/// Same id, different request.
	Conflict,
}

#[derive(Debug)]
struct Entry {
	fingerprint: Fingerprint,
	operation: Operation,
	at: MonoTime,
	seq: u64,
}

/// Retains operations per client within count and age bounds.
#[derive(Debug)]
pub struct Ledger {
	records: BTreeMap<(ClientId, OperationId), Entry>,
	order: VecDeque<(u64, ClientId, OperationId)>,
	next_seq: u64,
	per_client: usize,
	global: usize,
	retention: Duration,
}

impl Ledger {
	/// A ledger with the given bounds.
	pub fn new(per_client: usize, global: usize, retention: Duration) -> Self {
		Self {
			records: BTreeMap::new(),
			order: VecDeque::new(),
			next_seq: 0,
			per_client,
			global,
			retention,
		}
	}

	/// Looks an operation up by id and request fingerprint.
	pub fn lookup(&self, client: ClientId, id: OperationId, print: &Fingerprint) -> Lookup<'_> {
		match self.records.get(&(client, id)) {
			None => Lookup::Miss,
			Some(entry) if entry.fingerprint == *print => Lookup::Replay(&entry.operation),
			Some(_) => Lookup::Conflict,
		}
	}

	/// The stored outcome of an operation, for polling.
	pub fn get(&self, client: ClientId, id: OperationId) -> Option<&Operation> {
		self.records
			.get(&(client, id))
			.map(|entry| &entry.operation)
	}

	/// Stores an outcome, evicting the oldest records over the bounds.
	pub fn record(
		&mut self,
		client: ClientId,
		id: OperationId,
		print: Fingerprint,
		operation: Operation,
		now: MonoTime,
	) {
		while self.count_for(client) >= self.per_client && self.evict_oldest(Some(client)) {}
		while self.records.len() >= self.global && self.evict_oldest(None) {}
		let seq = self.next_seq;
		self.next_seq += 1;
		self.order.push_back((seq, client, id));
		self.records.insert(
			(client, id),
			Entry {
				fingerprint: print,
				operation,
				at: now,
				seq,
			},
		);
	}

	/// Drops records older than the retention window.
	pub fn expire(&mut self, now: MonoTime) {
		while let Some(&(seq, client, id)) = self.order.front() {
			if let Some(entry) = self.records.get(&(client, id))
				&& entry.seq == seq
			{
				if now.saturating_since(entry.at) < self.retention {
					break;
				}
				self.records.remove(&(client, id));
			}
			self.order.pop_front();
		}
	}

	/// Forgets everything a client recorded.
	pub fn purge_client(&mut self, client: ClientId) {
		self.records.retain(|(owner, _), _| *owner != client);
		self.order.retain(|&(_, owner, _)| owner != client);
	}

	fn count_for(&self, client: ClientId) -> usize {
		self.records
			.keys()
			.filter(|(owner, _)| *owner == client)
			.count()
	}

	fn evict_oldest(&mut self, only: Option<ClientId>) -> bool {
		let position = self.order.iter().position(|&(seq, client, id)| {
			only.is_none_or(|wanted| wanted == client)
				&& self
					.records
					.get(&(client, id))
					.is_some_and(|entry| entry.seq == seq)
		});
		let Some(index) = position else { return false };
		let Some((_, client, id)) = self.order.remove(index) else {
			return false;
		};
		self.records.remove(&(client, id));
		true
	}
}
