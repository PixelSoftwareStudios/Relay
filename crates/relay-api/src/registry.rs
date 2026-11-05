use relay_io::LockExt;
use std::collections::HashSet;
use std::sync::Mutex;

use tonic::Status;

/// Opaque 16-byte client identifier. An identifier, never a credential.
pub type ClientId = [u8; 16];

/// Decides whether a request's client context is acceptable.
pub trait ClientValidator: Send + Sync + 'static {
	/// Check a client id (and the incarnation it claims to know, if sent).
	fn validate(&self, client: &ClientId, incarnation: Option<&[u8]>) -> Result<(), Status>;
}

/// Bounded in-memory registry of attached clients for one engine incarnation.
#[derive(Debug)]
pub struct ClientRegistry {
	incarnation: [u8; 16],
	max_clients: usize,
	clients: Mutex<HashSet<ClientId>>,
}

impl ClientRegistry {
	/// Create a registry with a fresh random incarnation.
	pub fn new(max_clients: usize) -> Self {
		Self {
			incarnation: relay_io::entropy::random_bytes::<16>(),
			max_clients,
			clients: Mutex::new(HashSet::new()),
		}
	}

	/// The engine incarnation id clients must echo.
	pub fn incarnation(&self) -> [u8; 16] {
		self.incarnation
	}

	/// Allocate a new client id, failing with `RESOURCE_EXHAUSTED` at the cap.
	pub fn register(&self) -> Result<ClientId, Status> {
		let mut clients = self.clients.lock_unpoisoned();
		if clients.len() >= self.max_clients {
			return Err(Status::resource_exhausted("client limit reached"));
		}
		let id = relay_io::entropy::random_bytes::<16>();
		clients.insert(id);
		Ok(id)
	}

	/// Detach a client.
	pub fn remove(&self, client: &ClientId) {
		self.clients.lock_unpoisoned().remove(client);
	}

	/// Number of attached clients.
	pub fn len(&self) -> usize {
		self.clients.lock_unpoisoned().len()
	}

	/// True when no client is attached.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

impl ClientValidator for ClientRegistry {
	fn validate(&self, client: &ClientId, incarnation: Option<&[u8]>) -> Result<(), Status> {
		if let Some(seen) = incarnation
			&& seen != self.incarnation
		{
			return Err(Status::failed_precondition("ENGINE_INCARNATION_CHANGED"));
		}
		let known = self.clients.lock_unpoisoned().contains(client);
		if known {
			Ok(())
		} else {
			Err(Status::unauthenticated("client is not registered"))
		}
	}
}
