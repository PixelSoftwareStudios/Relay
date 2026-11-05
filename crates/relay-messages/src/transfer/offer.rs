//! Offer metadata validation and preview rules.

use crate::error::StoreError;
use crate::limits::*;
use crate::message::{FileOffer, OfferMeta, OfferStatus};
use crate::sanitize::{guess_media_type, sanitize_file_name, sanitize_with_limit};

/// Validates untrusted metadata into a retained offer (name re-sanitized, type re-derived).
pub fn offer_from_meta(m: &OfferMeta) -> Result<FileOffer, StoreError> {
	if m.size > MAX_FILE_BYTES {
		return Err(StoreError::Invalid("file too large"));
	}
	if m.name.len() > 4096 {
		return Err(StoreError::Invalid("name too long"));
	}
	let name = sanitize_file_name(&m.name);
	Ok(FileOffer {
		id: m.id,
		media_type: guess_media_type(name.as_str()),
		name,
		size: m.size,
		hash: m.hash,
		preview: None,
		status: OfferStatus::Offered,
	})
}

/// Wire metadata of a retained offer.
pub fn offer_to_meta(o: &FileOffer) -> OfferMeta {
	OfferMeta {
		id: o.id,
		name: o.name.as_str().to_owned(),
		size: o.size,
		hash: o.hash,
	}
}

/// Validates a sender-approved preview: UTF-8 text, at most 16 KiB and 100 lines.
pub fn validate_preview(text: &str) -> Result<String, StoreError> {
	let t = sanitize_with_limit(text, MAX_PREVIEW_BYTES)?;
	if t.lines().count() > MAX_PREVIEW_LINES {
		return Err(StoreError::Invalid("preview has too many lines"));
	}
	Ok(t)
}

/// Free space the receiver needs before accepting `size` bytes.
pub fn required_free_space(size: u64) -> u64 {
	size.saturating_add((size / 100 * FREE_MARGIN_PERCENT).max(MIN_FREE_MARGIN_BYTES))
}
