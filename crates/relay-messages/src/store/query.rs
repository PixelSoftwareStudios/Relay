//! Pins, search and copy accessors.

use super::SessionStore;
use crate::error::StoreError;
use crate::limits::{MAX_SEARCH_QUERY_BYTES, MAX_SEARCH_RESULTS};
use crate::markdown;
use crate::message::{MessageBody, MessageId};

/// Search outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
	/// Matching ids in transcript order, at most 100.
	pub hits: Vec<MessageId>,
	/// More matches existed than were returned.
	pub truncated: bool,
}

impl SessionStore {
	/// Pins or unpins a message locally. Pins are not synced.
	pub fn set_pinned(&mut self, id: MessageId, pinned: bool) -> Result<(), StoreError> {
		let i = *self.index.get(&id).ok_or(StoreError::NotFound)?;
		self.messages[i].pinned = pinned;
		Ok(())
	}

	/// Pinned messages in transcript order.
	pub fn pinned(&self) -> Vec<MessageId> {
		self.messages
			.iter()
			.filter(|m| m.pinned)
			.map(|m| m.id)
			.collect()
	}

	/// Case-insensitive substring search over text, file names and previews.
	pub fn search(&self, query: &str) -> Result<SearchResult, StoreError> {
		if query.len() > MAX_SEARCH_QUERY_BYTES {
			return Err(StoreError::QueryTooLong);
		}
		let needle = query.to_lowercase();
		let mut hits = Vec::new();
		if needle.is_empty() {
			return Ok(SearchResult {
				hits,
				truncated: false,
			});
		}
		for m in &self.messages {
			let found = match &m.body {
				MessageBody::Text(t) => t.to_lowercase().contains(&needle),
				MessageBody::File(o) => {
					o.name.as_str().to_lowercase().contains(&needle)
						|| o.preview
							.as_ref()
							.is_some_and(|p| p.to_lowercase().contains(&needle))
				}
				MessageBody::Image(_) => false,
			};
			if found {
				if hits.len() == MAX_SEARCH_RESULTS {
					return Ok(SearchResult {
						hits,
						truncated: true,
					});
				}
				hits.push(m.id);
			}
		}
		Ok(SearchResult {
			hits,
			truncated: false,
		})
	}

	/// The message text for a copy action (exact stored bytes).
	pub fn copy_text(&self, id: MessageId) -> Option<&str> {
		match &self.get(id)?.body {
			MessageBody::Text(t) => Some(t),
			_ => None,
		}
	}

	/// The exact content of the `index`th code block of a message.
	pub fn copy_code_block(&self, id: MessageId, index: usize) -> Option<String> {
		let doc = markdown::parse(self.copy_text(id)?);
		doc.code_blocks()
			.nth(index)
			.map(|c| c.copy_text().to_owned())
	}
}
