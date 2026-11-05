use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use relay_desktop::avatar::DEFAULT_PNG;
use relay_desktop::imaging::{MAX_TEXTURE_DIM, decode_for_texture};

fn png(width: u32, height: u32) -> Vec<u8> {
	let mut out = Vec::new();
	RgbaImage::new(width, height)
		.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
		.unwrap();
	out
}

#[test]
fn the_default_avatar_decodes_at_its_size() {
	let d = decode_for_texture(DEFAULT_PNG).unwrap();
	assert_eq!(d.size, [256, 256]);
	assert_eq!(d.rgba.len(), 256 * 256 * 4);
}

#[test]
fn large_images_are_scaled_down_keeping_the_aspect_ratio() {
	let d = decode_for_texture(&png(1024, 512)).unwrap();
	assert_eq!(
		d.size,
		[MAX_TEXTURE_DIM as usize, MAX_TEXTURE_DIM as usize / 2]
	);
}

#[test]
fn garbage_and_truncated_input_is_rejected_not_panicked_on() {
	assert!(decode_for_texture(b"not an image").is_none());
	assert!(decode_for_texture(&[]).is_none());
	let whole = png(64, 64);
	assert!(decode_for_texture(&whole[..whole.len() / 2]).is_none());
}

#[test]
fn absurd_dimensions_hit_the_decoder_limits() {
	assert!(decode_for_texture(&png(5000, 1)).is_none());
}

#[test]
fn the_engines_lossless_webp_avatars_decode() {
	let mut out = Vec::new();
	RgbaImage::new(64, 64)
		.write_to(&mut Cursor::new(&mut out), ImageFormat::WebP)
		.unwrap();
	assert_eq!(decode_for_texture(&out).unwrap().size, [64, 64]);
}
