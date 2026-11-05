//! Identifier minting. The domain never generates IDs; this adapter does.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use relay_core::{CallId, ClientId, Incarnation, OperationId, SessionId};

/// A source of 128-bit random values.
pub trait Entropy: Send + Sync {
	/// A fresh value.
	fn next_u128(&self) -> u128;
}

/// Operating-system seeded randomness.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
	fn next_u128(&self) -> u128 {
		relay_io::entropy::OsEntropy.next_u128()
	}
}

/// Deterministic counter for tests. Never use it outside tests.
#[derive(Debug, Default)]
pub struct CounterEntropy {
	next: AtomicU64,
}

impl Entropy for CounterEntropy {
	fn next_u128(&self) -> u128 {
		u128::from(self.next.fetch_add(1, Ordering::Relaxed) + 1)
	}
}

/// Mints the identifiers the domain consumes.
///
/// Call and session identifiers for a `StartCall` are derived from the
/// caller's `(client, operation)` with a process-secret key, so a retry of the
/// same operation yields the same call while staying unpredictable to others.
#[derive(Clone)]
pub struct Ids {
	entropy: Arc<dyn Entropy>,
	key: [u8; 32],
}

impl fmt::Debug for Ids {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Ids").finish_non_exhaustive()
	}
}

impl Ids {
	/// Builds the minter and draws its derivation key.
	pub fn new(entropy: Arc<dyn Entropy>) -> Self {
		let mut key = [0u8; 32];
		key[..16].copy_from_slice(&entropy.next_u128().to_be_bytes());
		key[16..].copy_from_slice(&entropy.next_u128().to_be_bytes());
		Self { entropy, key }
	}

	/// A fresh local client identifier.
	pub fn client(&self) -> ClientId {
		ClientId::from_u128(self.entropy.next_u128())
	}

	/// A fresh engine-internal operation identifier.
	pub fn operation(&self) -> OperationId {
		OperationId::from_u128(self.entropy.next_u128())
	}

	/// A fresh incarnation.
	pub fn incarnation(&self) -> Incarnation {
		Incarnation::from_u128(self.entropy.next_u128())
	}

	/// The call identifier belonging to a `StartCall` operation.
	pub fn call_for(&self, client: ClientId, operation: OperationId) -> CallId {
		CallId::from_u128(self.derive(b"call", client, operation))
	}

	/// The session identifier belonging to a `StartCall` operation.
	pub fn session_for(&self, client: ClientId, operation: OperationId) -> SessionId {
		SessionId::from_u128(self.derive(b"session", client, operation))
	}

	fn derive(&self, label: &[u8], client: ClientId, operation: OperationId) -> u128 {
		let mut hasher = blake3::Hasher::new_keyed(&self.key);
		hasher.update(label);
		hasher.update(&client.to_bytes());
		hasher.update(&operation.to_bytes());
		let digest = hasher.finalize();
		let mut bytes = [0u8; 16];
		bytes.copy_from_slice(&digest.as_bytes()[..16]);
		u128::from_be_bytes(bytes)
	}
}
