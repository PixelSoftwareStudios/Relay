//! Bounded image decoding for avatars and QR codes, off the UI thread.

use std::io::Cursor;

use image::{DynamicImage, ImageReader, Limits};

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
