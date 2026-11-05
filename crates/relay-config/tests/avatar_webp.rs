use std::io::Cursor;

use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat, Rgba, RgbaImage};
use relay_config::avatar::limits::{ENCODED_MAX_BYTES, OUTPUT_DIMENSION};
use relay_config::avatar::{process_avatar, validate_canonical};
use relay_config::{AvatarError, ConfigError};

fn encode(img: DynamicImage, fmt: ImageFormat) -> Vec<u8> {
	let mut out = Vec::new();
	img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
	out
}

/// A lossless WebP of any size, straight from the encoder.
fn webp(w: u32, h: u32) -> Vec<u8> {
	let img = RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, (x ^ y) as u8, 255]));
	let mut out = Vec::new();
	WebPEncoder::new_lossless(Cursor::new(&mut out))
		.write_image(img.as_raw(), w, h, ExtendedColorType::Rgba8)
		.unwrap();
	out
}

fn good() -> Vec<u8> {
	webp(OUTPUT_DIMENSION, OUTPUT_DIMENSION)
}

fn reason(bytes: &[u8]) -> AvatarError {
	match validate_canonical(bytes) {
		Err(ConfigError::Avatar(e)) => e,
		other => panic!("expected avatar error, got {other:?}"),
	}
}

fn not_canonical(bytes: &[u8]) {
	assert!(
		matches!(reason(bytes), AvatarError::NotCanonical(_)),
		"{:?}",
		reason(bytes)
	);
}

/// RIFF container around `chunks`, with a correct length.
fn riff(chunks: &[&[u8]]) -> Vec<u8> {
	let body: Vec<u8> = [b"WEBP".as_slice()]
		.into_iter()
		.chain(chunks.iter().copied())
		.flatten()
		.copied()
		.collect();
	let mut out = b"RIFF".to_vec();
	out.extend_from_slice(&(body.len() as u32).to_le_bytes());
	out.extend_from_slice(&body);
	out
}

fn chunk(name: &[u8; 4], data: &[u8]) -> Vec<u8> {
	let mut out = name.to_vec();
	out.extend_from_slice(&(data.len() as u32).to_le_bytes());
	out.extend_from_slice(data);
	if data.len() % 2 == 1 {
		out.push(0);
	}
	out
}

/// The VP8L chunk (header included) of a canonical file.
fn vp8l(file: &[u8]) -> Vec<u8> {
	file[12..].to_vec()
}

fn vp8x(flags: u8) -> Vec<u8> {
	let mut data = vec![flags, 0, 0, 0];
	data.extend_from_slice(&[255, 0, 0, 255, 0, 0]);
	chunk(b"VP8X", &data)
}

#[test]
fn encoder_output_is_the_canonical_form() {
	let a = process_avatar(&encode(
		DynamicImage::ImageRgba8(RgbaImage::from_fn(300, 300, |x, y| {
			Rgba([x as u8, y as u8, 7, 255])
		})),
		ImageFormat::Png,
	))
	.unwrap();
	assert_eq!(&a.image[..4], b"RIFF");
	assert_eq!(&a.image[8..16], b"WEBPVP8L");
	assert_eq!(validate_canonical(&a.image).unwrap(), a.hash);
	assert!(a.image.len() <= ENCODED_MAX_BYTES);
}

#[test]
fn encoding_is_deterministic() {
	let src = encode(
		DynamicImage::ImageRgba8(RgbaImage::from_fn(400, 300, |x, y| {
			let n = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503);
			Rgba([(n >> 3) as u8, (n >> 11) as u8, (x + y) as u8, 255])
		})),
		ImageFormat::Png,
	);
	let (a, b) = (process_avatar(&src).unwrap(), process_avatar(&src).unwrap());
	assert_eq!(a.image, b.image);
	assert_eq!(a.hash, b.hash);
	assert_eq!(a.hash, *blake3::hash(&a.image).as_bytes());
}

#[test]
fn a_hand_built_canonical_file_is_accepted() {
	assert!(validate_canonical(&good()).is_ok());
	// Odd-length payloads are zero padded; the same bytes rebuilt through `riff` still pass.
	let rebuilt = riff(&[&vp8l(&good())]);
	assert_eq!(rebuilt, good());
}

#[test]
fn lossy_vp8_is_rejected() {
	not_canonical(&riff(&[&chunk(b"VP8 ", &[0x10; 20])]));
}

#[test]
fn extended_files_with_metadata_are_rejected() {
	let exif = chunk(b"EXIF", b"secret gps");
	not_canonical(&riff(&[&vp8x(0x08), &vp8l(&good()), &exif]));
	not_canonical(&riff(&[&vp8x(0), &vp8l(&good())]));
}

#[test]
fn animated_files_are_rejected() {
	let anim = chunk(b"ANIM", &[0; 6]);
	let frame = chunk(b"ANMF", &[0; 16]);
	not_canonical(&riff(&[&vp8x(0x02), &anim, &frame]));
}

#[test]
fn unknown_and_extra_chunks_are_rejected() {
	not_canonical(&riff(&[&chunk(b"JUNK", &[0; 8])]));
	let mut extra = vp8l(&good());
	extra.extend_from_slice(&chunk(b"EXIF", b"x"));
	not_canonical(&riff(&[&extra]));
	let mut twice = vp8l(&good());
	twice.extend_from_slice(&vp8l(&good()));
	not_canonical(&riff(&[&twice]));
}

#[test]
fn wrong_dimensions_are_rejected() {
	for (w, h) in [(255, 256), (256, 255), (257, 257), (128, 128), (512, 512)] {
		not_canonical(&webp(w, h));
	}
}

#[test]
fn truncated_trailing_and_bad_length_files_are_rejected() {
	let ok = good();
	for cut in [1, 5, 20, ok.len() / 2, ok.len() - 1] {
		assert!(
			validate_canonical(&ok[..ok.len() - cut]).is_err(),
			"cut {cut}"
		);
	}
	assert!(validate_canonical(&ok[..10]).is_err());
	let mut trailing = ok.clone();
	trailing.push(0);
	not_canonical(&trailing);
	let mut long = ok.clone();
	long[4..8].copy_from_slice(&((ok.len() as u32) + 100).to_le_bytes());
	not_canonical(&long);
	let mut short = ok.clone();
	short[4..8].copy_from_slice(&((ok.len() as u32) - 100).to_le_bytes());
	not_canonical(&short);
	let mut chunk_len = ok.clone();
	chunk_len[16..20].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
	not_canonical(&chunk_len);
}

#[test]
fn corrupt_bitstream_fails_the_bounded_decode() {
	let mut bad = good();
	let n = bad.len();
	for byte in &mut bad[40..n - 8] {
		*byte = 0xff;
	}
	assert!(validate_canonical(&bad).is_err());
}

#[test]
fn oversized_input_is_rejected() {
	let mut big = good();
	big.resize(ENCODED_MAX_BYTES + 1, 0);
	assert!(matches!(reason(&big), AvatarError::TooLarge { .. }));
	assert_eq!(reason(&[]), AvatarError::Empty);
}

#[test]
fn other_image_formats_are_not_canonical() {
	let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 255])));
	for fmt in [ImageFormat::Png, ImageFormat::Gif, ImageFormat::Bmp] {
		assert_eq!(
			reason(&encode(img.clone(), fmt)),
			AvatarError::UnsupportedFormat
		);
	}
	let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
	assert_eq!(
		reason(&encode(rgb, ImageFormat::Jpeg)),
		AvatarError::UnsupportedFormat
	);
}
