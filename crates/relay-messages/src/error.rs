//! Typed errors.

use crate::image::ImageError;
use crate::sanitize::TextError;

/// Why a peer (or we) refused a message. Mirrors the wire `ChatAck.Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RejectReason {
	/// The permission for this kind is off.
	Disabled,
	/// Over a size limit.
	TooLarge,
	/// Failed validation.
	Invalid,
	/// Rate limit.
	RateLimited,
	/// Session history budget exhausted.
	HistoryFull,
	/// Kind not supported.
	Unsupported,
	/// Session has ended.
	Ended,
}

/// Errors from [`crate::SessionStore`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
	/// Text failed sanitization.
	#[error("text refused: {0}")]
	Text(#[from] TextError),
	/// Image failed validation.
	#[error("image refused: {0}")]
	Image(#[from] ImageError),
	/// The permission for this kind is off.
	#[error("this kind of item is disabled for the session")]
	Disabled,
	/// The session transcript is full.
	#[error("session history is full ({used} of {limit} bytes); export or clear it")]
	BudgetExceeded {
		/// Bytes used now.
		used: u64,
		/// Budget.
		limit: u64,
	},
	/// A rate limit was hit.
	#[error("rate limit exceeded")]
	RateLimited,
	/// The session has ended.
	#[error("session has ended")]
	Ended,
	/// Remote sequence went backwards or repeated with different content.
	#[error("sequence {0} is not newer than the last seen")]
	BadSequence(u64),
	/// Unknown message or offer.
	#[error("no such item")]
	NotFound,
	/// The item has the wrong kind or state for this operation.
	#[error("operation not valid for this item")]
	WrongState,
	/// Too many file offers in the session.
	#[error("too many file offers in this session")]
	TooManyOffers,
	/// Search query too long.
	#[error("search query too long")]
	QueryTooLong,
	/// Metadata invalid (sizes, hashes).
	#[error("invalid metadata: {0}")]
	Invalid(&'static str),
}

impl StoreError {
	/// The wire reason a receiver reports for this error.
	pub fn reject_reason(&self) -> RejectReason {
		match self {
			Self::Disabled => RejectReason::Disabled,
			Self::BudgetExceeded { .. } => RejectReason::HistoryFull,
			Self::RateLimited | Self::TooManyOffers => RejectReason::RateLimited,
			Self::Ended => RejectReason::Ended,
			Self::Text(TextError::TooLong { .. }) => RejectReason::TooLarge,
			Self::Image(ImageError::TooLarge | ImageError::TooManyPixels { .. }) => {
				RejectReason::TooLarge
			}
			Self::Image(ImageError::Unsupported) => RejectReason::Unsupported,
			_ => RejectReason::Invalid,
		}
	}
}

/// Errors from the transfer state machine.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransferError {
	/// Input not valid in the current state.
	#[error("not valid in the current transfer state")]
	WrongState,
	/// Two transfers are already running.
	#[error("too many concurrent transfers")]
	TooManyTransfers,
	/// Not enough free space.
	#[error("not enough free space (need {needed}, have {available})")]
	NoSpace {
		/// Bytes needed including margin.
		needed: u64,
		/// Bytes available.
		available: u64,
	},
	/// Chunk out of order or outside the window.
	#[error("chunk offset {got} but expected {expected}")]
	BadOffset {
		/// Offset received.
		got: u64,
		/// Offset expected.
		expected: u64,
	},
	/// Chunk too large or past the end of the file.
	#[error("chunk exceeds its limit")]
	BadChunk,
	/// Unknown transfer id.
	#[error("unknown transfer")]
	Unknown,
	/// Invalid offer metadata.
	#[error("invalid offer: {0}")]
	InvalidOffer(&'static str),
}
