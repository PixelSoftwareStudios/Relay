use rand::RngExt;

use crate::limits::EPOCH_BYTES;

/// Fresh random 128-bit identifier of one handshake's key generation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkEpoch([u8; EPOCH_BYTES]);

impl LinkEpoch {
	pub fn random() -> Self {
		let mut bytes = [0u8; EPOCH_BYTES];
		rand::rng().fill(&mut bytes[..]);
		Self(bytes)
	}

	pub const fn from_bytes(bytes: [u8; EPOCH_BYTES]) -> Self {
		Self(bytes)
	}

	pub fn from_slice(bytes: &[u8]) -> Option<Self> {
		bytes.try_into().ok().map(Self)
	}

	pub const fn as_bytes(&self) -> &[u8; EPOCH_BYTES] {
		&self.0
	}
}

impl std::fmt::Debug for LinkEpoch {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "LinkEpoch({:02x}{:02x}..)", self.0[0], self.0[1])
	}
}
