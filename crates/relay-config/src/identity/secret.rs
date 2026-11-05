//! Generic 32-byte secret and identity keypair containers (no dependency on the net crate).

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::error::{ConfigError, Result};

/// A 32-byte secret that is zeroed on drop and never printed.
#[derive(Clone)]
pub struct Secret32(Zeroizing<[u8; 32]>);

impl Secret32 {
	pub fn new(bytes: [u8; 32]) -> Self {
		Self(Zeroizing::new(bytes))
	}

	pub fn expose(&self) -> &[u8; 32] {
		&self.0
	}
}

impl std::fmt::Debug for Secret32 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Secret32(<redacted>)")
	}
}

impl PartialEq for Secret32 {
	/// Constant-time comparison.
	fn eq(&self, other: &Self) -> bool {
		self.0.ct_eq(&*other.0).into()
	}
}

/// A local identity: the private key and the matching public key.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredIdentity {
	pub secret: Secret32,
	pub public: [u8; 32],
}

impl StoredIdentity {
	pub(crate) const ENCODED_LEN: usize = 64;

	/// Encodes as `secret || public` inside a zeroizing buffer.
	pub(crate) fn encode(&self) -> Zeroizing<Vec<u8>> {
		let mut out = Zeroizing::new(Vec::with_capacity(Self::ENCODED_LEN));
		out.extend_from_slice(self.secret.expose());
		out.extend_from_slice(&self.public);
		out
	}

	pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
		if bytes.len() != Self::ENCODED_LEN {
			return Err(ConfigError::IdentityMalformed(format!(
				"expected {} bytes, found {}",
				Self::ENCODED_LEN,
				bytes.len()
			)));
		}
		let mut secret = [0u8; 32];
		let mut public = [0u8; 32];
		secret.copy_from_slice(&bytes[..32]);
		public.copy_from_slice(&bytes[32..]);
		let identity = Self {
			secret: Secret32::new(secret),
			public,
		};
		zeroize::Zeroize::zeroize(&mut secret);
		Ok(identity)
	}
}
