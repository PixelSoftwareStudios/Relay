//! Traffic classes for the engine and net layers.

pub use crate::limits::BULK_RATE_BYTES_PER_SEC;

/// Scheduling class, highest priority first (`Voice < ... < BulkFile` by [`Ord`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrafficClass {
	/// Call audio.
	Voice,
	/// Reserved; unused.
	Video,
	/// Call signaling, chat text, acks, offers and transfer control.
	Signaling,
	/// Text previews and inline images above the interactive size.
	Preview,
	/// File bytes: paced and always lowest priority.
	BulkFile,
}

impl TrafficClass {
	/// Numeric priority; lower is more urgent.
	pub fn priority(self) -> u8 {
		self as u8
	}

	/// True for classes that must be rate-limited.
	pub fn is_bulk(self) -> bool {
		self == Self::BulkFile
	}

	/// Class for a chat payload of `len` bytes (images larger than 64 KiB are `Preview`).
	pub fn for_chat_payload(len: usize) -> Self {
		if len <= 64 * 1024 {
			Self::Signaling
		} else {
			Self::Preview
		}
	}
}
