//! The session transcript that channel proofs are bound to.

use crate::identity::PublicKey;
use crate::session::{LinkEpoch, Role, SecureSession};

const DOMAIN: &str = "relay-net 2026 channel session binding v1";

/// Digest of the fresh link epoch (which is inside the Noise prologue) and both static public
/// keys, in initiator/responder order. A PAKE run, a confirmation or a reference computed
/// under one binding does not verify under another, so nothing recorded on one link replays on
/// another link, with other keys, or in the other direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionBinding {
	digest: [u8; 32],
	local_is_initiator: bool,
}

impl SessionBinding {
	pub fn new(
		epoch: &LinkEpoch,
		initiator: &PublicKey,
		responder: &PublicKey,
		local_is_initiator: bool,
	) -> Self {
		let mut hasher = blake3::Hasher::new_derive_key(DOMAIN);
		hasher.update(epoch.as_bytes());
		hasher.update(initiator.as_bytes());
		hasher.update(responder.as_bytes());
		Self {
			digest: *hasher.finalize().as_bytes(),
			local_is_initiator,
		}
	}

	/// Builds the binding for an established session; `local` is our own static public key.
	pub fn for_session(session: &SecureSession, local: &PublicKey) -> Self {
		match session.role() {
			Role::Initiator => Self::new(session.epoch(), local, session.remote_key(), true),
			Role::Responder => Self::new(session.epoch(), session.remote_key(), local, false),
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
