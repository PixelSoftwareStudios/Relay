//! Injected environment: the crate never reads clocks or randomness itself.

/// Time source.
pub trait Clock: Send + Sync {
	/// Wall-clock milliseconds since the Unix epoch (display and export only).
	fn wall_ms(&self) -> u64;
	/// Monotonic milliseconds (rate limits, ack timers).
	fn mono_ms(&self) -> u64;
}

/// Source of random identifiers.
pub trait IdSource: Send {
	/// A fresh 128-bit identifier.
	fn next_u128(&mut self) -> u128;
}
