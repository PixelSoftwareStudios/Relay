//! The one-line acknowledgement of a messaging mutation.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::SessionOpResult;
use serde::Serialize;

use crate::commands::names::hex_id;

/// What happened, for scripts and people.
#[derive(Debug, Serialize)]
pub struct Ack {
	pub action: &'static str,
	pub session: String,
	pub message: Option<u64>,
	pub seq: Option<u64>,
	pub offer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub detail: Option<String>,
}

impl Ack {
	/// From a mutation result.
	pub fn new(action: &'static str, result: &SessionOpResult) -> Self {
		Self {
			action,
			session: hex_id(&result.session_id),
			message: result.message.map(|m| m.ordinal),
			seq: result.message.map(|m| m.seq),
			offer: Some(hex_id(&result.offer_id)).filter(|id| !id.is_empty()),
			detail: None,
		}
	}

	/// Add a short note (a path, a count).
	pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
		self.detail = Some(detail.into());
		self
	}
}

impl Display for Ack {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(self.action)?;
		if let Some(message) = self.message {
			write!(f, " #{message}")?;
		}
		if let Some(offer) = &self.offer {
			write!(f, " offer {}", &offer[..offer.len().min(8)])?;
		}
		if let Some(detail) = &self.detail {
			write!(f, " {detail}")?;
		}
		Ok(())
	}
}
