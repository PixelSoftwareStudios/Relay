//! Secure-datagram packet-number space.
//!
//! This is deliberately independent of the RTP sequence number: it is a wide
//! per-lane counter that never wraps within a key epoch and never resets
//! across talk-spurt generations. Its bytes are intended as AEAD nonce
//! material and as the replay-window index.

use crate::error::PacketNumberError;
use crate::seq::{ReplayVerdict, ReplayWindow};

/// Default records allowed per lane before a rekey is mandatory.
pub const DEFAULT_MAX_RECORDS: u64 = 1 << 32;
/// Replay window size in packets (16 words of 64 bits).
pub const REPLAY_WINDOW_PACKETS: u64 = ReplayWindow::<16>::SIZE;

/// A 64-bit packet number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PacketNumber(pub u64);

impl PacketNumber {
	/// 12-byte nonce: 4-byte big-endian `lane` followed by the 8-byte number.
	/// Distinct lanes (direction / stream) under one key never collide.
	pub fn nonce(self, lane: u32) -> [u8; 12] {
		let mut nonce = [0; 12];
		nonce[..4].copy_from_slice(&lane.to_be_bytes());
		nonce[4..].copy_from_slice(&self.0.to_be_bytes());
		nonce
	}

	/// Inverse of [`Self::nonce`].
	pub fn from_nonce(nonce: &[u8; 12]) -> (u32, Self) {
		let lane = u32::from_be_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]);
		let n = u64::from_be_bytes([
			nonce[4], nonce[5], nonce[6], nonce[7], nonce[8], nonce[9], nonce[10], nonce[11],
		]);
		(lane, Self(n))
	}
}

/// Sender-side allocator: strictly increasing, never reused.
#[derive(Debug, Clone)]
pub struct PacketNumberSpace {
	next: u64,
	limit: u64,
}

impl Default for PacketNumberSpace {
	fn default() -> Self {
		Self::with_limit(DEFAULT_MAX_RECORDS)
	}
}

impl PacketNumberSpace {
	pub const fn with_limit(limit: u64) -> Self {
		Self { next: 0, limit }
	}

	/// Allocates the next number, or fails once the epoch budget is spent.
	pub fn allocate(&mut self) -> Result<PacketNumber, PacketNumberError> {
		if self.next >= self.limit {
			return Err(PacketNumberError::Exhausted);
		}
		let n = PacketNumber(self.next);
		self.next += 1;
		Ok(n)
	}

	/// Numbers still available before rekey.
	pub const fn remaining(&self) -> u64 {
		self.limit - self.next
	}
}

/// Receiver-side replay protection over packet numbers.
#[derive(Debug, Clone, Default)]
pub struct PacketReplay {
	window: ReplayWindow<16>,
}

impl PacketReplay {
	pub fn new() -> Self {
		Self::default()
	}

	/// Cheap pre-authentication check.
	pub fn check(&self, n: PacketNumber) -> ReplayVerdict {
		self.window.check(n.0)
	}

	/// Marks `n` as seen. Call only after the AEAD tag verified.
	pub fn accept(&mut self, n: PacketNumber) {
		self.window.accept(n.0);
	}
}
