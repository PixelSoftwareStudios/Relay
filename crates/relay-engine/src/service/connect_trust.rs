//! Why `ConnectAndTrust` failed. Every case leaves the trust store untouched.

use thiserror::Error;

/// A failed attempt to dial a scanned connection string and verify its key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConnectFailure {
	/// The text is not a `relay://host:port#<64 hex digits>` string, or its address is
	/// refused by the address policy.
	#[error("{0}")]
	InvalidString(String),
	/// The device at the address presented a different key than the scanned fingerprint.
	#[error("the device at that address does not have the scanned fingerprint")]
	Mismatch,
	/// Nobody answered at the address, or the handshake did not complete.
	#[error("could not reach a device at that address: {0}")]
	Unreachable(String),
}

impl ConnectFailure {
	/// The stable machine code of the failed operation's problem.
	pub fn code(&self) -> &'static str {
		match self {
			Self::InvalidString(_) => "CONNECT_STRING_INVALID",
			Self::Mismatch => "CONNECT_FINGERPRINT_MISMATCH",
			Self::Unreachable(_) => "CONNECT_UNREACHABLE",
		}
	}

	/// Whether trying again could succeed.
	pub fn retryable(&self) -> bool {
		matches!(self, Self::Unreachable(_))
	}
}
