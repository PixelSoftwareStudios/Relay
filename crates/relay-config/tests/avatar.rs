use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use relay_config::avatar::limits::{ENCODED_MAX_BYTES, OUTPUT_DIMENSION};
use relay_config::avatar::{parse_hash, process_avatar, read_avatar_file, validate_canonical};
use relay_config::{AvatarError, ConfigError};

fn encode(img: DynamicImage, fmt: ImageFormat) -> Vec<u8> {
	let mut out = Vec::new();
	img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
	out
}

fn flat(w: u32, h: u32) -> Vec<u8> {
	encode(
		DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba([10, 200, 30, 255]))),
		ImageFormat::Png,
	)
}

fn avatar_err(r: Result<impl std::fmt::Debug, ConfigError>) -> AvatarError {
	match r {
		Err(ConfigError::Avatar(e)) => e,
		other => panic!("expected avatar error, got {other:?}"),
	}
}

#[test]
fn valid_png_becomes_canonical_256_square() {
	let a = process_avatar(&flat(300, 200)).unwrap();
	let img = image::load_from_memory(&a.png).unwrap();
	assert_eq!(
		(img.width(), img.height()),
		(OUTPUT_DIMENSION, OUTPUT_DIMENSION)
	);
	assert_eq!(a.hash, *blake3::hash(&a.png).as_bytes());
	assert_eq!(a.hash, process_avatar(&flat(300, 200)).unwrap().hash);
	assert_eq!(validate_canonical(&a.png).unwrap(), a.hash);
}

#[test]
fn jpeg_is_accepted() {
	let rgb = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 64, image::Rgb([1, 2, 3])));
	process_avatar(&encode(rgb, ImageFormat::Jpeg)).unwrap();
}

#[test]
fn empty_garbage_and_wrong_format_are_rejected() {
	assert_eq!(avatar_err(process_avatar(&[])), AvatarError::Empty);
	assert!(matches!(
		avatar_err(process_avatar(b"definitely not an image")),
		AvatarError::UnsupportedFormat
	));
	let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;";
	assert_eq!(
		avatar_err(process_avatar(gif)),
		AvatarError::UnsupportedFormat
	);
	let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
	assert_eq!(
		avatar_err(process_avatar(svg)),
		AvatarError::UnsupportedFormat
	);
}

#[test]
fn truncated_png_is_rejected() {
	let png = flat(100, 100);
	assert!(matches!(
		avatar_err(process_avatar(&png[..png.len() / 2])),
		AvatarError::Decode(_)
	));
}

#[test]
fn oversize_encoded_is_rejected_before_decoding() {
	let big = vec![0x89; ENCODED_MAX_BYTES + 1];
	assert!(matches!(
		avatar_err(process_avatar(&big)),
		AvatarError::TooLarge { .. }
	));
}

#[test]
fn decompression_bomb_dimensions_are_rejected() {
	// Flat 4000x4000 grayscale compresses small but would decode to 16 MB.
	let gray = image::GrayImage::from_pixel(4000, 4000, image::Luma([7]));
	let bomb = encode(DynamicImage::ImageLuma8(gray), ImageFormat::Png);
	assert!(bomb.len() < ENCODED_MAX_BYTES);
	assert!(matches!(
		avatar_err(process_avatar(&bomb)),
		AvatarError::DimensionsTooLarge { .. } | AvatarError::Decode(_)
	));
}

#[test]
fn lying_header_dimensions_are_rejected() {
	// Patch the IHDR width of a valid PNG to 60000; CRC mismatch or limit must reject it.
	let mut png = flat(16, 16);
	png[16..20].copy_from_slice(&60000u32.to_be_bytes());
	assert!(process_avatar(&png).is_err());
}

#[test]
fn remote_canonical_rejects_large_images() {
	assert!(validate_canonical(&flat(257, 257)).is_err());
	assert!(validate_canonical(&flat(255, 255)).is_err());
	assert!(validate_canonical(&flat(256, 256)).is_ok());
}

#[test]
fn hash_parsing_is_strict() {
	assert!(parse_hash(&"ab".repeat(32)).is_ok());
	assert_eq!(parse_hash(&"AB".repeat(32)), Err(AvatarError::BadHash));
	assert_eq!(parse_hash("../../etc/passwd"), Err(AvatarError::BadHash));
	assert_eq!(parse_hash(&"a".repeat(63)), Err(AvatarError::BadHash));
}

#[test]
fn path_validation() {
	let dir = tempfile::tempdir().unwrap();
	let good = dir.path().join("me.png");
	std::fs::write(&good, flat(10, 10)).unwrap();
	assert!(read_avatar_file(&good).is_ok());
	let txt = dir.path().join("me.txt");
	std::fs::write(&txt, b"x").unwrap();
	assert!(matches!(
		avatar_err(read_avatar_file(&txt)),
		AvatarError::BadPath(_)
	));
	assert!(matches!(
		avatar_err(read_avatar_file(dir.path())),
		AvatarError::BadPath(_)
	));
	assert!(matches!(
		avatar_err(read_avatar_file("".as_ref())),
		AvatarError::BadPath(_)
	));
	let huge = dir.path().join("huge.png");
	std::fs::write(&huge, vec![0u8; ENCODED_MAX_BYTES + 10]).unwrap();
	assert!(matches!(
		avatar_err(read_avatar_file(&huge)),
		AvatarError::TooLarge { .. }
	));
	#[cfg(unix)]
	{
		let link = dir.path().join("link.png");
		std::os::unix::fs::symlink(&good, &link).unwrap();
		assert!(matches!(
			avatar_err(read_avatar_file(&link)),
			AvatarError::BadPath(_)
		));
	}
}

