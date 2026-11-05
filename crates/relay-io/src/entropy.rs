//! Operating-system seeded randomness.

use rand::RngExt;

/// `N` fresh random bytes from the operating system's CSPRNG.
pub fn random_bytes<const N: usize>() -> [u8; N] {
	let mut bytes = [0u8; N];
	OsEntropy.fill(&mut bytes);
	bytes
}

/// `N` fresh random bytes as lowercase hex (`2 * N` characters).
pub fn random_hex<const N: usize>() -> String {
	hex::encode(random_bytes::<N>())
}

/// Randomness from the operating system's CSPRNG (via the thread-local generator
/// that the `rand` crate seeds from it).
#[derive(Debug, Default, Clone, Copy)]
pub struct OsEntropy;

impl OsEntropy {
	/// A fresh 128-bit value.
	pub fn next_u128(&self) -> u128 {
		let mut bytes = [0u8; 16];
		self.fill(&mut bytes);
		u128::from_be_bytes(bytes)
	}

	/// Fills `buf` with random bytes.
	pub fn fill(&self, buf: &mut [u8]) {
		rand::rng().fill(buf);
	}
}

impl relay_messages::IdSource for OsEntropy {
	fn next_u128(&mut self) -> u128 {
		OsEntropy::next_u128(self)
	}
}
