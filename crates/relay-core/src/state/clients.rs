//! Client attach and detach.

use super::{ClientSession, State};
use crate::error::Reject;
use crate::event::{Event, InputFault};
use crate::id::{CallId, ClientId};
use crate::outcome::{OpResult, Reply};

impl State {
	pub(super) fn attach(&mut self, client: ClientId) -> OpResult {
		if self.clients.contains_key(&client) {
			return Err(Reject::AlreadyAttached);
		}
		if self.clients.len() >= self.limits.attached_clients {
			return Err(Reject::ClientLimit);
		}
		self.clients.insert(client, ClientSession::default());
		self.push_event(Event::ClientAttached(client));
		Ok(Reply::Done)
	}

	/// Removes a client and fails closed on everything it held.
	pub(super) fn detach(&mut self, client: ClientId) {
		if self.clients.remove(&client).is_none() {
			return;
		}
		let held: Vec<CallId> = self
			.calls
			.values()
			.filter(|call| call.controller.is_some_and(|lease| lease.holder == client))
			.map(|call| call.id)
			.collect();
		for call in held {
			self.close_leases(call, InputFault::ClientGone);
			self.lose_controller(call, InputFault::ClientGone);
		}
		self.ops.purge_client(client);
		self.push_event(Event::ClientDetached(client));
	}
}
