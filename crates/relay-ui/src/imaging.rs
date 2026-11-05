//! Bounded image decoding for avatars and QR codes, off the UI thread.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// Largest source dimension accepted for decoding.
const MAX_SOURCE_DIM: u32 = 4096;
/// Decoder memory ceiling; protects against decompression bombs.
const MAX_ALLOC: u64 = 96 * 1024 * 1024;
/// Previews and cached avatars are shown small; larger images are scaled down.
pub const MAX_TEXTURE_DIM: u32 = 256;

/// RGBA pixels ready to become a texture.
#[derive(Clone)]
pub struct Decoded {
	pub size: [usize; 2],
	pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Decoded {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Decoded({}x{})", self.size[0], self.size[1])
	}
}

fn decode_image(bytes: &[u8]) -> Option<DynamicImage> {
	let mut limits = Limits::default();
	limits.max_image_width = Some(MAX_SOURCE_DIM);
	limits.max_image_height = Some(MAX_SOURCE_DIM);
	limits.max_alloc = Some(MAX_ALLOC);
	let mut reader = ImageReader::new(Cursor::new(bytes))
		.with_guessed_format()
		.ok()?;
	reader.limits(limits);
	reader.decode().ok()
}

/// Decode PNG or JPEG bytes, scaling anything above [`MAX_TEXTURE_DIM`] down.
/// `None` for malformed, oversized or unsupported input.
pub fn decode_for_texture(bytes: &[u8]) -> Option<Decoded> {
	let mut img = decode_image(bytes)?;
	if img.width().max(img.height()) > MAX_TEXTURE_DIM {
		img = img.thumbnail(MAX_TEXTURE_DIM, MAX_TEXTURE_DIM);
	}
	let rgba = img.into_rgba8();
	Some(Decoded {
		size: [rgba.width() as usize, rgba.height() as usize],
		rgba: rgba.into_raw(),
	})
}

/// Largest width or height of an uploaded avatar, as the engine enforces it.
pub const AVATAR_MAX_DIM: u32 = 2048;

const FORMATS: &str = "use PNG, JPEG, WebP, GIF, BMP or TIFF";

/// Client-side mirror of the engine's avatar checks, with its wording (`docs/api.md`,
/// "Avatar refusal texts"): the format is sniffed from the bytes (the file name and media
/// type are ignored), then the header is read for the dimensions without decoding pixels.
/// Returns the media type of the sniffed format. The engine stays the authority.
pub fn avatar_check(bytes: &[u8]) -> Result<&'static str, String> {
	if let Some(problem) = crate::format::avatar_problem(bytes.len()) {
		return Err(problem.to_owned());
	}
	let reader = ImageReader::new(Cursor::new(bytes))
		.with_guessed_format()
		.map_err(|e| format!("avatar rejected: avatar could not be decoded: {e}"))?;
	let media = match reader.format() {
		Some(ImageFormat::Png) => "image/png",
		Some(ImageFormat::Jpeg) => "image/jpeg",
		Some(ImageFormat::WebP) => "image/webp",
		Some(ImageFormat::Gif) => "image/gif",
		Some(ImageFormat::Bmp) => "image/bmp",
		Some(ImageFormat::Tiff) => "image/tiff",
		_ => {
			return Err(format!(
				"avatar rejected: unsupported or unrecognised image format ({FORMATS})"
			));
		}
	};
	let (width, height) = reader
		.into_dimensions()
		.map_err(|e| format!("avatar rejected: avatar could not be decoded: {e}"))?;
	if width == 0 || height == 0 {
		return Err("avatar rejected: avatar has zero width or height".to_owned());
	}
	if width > AVATAR_MAX_DIM || height > AVATAR_MAX_DIM {
		return Err(format!(
			"avatar rejected: avatar dimensions {width}x{height} exceed the {AVATAR_MAX_DIM} pixel limit"
		));
	}
	Ok(media)
}
