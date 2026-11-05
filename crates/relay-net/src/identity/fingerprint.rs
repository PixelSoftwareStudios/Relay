//! Public-key fingerprints and human-comparable formatting.

use std::fmt;

use thiserror::Error;

use super::{KEY_BYTES, PublicKey};

const DOMAIN: &str = "relay-net 2026 identity fingerprint v1";
const SHORT_BYTES: usize = 10;

/// 256-bit BLAKE3 digest of a static public key (domain separated).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FingerprintParseError {
	#[error("fingerprint must contain exactly 64 hex digits")]
	Length,
	#[error("fingerprint contains a non-hex character")]
	Character,
}

impl Fingerprint {
	pub fn of_public_key(key: &PublicKey) -> Self {
		let key_bytes: &[u8; KEY_BYTES] = key.as_bytes();
		Self(blake3::derive_key(DOMAIN, key_bytes))
	}

	pub const fn from_bytes(bytes: [u8; 32]) -> Self {
		Self(bytes)
	}

	pub const fn as_bytes(&self) -> &[u8; 32] {
		&self.0
	}

	/// Short form for telling devices apart in lists, e.g. `A1B2-C3D4-E5F6-0718-293A`
	/// (80 bits). It is a disambiguation hint, not a verification string.
	pub fn short(&self) -> String {
		group_hex(&self.0[..SHORT_BYTES], 2, '-')
	}

	/// Full form for out-of-band comparison: 16 groups of four hex digits.
	/// Verification must compare this whole string, never only the short form.
	pub fn full(&self) -> String {
		group_hex(&self.0, 2, ' ')
	}

	/// Parses the output of [`Self::full`]; separators and case are ignored.
	pub fn parse(text: &str) -> Result<Self, FingerprintParseError> {
		let mut digits = Vec::with_capacity(64);
		for c in text.chars().filter(|c| !matches!(c, ' ' | '-' | ':')) {
			digits.push(c.to_digit(16).ok_or(FingerprintParseError::Character)? as u8);
		}
		if digits.len() != 64 {
			return Err(FingerprintParseError::Length);
		}
		let mut out = [0u8; 32];
		for (byte, pair) in out.iter_mut().zip(digits.chunks_exact(2)) {
			*byte = pair[0] << 4 | pair[1];
		}
		Ok(Self(out))
	}
}

fn group_hex(bytes: &[u8], group: usize, sep: char) -> String {
	let mut out = String::with_capacity(bytes.len() * 3);
	for (i, chunk) in bytes.chunks(group).enumerate() {
		if i > 0 {
			out.push(sep);
		}
		for b in chunk {
			out.push_str(&format!("{b:02X}"));
		}
	}
	out
}

impl fmt::Display for Fingerprint {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.full())
	}
}

impl fmt::Debug for Fingerprint {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Fingerprint({})", self.short())
	}
}
