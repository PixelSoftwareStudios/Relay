//! The session transcript that channel proofs are bound to.

use crate::session::SecureSession;

const DOMAIN: &str = "relay-net 2026 channel session binding v1";

/// Digest of the Noise handshake hash, which commits to the prologue (protocol major and the
/// fresh link epoch), both static public keys and every handshake message. A PAKE run, a
/// confirmation or a reference computed under one binding does not verify under another, so
/// nothing recorded on one link replays on another link, with other keys, or in the other
/// direction (the role is part of every reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionBinding {
	digest: [u8; 32],
	local_is_initiator: bool,
}

impl SessionBinding {
	/// Binding for an established session.
	pub fn for_session(session: &SecureSession) -> Self {
		Self::from_handshake_hash(
			session.handshake_hash(),
			session.role() == crate::session::Role::Initiator,
		)
	}

	/// Binding from a raw handshake hash (both ends hold the same one).
	pub fn from_handshake_hash(hash: &[u8; 32], local_is_initiator: bool) -> Self {
		Self {
			digest: blake3::derive_key(DOMAIN, hash),
			local_is_initiator,
		}
	}

	pub const fn digest(&self) -> &[u8; 32] {
		&self.digest
	}

	pub const fn local_is_initiator(&self) -> bool {
		self.local_is_initiator
	}

	/// The same link seen from the other end.
	pub const fn peer_view(&self) -> Self {
		Self {
			digest: self.digest,
			local_is_initiator: !self.local_is_initiator,
		}
	}
}
