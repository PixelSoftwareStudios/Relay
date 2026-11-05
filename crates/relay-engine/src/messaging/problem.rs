//! Stable problem codes for messaging failures. Messages never carry text, names or paths.

use relay_api::pb::Problem;
use relay_messages::image::ImageError;
use relay_messages::sanitize::TextError;
use relay_messages::{StoreError, TransferError};

/// A refusal with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
	/// Machine code (`docs/api.md`).
	pub code: &'static str,
	/// Description without private content.
	pub message: String,
}

impl Refusal {
	/// A refusal.
	pub fn new(code: &'static str, message: impl Into<String>) -> Self {
		Self {
			code,
			message: message.into(),
		}
	}

	/// The wire problem.
	pub fn problem(&self) -> Problem {
		Problem {
			code: self.code.to_owned(),
			message: self.message.clone(),
			retryable: matches!(self.code, "RATE_LIMITED" | "PEER_UNAVAILABLE"),
			recovery_action: String::new(),
		}
	}
}

impl From<StoreError> for Refusal {
	fn from(error: StoreError) -> Self {
		let code = match &error {
			StoreError::Text(TextError::TooLong { .. }) => "TEXT_TOO_LONG",
			StoreError::Text(_) => "TEXT_INVALID",
			StoreError::Image(ImageError::TooLarge | ImageError::TooManyPixels { .. }) => {
				"IMAGE_TOO_LARGE"
			}
			StoreError::Image(ImageError::Unsupported) => "IMAGE_UNSUPPORTED",
			StoreError::Image(_) => "IMAGE_INVALID",
			StoreError::Disabled => "SESSION_PERMISSION_OFF",
			StoreError::BudgetExceeded { .. } => "HISTORY_FULL",
			StoreError::RateLimited => "RATE_LIMITED",
			StoreError::Ended => "SESSION_ENDED",
			StoreError::TooManyOffers => "TOO_MANY_OFFERS",
			StoreError::NotFound => "MESSAGE_NOT_FOUND",
			StoreError::WrongState => "MESSAGE_STATE",
			StoreError::BadSequence(_) | StoreError::QueryTooLong | StoreError::Invalid(_) => {
				"MESSAGE_STATE"
			}
		};
		// The store's Display never contains message content, but stay generic anyway.
		Self::new(code, error.to_string())
	}
}

impl From<TransferError> for Refusal {
	fn from(error: TransferError) -> Self {
		let code = match &error {
			TransferError::TooManyTransfers => "TOO_MANY_TRANSFERS",
			TransferError::NoSpace { .. } => "NO_SPACE",
			TransferError::Unknown => "OFFER_NOT_FOUND",
			_ => "OFFER_STATE",
		};
		Self::new(code, error.to_string())
	}
}
