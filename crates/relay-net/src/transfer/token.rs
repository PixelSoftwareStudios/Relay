//! One-time transfer tokens and the registry that redeems them.
//!
//! The listener side of a transfer mints a token, sends it over the authenticated control link
//! and registers what it authorises: one offer, one session, one peer key. A data link is
//! accepted only when its Noise handshake proves that exact key and its payload carries the
//! token. A token is removed the moment it is redeemed, expires on its own, is revoked after a
//! few wrong attempts by the right peer, and is never logged.

use relay_io::LockExt;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rand::RngExt;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::Instant;
use zeroize::Zeroize;

use super::DataError;
use super::limits::{TOKEN_ATTEMPTS_MAX, TOKEN_TTL_MAX, TOKENS_OUTSTANDING_MAX};
use super::link::{DataLink, TransferRole};
use crate::identity::Fingerprint;

/// Token length on the wire.
pub const TOKEN_BYTES: usize = 32;

/// 256 random bits authorising exactly one data link.
#[derive(Clone, PartialEq, Eq)]
pub struct TransferToken([u8; TOKEN_BYTES]);

impl TransferToken {
	pub fn random() -> Self {
		let mut bytes = [0u8; TOKEN_BYTES];
		rand::rng().fill(&mut bytes[..]);
		Self(bytes)
	}

	pub fn from_slice(bytes: &[u8]) -> Option<Self> {
		bytes.try_into().ok().map(Self)
	}

	pub const fn as_bytes(&self) -> &[u8; TOKEN_BYTES] {
		&self.0
	}

	/// Digest compared in constant time and kept instead of the token itself.
	pub(crate) fn digest(&self) -> blake3::Hash {
		blake3::keyed_hash(&[0x5a; 32], &self.0)
	}
}

impl Drop for TransferToken {
	fn drop(&mut self) {
		self.0.zeroize();
	}
}

impl std::fmt::Debug for TransferToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("TransferToken(<redacted>)")
	}
}

/// What a token authorises, fixed when it is issued.
#[derive(Debug, Clone)]
pub struct Expectation {
	/// 16-byte offer identifier.
	pub offer_id: [u8; 16],
	/// 16-byte session identifier (engine-local meaning).
	pub session_id: [u8; 16],
	/// The only peer allowed to redeem the token.
	pub peer: Fingerprint,
	/// Role of the redeeming side's counterpart, that is, of the link we will hold.
	pub role: TransferRole,
	/// Declared file size; frames are validated against it.
	pub size: u64,
}

pub(crate) struct Claimed {
	pub expectation: Expectation,
	pub deliver: oneshot::Sender<DataLink<TcpStream>>,
}

struct Entry {
	expectation: Expectation,
	digest: blake3::Hash,
	expires: Instant,
	failures: u8,
	deliver: oneshot::Sender<DataLink<TcpStream>>,
}

/// Outstanding tokens of one hub.
#[derive(Default)]
pub(crate) struct Registry {
	entries: Mutex<HashMap<[u8; 16], Entry>>,
}

impl Registry {
	pub(crate) fn issue(
		&self,
		expectation: Expectation,
		ttl: Duration,
	) -> Result<
		(
			TransferToken,
			Instant,
			oneshot::Receiver<DataLink<TcpStream>>,
		),
		DataError,
	> {
		let now = Instant::now();
		let mut entries = self.entries.lock_unpoisoned();
		entries.retain(|_, e| e.expires > now && !e.deliver.is_closed());
		if entries.len() >= TOKENS_OUTSTANDING_MAX || entries.contains_key(&expectation.offer_id) {
			return Err(DataError::TooManyPending);
		}
		let token = TransferToken::random();
		let expires = now + ttl.min(TOKEN_TTL_MAX);
		let (deliver, receive) = oneshot::channel();
		entries.insert(
			expectation.offer_id,
			Entry {
				digest: token.digest(),
				expectation,
				expires,
				failures: 0,
				deliver,
			},
		);
		Ok((token, expires, receive))
	}

	pub(crate) fn revoke(&self, offer_id: &[u8; 16]) {
		self.entries.lock_unpoisoned().remove(offer_id);
	}

	/// Redeems a presented token. Every failure looks the same to the dialer (the connection
	/// just closes). The token is consumed only on success.
	pub(crate) fn claim(
		&self,
		peer: &Fingerprint,
		session_id: &[u8; 16],
		offer_id: &[u8; 16],
		token: &TransferToken,
	) -> Result<Claimed, DataError> {
		let mut entries = self.entries.lock_unpoisoned();
		let entry = entries.get_mut(offer_id).ok_or(DataError::TokenRejected)?;
		if entry.expires <= Instant::now() || entry.deliver.is_closed() {
			entries.remove(offer_id);
			return Err(DataError::TokenRejected);
		}
		// A different key never touches the token: strangers cannot burn it.
		if &entry.expectation.peer != peer {
			return Err(DataError::TokenRejected);
		}
		if &entry.expectation.session_id != session_id || entry.digest != token.digest() {
			entry.failures += 1;
			if entry.failures >= TOKEN_ATTEMPTS_MAX {
				entries.remove(offer_id);
			}
			return Err(DataError::TokenRejected);
		}
		let entry = entries.remove(offer_id).ok_or(DataError::TokenRejected)?;
		Ok(Claimed {
			expectation: entry.expectation,
			deliver: entry.deliver,
		})
	}

	pub(crate) fn len(&self) -> usize {
		self.entries.lock_unpoisoned().len()
	}
}
