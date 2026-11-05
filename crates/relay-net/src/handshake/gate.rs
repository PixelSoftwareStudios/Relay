use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::HandshakeError;
use crate::limits::PENDING_HANDSHAKES_MAX;

/// Caps concurrent unauthenticated handshakes. Acquire before any expensive work.
#[derive(Debug, Clone)]
pub struct HandshakeGate {
	slots: Arc<Semaphore>,
}

/// Holds one pending-handshake slot until dropped.
#[derive(Debug)]
pub struct HandshakePermit(#[allow(dead_code)] OwnedSemaphorePermit);

impl HandshakeGate {
	pub fn new() -> Self {
		Self::with_capacity(PENDING_HANDSHAKES_MAX)
	}

	pub fn with_capacity(capacity: usize) -> Self {
		Self {
			slots: Arc::new(Semaphore::new(capacity)),
		}
	}

	/// Takes a slot or fails immediately; callers drop the connection on failure.
	pub fn try_enter(&self) -> Result<HandshakePermit, HandshakeError> {
		Arc::clone(&self.slots)
			.try_acquire_owned()
			.map(HandshakePermit)
			.map_err(|_| HandshakeError::Overloaded)
	}
}

impl Default for HandshakeGate {
	fn default() -> Self {
		Self::new()
	}
}
