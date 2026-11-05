//! A cheap syntactic check of `relay://<ip>:<port>#<64 hex fingerprint>`.
//!
//! This only spares the user a round trip for obvious typos. The engine
//! applies the real LAN-only address policy and key pinning, and is the
//! authority on whether a string is acceptable.

use std::net::SocketAddr;

/// Why a pasted string was rejected locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStringProblem {
	/// Does not start with `relay://`.
	Scheme,
	/// No `#fingerprint` part.
	MissingFingerprint,
	/// Fingerprint is not 64 hexadecimal characters.
	BadFingerprint,
	/// Address is not a literal `ip:port` (names are not accepted).
	BadAddress,
}

impl ConnectionStringProblem {
	/// User-facing explanation.
	pub fn message(self) -> &'static str {
		match self {
			Self::Scheme => "A connection string starts with relay://",
			Self::MissingFingerprint => "The connection string is missing its #fingerprint part.",
			Self::BadFingerprint => "The fingerprint must be 64 hexadecimal characters.",
			Self::BadAddress => "The address must be a literal ip:port, not a name.",
		}
	}
}

/// Longest text accepted, far above any real string.
const MAX_LEN: usize = 256;

/// Check the shape of a pasted connection string; surrounding space is ignored.
pub fn check(text: &str) -> Result<(), ConnectionStringProblem> {
	let text = text.trim();
	let rest = text
		.strip_prefix("relay://")
		.filter(|_| text.len() <= MAX_LEN)
		.ok_or(ConnectionStringProblem::Scheme)?;
	let (address, fingerprint) = rest
		.split_once('#')
		.ok_or(ConnectionStringProblem::MissingFingerprint)?;
	if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
		return Err(ConnectionStringProblem::BadFingerprint);
	}
	address
		.parse::<SocketAddr>()
		.map(drop)
		.map_err(|_| ConnectionStringProblem::BadAddress)
}
