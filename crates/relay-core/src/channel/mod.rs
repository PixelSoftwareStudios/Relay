//! Named channels: a scope for discovery, visibility and call authorization.
//!
//! This module is pure policy over values. It never holds a call id, never reads state that
//! belongs to the media path, and cannot express "mix several peers into one stream": a channel
//! is metadata about *peers*, and every call stays an independent 1:1 session. It also has no
//! way to grant microphone consent or accept a call; [`RingDecision`] has no accept variant.
//! Cryptographic proof of membership lives in `relay-net`; here a peer is only ever
//! [`Evidence::Candidate`] (recognised by an unauthenticated tag) or [`Evidence::Proven`].

mod membership;
mod name;
mod scope;

use std::fmt;

pub use membership::{ChannelEntry, LimitReached, Memberships, Role};
pub use name::{ChannelName, NameError};
pub use scope::{Evidence, InboundPolicy, PeerScope, RingDecision, visible};

/// Joined channels per device.
pub const CHANNELS_MAX: usize = 16;
/// Channel ids a single peer may be associated with at once (bounds `PeerScope`).
pub const PEER_CHANNELS_MAX: usize = 16;
/// Channel name length bounds, in Unicode scalar values.
pub const NAME_MIN_CHARS: usize = 1;
pub const NAME_MAX_CHARS: usize = 48;

/// 128-bit channel identifier. Treated as a discovery secret by `relay-net`, so `Debug` and
/// `Display` show only a short prefix.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId([u8; 16]);

impl ChannelId {
	pub const fn from_bytes(bytes: [u8; 16]) -> Self {
		Self(bytes)
	}

	pub const fn as_bytes(&self) -> &[u8; 16] {
		&self.0
	}

	/// Abbreviation safe to show in logs and lists.
	pub fn short(&self) -> String {
		self.0[..3].iter().map(|b| format!("{b:02x}")).collect()
	}
}

impl fmt::Debug for ChannelId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ChannelId({}..)", self.short())
	}
}

impl fmt::Display for ChannelId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}..", self.short())
	}
}

/// How a channel is protected and advertised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Policy {
	/// The name is advertised in clear. Unlisted channels advertise only blinded tags.
	pub listed: bool,
	/// Joining requires the channel password.
	pub password: bool,
	/// Joining requires an invite token.
	pub invite_only: bool,
}

impl Policy {
	/// Whether any secret beyond the channel id is needed to join.
	pub const fn is_protected(&self) -> bool {
		self.password || self.invite_only
	}
}

#[cfg(test)]
mod tests;
