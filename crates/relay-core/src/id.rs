//! Distinct identifier, generation and revision types.
//!
//! Every identifier is its own newtype so engine logic cannot confuse, say,
//! a [`CallId`] with an [`OperationId`]. Counters never wrap: `next` returns
//! `None` on exhaustion so the owning object can be ended instead.

use std::fmt;

macro_rules! wide_id {
	($(#[$meta:meta])* $name:ident) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
		pub struct $name(u128);

		impl $name {
			/// Wraps a raw 128-bit value (supplied by an entropy adapter).
			pub const fn from_u128(value: u128) -> Self {
				Self(value)
			}

			/// Returns the raw 128-bit value.
			pub const fn as_u128(self) -> u128 {
				self.0
			}

			/// Big-endian byte form, used for hashing and wire mapping.
			pub const fn to_bytes(self) -> [u8; 16] {
				self.0.to_be_bytes()
			}

			/// First 8 hex digits: enough to follow one call in a log, not to identify it.
			pub fn short(self) -> String {
				format!("{:08x}", self.0 >> 96)
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				write!(f, "{:032x}", self.0)
			}
		}
	};
}

macro_rules! counter {
	($(#[$meta:meta])* $name:ident) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
		pub struct $name(u64);

		impl $name {
			/// The initial value.
			pub const ZERO: Self = Self(0);

			/// Wraps a raw counter value.
			pub const fn new(value: u64) -> Self {
				Self(value)
			}

			/// Returns the raw value.
			pub const fn get(self) -> u64 {
				self.0
			}

			/// The following value, or `None` once the counter is exhausted.
			pub fn next(self) -> Option<Self> {
				self.0.checked_add(1).map(Self)
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				write!(f, "{}", self.0)
			}
		}
	};
}

wide_id!(
	/// Unpredictable identifier of a call (also the invitation identifier).
	CallId
);
wide_id!(
	/// Identifier of a mutation request, scoped to one client session.
	OperationId
);
wide_id!(
	/// Identifier of a local API client session.
	ClientId
);

impl ClientId {
	/// The engine's own client: it holds presence while background availability is on and no
	/// shell is attached, and owns calls it answers by itself. Its controller lease can be
	/// taken by any client without an explicit takeover. Real clients get random ids.
	pub const HOUSE: Self = Self(u128::MAX);
}

wide_id!(
	/// Identifier of one call session negotiated between two engines.
	SessionId
);
wide_id!(
	/// Random per-process identifier that invalidates handles from older runs.
	Incarnation
);

/// Digest of an authenticated static public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceKeyId([u8; 32]);

impl DeviceKeyId {
	/// Wraps a 32-byte digest.
	pub const fn from_bytes(bytes: [u8; 32]) -> Self {
		Self(bytes)
	}

	/// Returns the digest bytes.
	pub const fn as_bytes(&self) -> &[u8; 32] {
		&self.0
	}

	/// First 8 hex digits: enough to follow one peer in a log, not to identify it.
	pub fn short(&self) -> String {
		self.0[..4]
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect()
	}
}

impl fmt::Display for DeviceKeyId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
	}
}

/// A remote peer is identified by its authenticated device key.
pub type PeerId = DeviceKeyId;

counter!(
	/// Identifies one grant of controller ownership over a call.
	ControllerLeaseId
);
counter!(
	/// Identifies one renewable push-to-talk lease.
	InputLeaseId
);
counter!(
	/// Identifies a push-to-talk source (hotkey backend, terminal, helper).
	InputSourceId
);
counter!(
	/// Generation of the shared capture path; stale completions are rejected.
	AudioGeneration
);
counter!(
	/// Generation of a call's transmit authorization.
	RouteGeneration
);
counter!(
	/// Monotonic lifecycle event revision.
	Revision
);
counter!(
	/// Revision of engine-wide settings, used for conflict detection.
	SettingsRevision
);
