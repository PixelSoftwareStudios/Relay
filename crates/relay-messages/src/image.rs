//! Header-only image validation. Nothing here decodes pixel data.

use crate::limits::{
	MAX_ANIMATION_DECODED_BYTES, MAX_ANIMATION_FRAMES, MAX_IMAGE_BYTES, MAX_IMAGE_PIXELS,
	MAX_IMAGE_SIDE,
};

/// Accepted container formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImageFormat {
	/// PNG (including APNG).
	Png,
	/// JPEG.
	Jpeg,
	/// WebP (including animated).
	Webp,
	/// GIF.
	Gif,
}

impl ImageFormat {
	/// IANA media type.
	pub fn media_type(self) -> &'static str {
		match self {
			Self::Png => "image/png",
			Self::Jpeg => "image/jpeg",
			Self::Webp => "image/webp",
			Self::Gif => "image/gif",
		}
	}
}

/// Facts read from the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
	/// Container format.
	pub format: ImageFormat,
	/// Pixel width.
	pub width: u32,
	/// Pixel height.
	pub height: u32,
	/// Frame count (1 for still images).
	pub frames: u32,
}

impl ImageInfo {
	/// RGBA bytes of one decoded frame.
	pub fn frame_bytes(&self) -> u64 {
		u64::from(self.width) * u64::from(self.height) * 4
	}

	/// True when more than one frame.
	pub fn animated(&self) -> bool {
		self.frames > 1
	}
}

/// Why an image was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
	/// Compressed size over the limit.
	#[error("image too large")]
	TooLarge,
	/// Not PNG, JPEG, WebP or GIF.
	#[error("unsupported image format")]
	Unsupported,
	/// Header unreadable or inconsistent.
	#[error("malformed image header")]
	Malformed,
	/// Zero or oversized dimensions.
	#[error("image dimensions {width}x{height} exceed the decoded-size ceiling")]
	TooManyPixels {
		/// Width.
		width: u32,
		/// Height.
		height: u32,
	},
	/// Too many frames or too much total decoded memory.
	#[error("animation exceeds frame or memory limits")]
	AnimationTooLarge,
}

type R<T> = Result<T, ImageError>;

fn arr<const N: usize>(b: &[u8], at: usize) -> R<[u8; N]> {
	b.get(at..at.checked_add(N).ok_or(ImageError::Malformed)?)
		.and_then(|s| s.try_into().ok())
		.ok_or(ImageError::Malformed)
}
fn be32(b: &[u8], at: usize) -> R<u32> {
	arr::<4>(b, at).map(u32::from_be_bytes)
}
fn be16(b: &[u8], at: usize) -> R<u32> {
	arr::<2>(b, at).map(|a| u32::from(u16::from_be_bytes(a)))
}
fn le16(b: &[u8], at: usize) -> R<u32> {
	arr::<2>(b, at).map(|a| u32::from(u16::from_le_bytes(a)))
}
fn le32(b: &[u8], at: usize) -> R<u32> {
	arr::<4>(b, at).map(u32::from_le_bytes)
}
fn le24(b: &[u8], at: usize) -> R<u32> {
	arr::<3>(b, at).map(|s| u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
}

/// Validates compressed bytes from the header only.
pub fn inspect(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
	if bytes.len() > MAX_IMAGE_BYTES {
		return Err(ImageError::TooLarge);
	}
	let info = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
		png(bytes)?
	} else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
		jpeg(bytes)?
	} else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
		gif(bytes)?
	} else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
		webp(bytes)?
	} else {
		return Err(ImageError::Unsupported);
	};
	check_limits(&info)?;
	Ok(info)
}

fn check_limits(i: &ImageInfo) -> R<()> {
	let (w, h) = (i.width, i.height);
	if w == 0
		|| h == 0
		|| w > MAX_IMAGE_SIDE
		|| h > MAX_IMAGE_SIDE
		|| u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS
	{
		return Err(ImageError::TooManyPixels {
			width: w,
			height: h,
		});
	}
	if i.frames == 0 {
		return Err(ImageError::Malformed);
	}
	if i.frames > MAX_ANIMATION_FRAMES
		|| u64::from(i.frames) * i.frame_bytes() > MAX_ANIMATION_DECODED_BYTES.max(i.frame_bytes())
	{
		return Err(ImageError::AnimationTooLarge);
	}
	Ok(())
}

