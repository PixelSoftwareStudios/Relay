//! Which local clients exist, what they may do, and whether they are idle.

use relay_io::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use relay_core::ClientId;
use tokio::time::Instant;

use super::error::ServiceError;

/// A permission a client can be granted at registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
	/// Read snapshots and watch state.
	Observe,
	/// Change engine and call state, including push-to-talk input.
	CallControl,
	/// Stop the whole engine. Never granted unless named at registration.
	Admin,
	/// Read and send session messages, transfers and transcripts. Never granted by default and
	/// not implied by `Admin`: transcripts are private to the people in the call.
	Messages,
}

impl Capability {
	/// Every capability this engine can grant.
	pub const ALL: [Self; 4] = [
		Self::Observe,
		Self::CallControl,
		Self::Admin,
		Self::Messages,
	];

	/// What a client gets when it asks for nothing in particular.
	pub const DEFAULT: [Self; 2] = [Self::Observe, Self::CallControl];

	/// The wire name.
	pub const fn name(self) -> &'static str {
		match self {
			Self::Observe => "observe",
			Self::CallControl => "call-control",
			Self::Admin => "admin",
			Self::Messages => "messages",
		}
	}

	/// Parses a wire name.
	pub fn parse(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|cap| cap.name() == name)
	}
}

#[derive(Debug)]
struct Entry {
	granted: Vec<Capability>,
	last_seen: Instant,
	streams: usize,
}

/// Registered clients as seen by the API layer.
#[derive(Debug, Default)]
pub struct ClientBook {
	entries: Mutex<HashMap<ClientId, Entry>>,
}

/// Keeps a client counted as active while one of its streams is open.
#[derive(Debug)]
pub struct StreamTicket {
	book: Arc<ClientBook>,
	client: ClientId,
}

impl Drop for StreamTicket {
	fn drop(&mut self) {
		let mut entries = self.book.lock();
		if let Some(entry) = entries.get_mut(&self.client) {
			entry.streams = entry.streams.saturating_sub(1);
			entry.last_seen = Instant::now();
		}
	}
}

impl ClientBook {
	fn lock(&self) -> MutexGuard<'_, HashMap<ClientId, Entry>> {
		self.entries.lock_unpoisoned()
	}

	/// Adds a client with the capabilities it was granted.
	pub fn insert(&self, client: ClientId, granted: Vec<Capability>) {
		self.lock().insert(
			client,
			Entry {
				granted,
				last_seen: Instant::now(),
				streams: 0,
			},
		);
	}

	/// Forgets a client.
	pub fn remove(&self, client: ClientId) {
		self.lock().remove(&client);
	}

	/// Records activity and checks the capability.
	pub fn require(&self, client: ClientId, cap: Capability) -> Result<(), ServiceError> {
		let mut entries = self.lock();
		let entry = entries
			.get_mut(&client)
			.ok_or_else(|| ServiceError::Precondition("client is not registered".to_owned()))?;
		entry.last_seen = Instant::now();
		if entry.granted.contains(&cap) {
			Ok(())
		} else {
			Err(ServiceError::CapabilityDenied(cap.name()))
		}
	}

	/// Counts a stream against the client until the ticket is dropped.
	pub fn open_stream(self: &Arc<Self>, client: ClientId) -> StreamTicket {
		if let Some(entry) = self.lock().get_mut(&client) {
			entry.streams += 1;
		}
		StreamTicket {
			book: Arc::clone(self),
			client,
		}
	}

	/// Clients with no open stream that have been quiet for `ttl`.
	pub fn idle(&self, ttl: Duration) -> Vec<ClientId> {
		self.lock()
			.iter()
			.filter(|(_, e)| e.streams == 0 && e.last_seen.elapsed() >= ttl)
			.map(|(client, _)| *client)
			.collect()
	}
}
