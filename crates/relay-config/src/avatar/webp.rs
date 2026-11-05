//! The canonical avatar: a lossless WebP (encoding at upload time, validation at receive).
//!
//! The canonical form is a 256x256 RIFF/WEBP file holding exactly one `VP8L` chunk (the
//! lossless bitstream): no lossy `VP8`, no `VP8X` extended header (so no separate alpha,
//! animation, EXIF, XMP or ICC chunks), no unknown chunks, no trailing bytes. Receivers check that form
//! from the header without re-encoding, so the bytes they store hash to what the sender
//! announced. Encoding is the pure-Rust `image-webp` lossless encoder (pinned through
//! `Cargo.lock`): it has no options, timestamps or metadata, so equal pixels give equal bytes.

use std::io::Cursor;

use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

use super::limits::{ENCODED_MAX_BYTES, OUTPUT_DIMENSION};
use crate::error::AvatarError;

/// RIFF header (12) plus the `VP8L` chunk header (8) plus the 5-byte VP8L header.
const MIN_LEN: usize = 12 + 8 + 5;
const VP8L_SIGNATURE: u8 = 0x2f;

/// Encodes pixels as a lossless WebP. Runs at upload time only.
pub(super) fn encode_canonical(pixels: &RgbaImage) -> Result<Vec<u8>, AvatarError> {
	let mut out = Vec::new();
	WebPEncoder::new_lossless(Cursor::new(&mut out))
		.write_image(
			pixels.as_raw(),
			pixels.width(),
			pixels.height(),
			ExtendedColorType::Rgba8,
		)
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	// The encoder is trusted, but never ship something receivers would refuse.
	check_canonical(&out)?;
	Ok(out)
}

/// Checks the canonical form of untrusted bytes from the header alone (no pixel decode).
pub(super) fn check_canonical(bytes: &[u8]) -> Result<(), AvatarError> {
	let bad = AvatarError::NotCanonical;
	if bytes.is_empty() {
		return Err(AvatarError::Empty);
	}
	if bytes.len() > ENCODED_MAX_BYTES {
		return Err(AvatarError::TooLarge {
			size: bytes.len(),
			limit: ENCODED_MAX_BYTES,
		});
	}
	if bytes.len() < MIN_LEN || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
		return Err(AvatarError::UnsupportedFormat);
	}
	let riff_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
	if riff_len != bytes.len() - 8 {
		return Err(bad("RIFF length does not match the file"));
	}
	match &bytes[12..16] {
		b"VP8L" => {}
		b"VP8 " => return Err(bad("lossy WebP")),
		b"VP8X" => return Err(bad("extended WebP (metadata, alpha or animation)")),
		_ => return Err(bad("first chunk is not VP8L")),
	}
	let chunk_len = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
	// Chunks are padded to an even size; the padding byte must be zero. Exactly one chunk.
	let padded = chunk_len + (chunk_len & 1);
	if bytes.len() != 20 + padded || (chunk_len & 1 == 1 && bytes[bytes.len() - 1] != 0) {
		return Err(bad("extra chunks, trailing bytes or bad chunk length"));
	}
	if chunk_len < 5 || bytes[20] != VP8L_SIGNATURE {
		return Err(bad("not a VP8L bitstream"));
	}
	// 14 bits of width-1, 14 bits of height-1, alpha flag, 3 version bits.
	let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
	let width = (bits & 0x3fff) + 1;
	let height = ((bits >> 14) & 0x3fff) + 1;
	if width != OUTPUT_DIMENSION || height != OUTPUT_DIMENSION {
		return Err(bad("dimensions are not 256x256"));
	}
	if bits >> 29 != 0 {
		return Err(bad("unsupported VP8L version"));
	}
	Ok(())
}
