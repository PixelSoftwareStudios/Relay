//! Device identity: a static X25519 key used as the Noise static key.

mod fingerprint;
mod trust;

use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::types::Dh;
use thiserror::Error;
use zeroize::Zeroizing;

pub use fingerprint::{Fingerprint, FingerprintParseError};
pub use trust::{
	InMemoryTrustStore, PeerTrust, TrustError, TrustRecord, TrustState, TrustStore, evaluate_peer,
};

pub(crate) const KEY_BYTES: usize = 32;

/// Identity operation failures.
#[derive(Debug, Error)]
pub enum IdentityError {
	#[error("key generation failed")]
	Generate,
	#[error("invalid private key")]
	InvalidKey,
	#[error("remote static key is non-canonical or a low-order point")]
	InvalidRemoteKey,
}

/// Rejects remote static keys that are not a proper identity: a set bit 255 (X25519 ignores it,
/// so one private key would present as two fingerprints) or a low-order point (the Diffie-Hellman
/// output with our own key is all zero, so the key authenticates nobody).
pub fn check_remote_key(local: &Identity, remote: &PublicKey) -> Result<(), IdentityError> {
	if remote.as_bytes()[KEY_BYTES - 1] & 0x80 != 0 {
		return Err(IdentityError::InvalidRemoteKey);
	}
	let mut dh: Box<dyn Dh> = DefaultResolver
		.resolve_dh(&snow::params::DHChoice::Curve25519)
		.ok_or(IdentityError::InvalidKey)?;
	dh.set(local.secret_bytes());
	let mut out = [0u8; KEY_BYTES];
	dh.dh(remote.as_bytes(), &mut out)
		.map_err(|_| IdentityError::InvalidRemoteKey)?;
	if out.iter().all(|b| *b == 0) {
		return Err(IdentityError::InvalidRemoteKey);
	}
	Ok(())
}

/// A remote or local static public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; KEY_BYTES]);

impl PublicKey {
	pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
		Self(bytes)
	}

	pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
		&self.0
	}

	/// The stable device identifier derived from this key.
	pub fn fingerprint(&self) -> Fingerprint {
		Fingerprint::of_public_key(self)
	}
}

impl std::fmt::Debug for PublicKey {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "PublicKey({})", self.fingerprint().short())
	}
}

/// The local device identity. The private key is zeroized on drop and never printed.
pub struct Identity {
	secret: Zeroizing<[u8; KEY_BYTES]>,
	public: PublicKey,
}

impl Identity {
	/// Generates a fresh identity from OS randomness.
	pub fn generate() -> Result<Self, IdentityError> {
		let keypair = crate::noise::builder()
			.generate_keypair()
			.map_err(|_| IdentityError::Generate)?;
		let secret: [u8; KEY_BYTES] = keypair
			.private
			.as_slice()
			.try_into()
			.map_err(|_| IdentityError::Generate)?;
		Self::from_secret_bytes(secret)
	}

	/// Restores an identity from a stored private key, re-deriving the public key.
	pub fn from_secret_bytes(secret: [u8; KEY_BYTES]) -> Result<Self, IdentityError> {
		let secret = Zeroizing::new(secret);
		let mut dh: Box<dyn Dh> = DefaultResolver
			.resolve_dh(&snow::params::DHChoice::Curve25519)
			.ok_or(IdentityError::InvalidKey)?;
		dh.set(&secret[..]);
		let public: [u8; KEY_BYTES] = dh
			.pubkey()
			.try_into()
			.map_err(|_| IdentityError::InvalidKey)?;
		Ok(Self {
			secret,
			public: PublicKey(public),
		})
	}

	pub const fn public(&self) -> &PublicKey {
		&self.public
	}

	pub fn fingerprint(&self) -> Fingerprint {
		self.public.fingerprint()
	}

	/// Private key bytes, for persistence into a credential store by the caller only.
	pub fn secret_bytes(&self) -> &[u8; KEY_BYTES] {
		&self.secret
	}
}

impl std::fmt::Debug for Identity {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Identity({})", self.fingerprint().short())
	}
}
