//! The canonical avatar PNG: encoding (upload time only) and validation (receive time).
//!
//! The canonical form is a 256x256, 8-bit RGBA, non-interlaced PNG made of exactly
//! `IHDR`, one or more `IDAT` and `IEND` chunks: no ancillary chunks, no trailing
//! bytes. Receivers check that form without re-encoding, so the bytes they store hash
//! to what the sender announced.

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use oxipng::{Deflater, Options, StripChunks};

use super::limits::{DECODE_ALLOC_MAX, ENCODED_MAX_BYTES, OUTPUT_DIMENSION};
use crate::error::AvatarError;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Bytes of one chunk besides its data: length, type and CRC.
const CHUNK_OVERHEAD: usize = 12;
const IHDR_LEN: usize = 13;

/// Lossless recompression settings. Deterministic: libdeflate with a fixed level and
/// filter set, no parallelism, no time limit (a timeout would make output depend on
/// machine speed). Colour type, bit depth and palette reduction are off so the output
/// stays RGBA8, and everything but the critical chunks is stripped.
fn options() -> Options {
	let mut options = Options::from_preset(2);
	options.deflater = Deflater::Libdeflater { compression: 12 };
	options.strip = StripChunks::All;
	options.bit_depth_reduction = false;
	options.color_type_reduction = false;
	options.palette_reduction = false;
	options.grayscale_reduction = false;
	options.optimize_alpha = false;
	options.interlace = Some(false);
	options.timeout = None;
	options
}

/// Encodes pixels as the smallest canonical PNG. Runs at upload time only.
pub(super) fn encode_canonical(pixels: &RgbaImage) -> Result<Vec<u8>, AvatarError> {
	let mut plain = Vec::new();
	pixels
		.write_to(&mut Cursor::new(&mut plain), ImageFormat::Png)
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	let packed = oxipng::optimize_from_memory(&plain, &options())
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	// Recompression is best effort: never ship something that is no longer canonical.
	if check_canonical(&packed).is_ok() {
		Ok(packed)
	} else {
		Ok(plain)
	}
}

/// Checks the canonical form of untrusted bytes without decoding pixels.
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
	let Some(mut rest) = bytes.strip_prefix(&SIGNATURE) else {
		return Err(AvatarError::UnsupportedFormat);
	};
	let mut index = 0usize;
	let mut idat = 0usize;
	loop {
		if rest.len() < CHUNK_OVERHEAD {
			return Err(bad("truncated"));
		}
		let len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
		let name: [u8; 4] = [rest[4], rest[5], rest[6], rest[7]];
		let total = len.checked_add(CHUNK_OVERHEAD).ok_or(bad("chunk length"))?;
		if rest.len() < total {
			return Err(bad("truncated"));
		}
		let data = &rest[8..8 + len];
		let crc =
			u32::from_be_bytes([rest[8 + len], rest[9 + len], rest[10 + len], rest[11 + len]]);
		if crc != crc32(&rest[4..8 + len]) {
			return Err(bad("chunk checksum"));
		}
		match (index, &name) {
			(0, b"IHDR") => check_header(data)?,
			(0, _) => return Err(bad("first chunk is not IHDR")),
			(_, b"IDAT") => idat += 1,
			(_, b"IEND") if idat > 0 && len == 0 => {
				return if rest.len() == total {
					Ok(())
				} else {
					Err(bad("data after IEND"))
				};
			}
			_ => return Err(bad("unexpected chunk")),
		}
		index += 1;
		rest = &rest[total..];
	}
}

fn check_header(data: &[u8]) -> Result<(), AvatarError> {
	if data.len() != IHDR_LEN {
		return Err(AvatarError::NotCanonical("IHDR length"));
	}
	let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
	let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
	if width == 0 || height == 0 {
		return Err(AvatarError::ZeroDimension);
	}
	if width != OUTPUT_DIMENSION || height != OUTPUT_DIMENSION {
		return Err(AvatarError::NotCanonical("dimensions are not 256x256"));
	}
	// bit depth 8, colour type 6 (RGBA), deflate, adaptive filtering, no interlace.
	if data[8..13] != [8, 6, 0, 0, 0] {
		return Err(AvatarError::NotCanonical("not 8-bit non-interlaced RGBA"));
	}
	let needed = u64::from(width) * u64::from(height) * 4;
	if needed > DECODE_ALLOC_MAX {
		return Err(AvatarError::DecodeBudget {
			needed,
			limit: DECODE_ALLOC_MAX,
		});
	}
	Ok(())
}

/// CRC-32 (IEEE) as used by PNG chunks.
fn crc32(data: &[u8]) -> u32 {
	let mut crc = !0u32;
	for &byte in data {
		crc ^= u32::from(byte);
		for _ in 0..8 {
			crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
		}
	}
	!crc
}
