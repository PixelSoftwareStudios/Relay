//! Wire format of an authenticated record: a 32-byte public header followed by the AEAD
//! ciphertext of `header_copy || kind || body`.

mod header;
mod kind;

pub use header::{HeaderError, Lane, RecordHeader};
pub use kind::RecordKind;

use crate::limits::{AEAD_TAG_BYTES, RECORD_HEADER_BYTES};

/// Plaintext bytes preceding the typed body: inner header copy plus the kind byte.
pub const INNER_PREFIX_BYTES: usize = RECORD_HEADER_BYTES + 1;

/// Total sealed record length for a body of `body_len` bytes.
pub const fn sealed_len(body_len: usize) -> usize {
	RECORD_HEADER_BYTES + INNER_PREFIX_BYTES + body_len + AEAD_TAG_BYTES
}

/// Largest body that fits a sealed record of at most `record_max` bytes.
pub const fn max_body_len(record_max: usize) -> usize {
	record_max.saturating_sub(sealed_len(0))
}
