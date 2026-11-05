use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use relay_config::avatar::limits::{ENCODED_MAX_BYTES, OUTPUT_DIMENSION};
use relay_config::avatar::{
	AvatarCache, parse_hash, process_avatar, read_avatar_file, validate_canonical,
};
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
fn valid_png_becomes_canonical_128_square() {
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
	assert!(validate_canonical(&flat(129, 129)).is_err());
	assert!(validate_canonical(&flat(128, 128)).is_ok());
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

#[test]
fn cache_stores_verifies_and_rejects_untrusted() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path().join("av")).unwrap();
	let a = process_avatar(&flat(50, 50)).unwrap();
	cache.put(&a).unwrap();
	assert_eq!(
		cache.get(&a.hash).unwrap().as_deref(),
		Some(a.png.as_slice())
	);
	assert_eq!(cache.get(&[0; 32]).unwrap(), None);
	assert!(cache.put_untrusted(b"junk").is_err());
	assert_eq!(cache.put_untrusted(&a.png).unwrap(), a.hash);
}

#[test]
fn cache_detects_tampering() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path().join("av")).unwrap();
	let a = process_avatar(&flat(50, 50)).unwrap();
	cache.put(&a).unwrap();
	std::fs::write(
		dir.path().join("av").join(format!("{}.png", a.hash_hex())),
		b"evil",
	)
	.unwrap();
	assert_eq!(cache.get(&a.hash).unwrap(), None);
}

#[test]
fn cache_evicts_oldest_within_bound() {
	let dir = tempfile::tempdir().unwrap();
	let mk = |shade: u8| {
		let img = RgbaImage::from_fn(64, 64, |x, y| {
			Rgba([shade, (x * 4) as u8, (y * 4) as u8, 255])
		});
		process_avatar(&encode(DynamicImage::ImageRgba8(img), ImageFormat::Png)).unwrap()
	};
	let (a, b, c) = (mk(1), mk(2), mk(3));
	let bound = (a.png.len() + b.png.len() + c.png.len()) as u64 - 1;
	let cache = AvatarCache::with_limit(dir.path().join("av"), bound).unwrap();
	cache.put(&a).unwrap();
	std::thread::sleep(std::time::Duration::from_millis(20));
	cache.put(&b).unwrap();
	std::thread::sleep(std::time::Duration::from_millis(20));
	cache.put(&c).unwrap();
	assert!(cache.size().unwrap() <= bound);
	assert_eq!(cache.get(&a.hash).unwrap(), None);
	assert!(cache.get(&c.hash).unwrap().is_some());
}