/// A noisy gradient: close to the worst case for PNG, so sizes stay well under the cap.
fn busy(w: u32, h: u32) -> Vec<u8> {
	let img = RgbaImage::from_fn(w, h, |x, y| {
		let n = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503);
		Rgba([(n >> 3) as u8, (n >> 11) as u8, (x + y) as u8, 255])
	});
	encode(DynamicImage::ImageRgba8(img), ImageFormat::Png)
}

#[test]
fn compression_is_deterministic_and_shrinks() {
	let src = busy(512, 400);
	let a = process_avatar(&src).unwrap();
	let b = process_avatar(&src).unwrap();
	assert_eq!(a.png, b.png);
	assert_eq!(a.hash, b.hash);
	let plain = {
		let img = image::load_from_memory(&src).unwrap().to_rgba8();
		let small = image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Lanczos3);
		encode(DynamicImage::ImageRgba8(small), ImageFormat::Png)
	};
	assert!(a.png.len() <= plain.len());
	assert!(a.png.len() <= ENCODED_MAX_BYTES);
	let flat = process_avatar(&flat(300, 300)).unwrap();
	assert!(
		flat.png.len() < 2048,
		"flat avatar is {} bytes",
		flat.png.len()
	);
}

/// Splits a PNG into (type, whole chunk bytes).
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
	let mut out = Vec::new();
	let mut at = 8;
	while at < png.len() {
		let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
		out.push((
			png[at + 4..at + 8].try_into().unwrap(),
			png[at..at + 12 + len].to_vec(),
		));
		at += 12 + len;
	}
	out
}

#[test]
fn canonical_output_has_only_critical_chunks_and_no_metadata() {
	let a = process_avatar(&busy(300, 300)).unwrap();
	let names: Vec<_> = chunks(&a.png).iter().map(|c| c.0).collect();
	assert_eq!(names.first(), Some(b"IHDR"));
	assert_eq!(names.last(), Some(b"IEND"));
	assert!(names[1..names.len() - 1].iter().all(|n| n == b"IDAT"));
	// RGBA8 non-interlaced.
	assert_eq!(a.png[24..29], [8, 6, 0, 0, 0]);
}

#[test]
fn receivers_reject_non_canonical_forms_without_reencoding() {
	let good = process_avatar(&busy(300, 300)).unwrap().png;
	let parts = chunks(&good);
	let rebuild = |extra: Option<Vec<u8>>, tail: &[u8]| {
		let mut out = good[..8].to_vec();
		for (name, bytes) in &parts {
			if name == b"IEND"
				&& let Some(e) = &extra
			{
				out.extend_from_slice(e);
			}
			out.extend_from_slice(bytes);
		}
		out.extend_from_slice(tail);
		out
	};
	assert!(validate_canonical(&rebuild(None, &[])).is_ok());
	// Trailing bytes.
	assert!(validate_canonical(&rebuild(None, b"x")).is_err());
	// An ancillary chunk (tEXt with a valid CRC).
	let mut text = Vec::new();
	let body = b"tEXtk\0v";
	text.extend_from_slice(&3u32.to_be_bytes());
	text.extend_from_slice(body);
	text.extend_from_slice(&crc32(body).to_be_bytes());
	assert!(validate_canonical(&rebuild(Some(text), &[])).is_err());
	// Wrong colour type (RGB, 256x256), JPEG, flipped checksum bit.
	let rgb = encode(
		DynamicImage::ImageRgb8(image::RgbImage::from_pixel(256, 256, image::Rgb([1, 2, 3]))),
		ImageFormat::Png,
	);
	assert!(validate_canonical(&rgb).is_err());
	let jpeg = encode(
		DynamicImage::ImageRgb8(image::RgbImage::from_pixel(256, 256, image::Rgb([1, 2, 3]))),
		ImageFormat::Jpeg,
	);
	assert!(validate_canonical(&jpeg).is_err());
	let mut broken = good.clone();
	let last = broken.len() - 20;
	broken[last] ^= 1;
	assert!(validate_canonical(&broken).is_err());
	// Size cap applies to what is received.
	assert!(matches!(
		avatar_err(validate_canonical(&vec![0u8; ENCODED_MAX_BYTES + 1])),
		AvatarError::TooLarge { .. }
	));
}

/// CRC-32 for the hand-built chunk above (bitwise, test only).
fn crc32(data: &[u8]) -> u32 {
	let mut crc = !0u32;
	for &b in data {
		crc ^= u32::from(b);
		for _ in 0..8 {
			crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
		}
	}
	!crc
}
