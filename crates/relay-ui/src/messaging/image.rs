//! Inline image decoding with hard limits, run off the UI thread.

use crate::tr;
use std::io::Cursor;

use image::{ImageReader, Limits};
use relay_messages::image::inspect;

use super::rules::{INLINE_IMAGE_MAX_BYTES, MAX_DECODED_PIXELS};
use crate::imaging::Decoded;

/// Decoder memory ceiling: 16 megapixels of RGBA plus slack.
const MAX_ALLOC: u64 = 80 * 1024 * 1024;
/// Largest side of the texture kept; bigger pictures are scaled down.
pub const THUMB_SIDE: u32 = 384;

/// Why an image is not shown. The text is safe to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProblem {
	TooLarge,
	Invalid,
	TooManyPixels,
	Failed,
}

impl std::fmt::Display for ImageProblem {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::TooLarge => f.write_str(&tr!("image-too-large", limit = INLINE_IMAGE_MAX_BYTES)),
			Self::Invalid => f.write_str(tr!("image-invalid")),
			Self::TooManyPixels => f.write_str(tr!("image-too-many-pixels")),
			Self::Failed => f.write_str(tr!("image-failed")),
		}
	}
}

impl std::error::Error for ImageProblem {}

/// Header check first (nothing is allocated for a refused picture), then a
/// decode under an allocation limit, scaled to a thumbnail.
pub fn decode_inline(bytes: &[u8]) -> Result<Decoded, ImageProblem> {
	if bytes.len() > INLINE_IMAGE_MAX_BYTES {
		return Err(ImageProblem::TooLarge);
	}
	let info = inspect(bytes).map_err(|_| ImageProblem::Invalid)?;
	if u64::from(info.width) * u64::from(info.height) > MAX_DECODED_PIXELS {
		return Err(ImageProblem::TooManyPixels);
	}
	let mut limits = Limits::default();
	limits.max_image_width = Some(info.width);
	limits.max_image_height = Some(info.height);
	limits.max_alloc = Some(MAX_ALLOC);
	let mut reader = ImageReader::new(Cursor::new(bytes))
		.with_guessed_format()
		.map_err(|_| ImageProblem::Invalid)?;
	reader.limits(limits);
	let mut img = reader.decode().map_err(|_| ImageProblem::Failed)?;
	if img.width().max(img.height()) > THUMB_SIDE {
		img = img.thumbnail(THUMB_SIDE, THUMB_SIDE);
	}
	let rgba = img.into_rgba8();
	Ok(Decoded {
		size: [rgba.width() as usize, rgba.height() as usize],
		rgba: rgba.into_raw(),
	})
}
