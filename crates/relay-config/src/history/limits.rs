//! Every retention and paging limit of the call history, in one place.

/// Most entries kept; older ones are deleted when a new one is written.
pub const MAX_ENTRIES: usize = 200;
/// Longest age kept, in milliseconds (90 days).
pub const MAX_AGE_MS: u64 = 90 * 24 * 60 * 60 * 1000;
/// Largest page `list` returns.
pub const MAX_LIST: usize = 200;
/// Page size when a caller asks for none.
pub const DEFAULT_LIST: usize = 50;
/// Longest stored peer name, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Longest stored short fingerprint, in characters.
pub const MAX_SHORT_CHARS: usize = 32;

/// The retention limits a [`super::CallHistory`] enforces when it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
	pub max_entries: usize,
	pub max_age_ms: u64,
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			max_entries: MAX_ENTRIES,
			max_age_ms: MAX_AGE_MS,
		}
	}
}
