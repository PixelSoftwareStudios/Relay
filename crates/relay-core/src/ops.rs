//! Bounded idempotency cache for mutation requests.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use crate::command::Command;
use crate::id::{ClientId, OperationId};
use crate::outcome::OpResult;
use crate::time::MonoTime;

/// A remembered operation: what was asked and what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
	/// The original request body.
	pub command: Command,
	/// The result it produced.
	pub result: OpResult,
	/// When it was recorded.
	pub at: MonoTime,
	seq: u64,
}

/// What a lookup of a request found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Lookup<'a> {
	/// Never seen (or evicted: the exactly-once window is gone).
	Miss,
	/// Same ID, same body.
	Replay(&'a OperationRecord),
	/// Same ID, different body.
	Conflict,
}

/// Retains results per `(client, operation)` within per-client and global
/// bounds and a retention window.
#[derive(Debug)]
pub(crate) struct OperationCache {
	records: BTreeMap<(ClientId, OperationId), OperationRecord>,
	order: VecDeque<(u64, ClientId, OperationId)>,
	per_client: BTreeMap<ClientId, usize>,
	next_seq: u64,
	per_client_limit: usize,
	global_limit: usize,
	retention: Duration,
}

impl OperationCache {
	pub(crate) fn new(per_client_limit: usize, global_limit: usize, retention: Duration) -> Self {
		Self {
			records: BTreeMap::new(),
			order: VecDeque::new(),
			per_client: BTreeMap::new(),
			next_seq: 0,
			per_client_limit,
			global_limit,
			retention,
		}
	}

	pub(crate) fn get(&self, client: ClientId, op: OperationId) -> Option<&OperationRecord> {
		self.records.get(&(client, op))
	}

	pub(crate) fn lookup(
		&self,
		client: ClientId,
		op: OperationId,
		command: &Command,
	) -> Lookup<'_> {
		match self.get(client, op) {
			None => Lookup::Miss,
			Some(record) if record.command == *command => Lookup::Replay(record),
			Some(_) => Lookup::Conflict,
		}
	}

	pub(crate) fn record(
		&mut self,
		client: ClientId,
		op: OperationId,
		command: Command,
		result: OpResult,
		now: MonoTime,
	) {
		while self.per_client.get(&client).copied().unwrap_or(0) >= self.per_client_limit {
			if !self.pop_oldest(Some(client)) {
				break;
			}
		}
		while self.records.len() >= self.global_limit {
			if !self.pop_oldest(None) {
				break;
			}
		}
		let seq = self.next_seq;
		self.next_seq += 1;
		self.order.push_back((seq, client, op));
		*self.per_client.entry(client).or_default() += 1;
		self.records.insert(
			(client, op),
			OperationRecord {
				command,
				result,
				at: now,
				seq,
			},
		);
	}

	/// Drops records older than the retention window.
	pub(crate) fn expire(&mut self, now: MonoTime) {
		while let Some(&(seq, client, op)) = self.order.front() {
			match self.records.get(&(client, op)) {
				Some(record) if record.seq == seq => {
					if now.saturating_since(record.at) < self.retention {
						break;
					}
					self.remove(client, op);
				}
				_ => {}
			}
			self.order.pop_front();
		}
	}

	/// Forgets everything a client recorded.
	pub(crate) fn purge_client(&mut self, client: ClientId) {
		self.records.retain(|(owner, _), _| *owner != client);
		self.per_client.remove(&client);
		self.order.retain(|&(_, owner, _)| owner != client);
	}

	/// Evicts the oldest live record, optionally only for one client.
	fn pop_oldest(&mut self, only: Option<ClientId>) -> bool {
		let position = self.order.iter().position(|&(seq, client, op)| {
			only.is_none_or(|wanted| wanted == client)
				&& self
					.records
					.get(&(client, op))
					.is_some_and(|record| record.seq == seq)
		});
		let Some(index) = position else { return false };
		let Some((_, client, op)) = self.order.remove(index) else {
			return false;
		};
		self.remove(client, op);
		true
	}

	fn remove(&mut self, client: ClientId, op: OperationId) {
		if self.records.remove(&(client, op)).is_some()
			&& let Some(count) = self.per_client.get_mut(&client)
		{
			*count = count.saturating_sub(1);
		}
	}
}
