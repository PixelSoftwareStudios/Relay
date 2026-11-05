//! Constrained decode of untrusted avatar bytes and re-encode to a canonical lossless WebP.

use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;

use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Limits};

use super::limits::{DECODE_ALLOC_MAX, ENCODED_MAX_BYTES, INPUT_DIMENSION_MAX, OUTPUT_DIMENSION};
use super::webp::{check_canonical, encode_canonical};
use crate::error::{AvatarError, ConfigError, Result};

/// A canonical avatar: a lossless WebP image plus its blake3 content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avatar {
	pub image: Vec<u8>,
	pub hash: [u8; 32],
}

impl Avatar {
	pub fn hash_hex(&self) -> String {
		hex::encode(self.hash)
	}
}

fn reader(
	bytes: &[u8],
	limit_dim: u32,
) -> std::result::Result<ImageReader<Cursor<&[u8]>>, AvatarError> {
	let mut reader = ImageReader::new(Cursor::new(bytes))
		.with_guessed_format()
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	match reader.format() {
		Some(
			ImageFormat::Png
			| ImageFormat::Jpeg
			| ImageFormat::WebP
			| ImageFormat::Gif
			| ImageFormat::Bmp
			| ImageFormat::Tiff,
		) => {}
		_ => return Err(AvatarError::UnsupportedFormat),
	}
	let mut limits = Limits::default();
	limits.max_image_width = Some(limit_dim);
	limits.max_image_height = Some(limit_dim);
	limits.max_alloc = Some(DECODE_ALLOC_MAX);
	reader.limits(limits);
	Ok(reader)
}

fn check_size(bytes: &[u8]) -> std::result::Result<(), AvatarError> {
	if bytes.is_empty() {
		return Err(AvatarError::Empty);
	}
	if bytes.len() > ENCODED_MAX_BYTES {
		return Err(AvatarError::TooLarge {
			size: bytes.len(),
			limit: ENCODED_MAX_BYTES,
		});
	}
	Ok(())
}

/// Reads dimensions from the header only, before any pixel allocation.
fn check_dimensions(bytes: &[u8], limit: u32) -> std::result::Result<(), AvatarError> {
	// Unlimited here so an oversized header is reported with its real size, not as a decoder error.
	let (w, h) = reader(bytes, u32::MAX)?
		.into_dimensions()
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	if w == 0 || h == 0 {
		return Err(AvatarError::ZeroDimension);
	}
	if w > limit || h > limit {
		return Err(AvatarError::DimensionsTooLarge {
			width: w,
			height: h,
			limit,
		});
	}
	let needed = u64::from(w) * u64::from(h) * 4;
	if needed > DECODE_ALLOC_MAX {
		return Err(AvatarError::DecodeBudget {
			needed,
			limit: DECODE_ALLOC_MAX,
		});
	}
	Ok(())
}

/// Validates untrusted upload bytes and returns the canonical avatar.
///
/// The image is center-cropped to a square, resized to 256x256 and re-encoded as a
/// lossless WebP, which discards metadata and any trailing or hidden data. Accepts
/// PNG, JPEG, WebP, GIF, BMP and TIFF; animated inputs contribute their first frame.
/// Encoding is deterministic, so equal input gives equal bytes and hash.
pub fn process_avatar(bytes: &[u8]) -> Result<Avatar> {
	check_size(bytes)?;
	check_dimensions(bytes, INPUT_DIMENSION_MAX)?;
	let decoded = reader(bytes, INPUT_DIMENSION_MAX)?
		.decode()
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	let (w, h) = (decoded.width(), decoded.height());
	let side = w.min(h);
	let square = decoded.crop_imm((w - side) / 2, (h - side) / 2, side, side);
	let small = square.resize_exact(OUTPUT_DIMENSION, OUTPUT_DIMENSION, FilterType::Lanczos3);
	let image = encode_canonical(&small.to_rgba8())?;
	let hash = *blake3::hash(&image).as_bytes();
	Ok(Avatar { image, hash })
}

/// Validates bytes received from a peer as an already-canonical avatar: a 256x256
/// lossless WebP with a single `VP8L` chunk and nothing else, within the size limit.
/// Nothing is re-encoded. Returns the blake3 hash of the received bytes.
pub fn validate_canonical(bytes: &[u8]) -> Result<[u8; 32]> {
	check_canonical(bytes)?;
	reader(bytes, OUTPUT_DIMENSION)?
		.decode()
		.map_err(|e| AvatarError::Decode(e.to_string()))?;
	Ok(*blake3::hash(bytes).as_bytes())
}

/// Parses a strict lowercase 64-character hex hash.
pub fn parse_hash(text: &str) -> std::result::Result<[u8; 32], AvatarError> {
	if text.len() != 64
		|| !text
			.bytes()
			.all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
	{
		return Err(AvatarError::BadHash);
	}
	let mut out = [0u8; 32];
	hex::decode_to_slice(text, &mut out).map_err(|_| AvatarError::BadHash)?;
	Ok(out)
}

/// Reads a user-chosen avatar file after path validation.
///
/// Accepts only regular, non-symlink files with a png/jpg/jpeg/webp/gif/bmp/tif/tiff extension and never
/// reads more than the encoded limit.
pub fn read_avatar_file(path: &Path) -> Result<Vec<u8>> {
	let bad = |m: &'static str| ConfigError::Avatar(AvatarError::BadPath(m));
	if path.as_os_str().is_empty() || path.to_string_lossy().contains('\0') {
		return Err(bad("empty or contains NUL"));
	}
	let ext = path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase);
	if !matches!(
		ext.as_deref(),
		Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff")
	) {
		return Err(bad(
			"extension must be png, jpg, jpeg, webp, gif, bmp or tiff",
		));
	}
	let meta = fs::symlink_metadata(path).map_err(|e| ConfigError::io(path, e))?;
	if meta.file_type().is_symlink() {
		return Err(bad("symbolic links are not accepted"));
	}
	if !meta.is_file() {
		return Err(bad("not a regular file"));
	}
	if meta.len() > ENCODED_MAX_BYTES as u64 {
		return Err(AvatarError::TooLarge {
			size: meta.len() as usize,
			limit: ENCODED_MAX_BYTES,
		}
		.into());
	}
	let mut bytes = Vec::new();
	File::open(path)
		.and_then(|f| f.take(ENCODED_MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
		.map_err(|e| ConfigError::io(path, e))?;
	check_size(&bytes)?;
	Ok(bytes)
}