fn png(b: &[u8]) -> R<ImageInfo> {
	if be32(b, 8)? != 13 || b.get(12..16) != Some(b"IHDR") {
		return Err(ImageError::Malformed);
	}
	let (width, height) = (be32(b, 16)?, be32(b, 20)?);
	if !matches!(b.get(25), Some(0 | 2 | 3 | 4 | 6)) {
		return Err(ImageError::Malformed);
	}
	let (mut at, mut frames) = (8 + 12 + 13, 1);
	for _ in 0..64 {
		let (Ok(len), Some(kind)) = (be32(b, at), b.get(at + 4..at + 8)) else {
			break;
		};
		if kind == b"IDAT" {
			break;
		}
		if kind == b"acTL" {
			frames = be32(b, at + 8)?;
			break;
		}
		at = at.saturating_add(12).saturating_add(len as usize);
	}
	Ok(ImageInfo {
		format: ImageFormat::Png,
		width,
		height,
		frames,
	})
}

fn jpeg(b: &[u8]) -> R<ImageInfo> {
	let mut at = 2;
	for _ in 0..512 {
		while b.get(at) == Some(&0xFF) && b.get(at + 1) == Some(&0xFF) {
			at += 1;
		}
		if b.get(at) != Some(&0xFF) {
			return Err(ImageError::Malformed);
		}
		let m = *b.get(at + 1).ok_or(ImageError::Malformed)?;
		if matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
			return Ok(ImageInfo {
				format: ImageFormat::Jpeg,
				height: be16(b, at + 5)?,
				width: be16(b, at + 7)?,
				frames: 1,
			});
		}
		if m == 0xDA || m == 0xD9 {
			return Err(ImageError::Malformed);
		}
		if matches!(m, 0x01 | 0xD0..=0xD8) {
			at += 2;
			continue;
		}
		let len = be16(b, at + 2)? as usize;
		if len < 2 {
			return Err(ImageError::Malformed);
		}
		at += 2 + len;
	}
	Err(ImageError::Malformed)
}

fn skip_sub_blocks(b: &[u8], mut at: usize) -> R<usize> {
	loop {
		let n = usize::from(*b.get(at).ok_or(ImageError::Malformed)?);
		at += 1 + n;
		if n == 0 {
			return Ok(at);
		}
	}
}

fn gif(b: &[u8]) -> R<ImageInfo> {
	let (width, height) = (le16(b, 6)?, le16(b, 8)?);
	let flags = *b.get(10).ok_or(ImageError::Malformed)?;
	let mut at = 13
		+ if flags & 0x80 != 0 {
			3 << ((flags & 7) + 1)
		} else {
			0
		};
	let mut frames = 0u32;
	while let Some(&tag) = b.get(at) {
		match tag {
			0x21 => at = skip_sub_blocks(b, at + 2)?,
			0x2C => {
				frames += 1;
				if frames > MAX_ANIMATION_FRAMES {
					return Err(ImageError::AnimationTooLarge);
				}
				let f = *b.get(at + 9).ok_or(ImageError::Malformed)?;
				let ct = if f & 0x80 != 0 { 3 << ((f & 7) + 1) } else { 0 };
				at = skip_sub_blocks(b, at + 10 + ct + 1)?;
			}
			0x3B => break,
			_ => return Err(ImageError::Malformed),
		}
	}
	Ok(ImageInfo {
		format: ImageFormat::Gif,
		width,
		height,
		frames: frames.max(1),
	})
}

fn webp(b: &[u8]) -> R<ImageInfo> {
	let kind = b.get(12..16).ok_or(ImageError::Malformed)?;
	let (width, height, mut frames) = match kind {
		b"VP8 " => {
			if b.get(23..26) != Some(&[0x9D, 0x01, 0x2A]) {
				return Err(ImageError::Malformed);
			}
			(le16(b, 26)? & 0x3FFF, le16(b, 28)? & 0x3FFF, 1)
		}
		b"VP8L" => {
			if b.get(20) != Some(&0x2F) {
				return Err(ImageError::Malformed);
			}
			let bits = le32(b, 21)?;
			((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1, 1)
		}
		b"VP8X" => {
			let animated = b.get(20).ok_or(ImageError::Malformed)? & 0x02 != 0;
			(
				le24(b, 24)? + 1,
				le24(b, 27)? + 1,
				if animated { 0 } else { 1 },
			)
		}
		_ => return Err(ImageError::Malformed),
	};
	if frames == 0 {
		let mut at = 12;
		for _ in 0..1024 {
			let (Some(id), Ok(len)) = (b.get(at..at + 4), le32(b, at + 4)) else {
				break;
			};
			if id == b"ANMF" {
				frames += 1;
				if frames > MAX_ANIMATION_FRAMES {
					return Err(ImageError::AnimationTooLarge);
				}
			}
			at = at
				.saturating_add(8)
				.saturating_add(len as usize)
				.saturating_add(len as usize & 1);
		}
		frames = frames.max(1);
	}
	Ok(ImageInfo {
		format: ImageFormat::Webp,
		width,
		height,
		frames,
	})
}
