use relay_ui::messaging::image::{ImageProblem, decode_inline};

fn png(w: u32, h: u32) -> Vec<u8> {
	use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
	let rgba = vec![200u8; (w * h * 4) as usize];
	let mut out = Vec::new();
	PngEncoder::new(&mut out)
		.write_image(&rgba, w, h, ExtendedColorType::Rgba8)
		.unwrap();
	out
}

#[test]
fn a_small_png_decodes() {
	let d = decode_inline(&png(32, 16)).unwrap();
	assert_eq!(d.size, [32, 16]);
}

#[test]
fn big_pictures_are_scaled_to_a_thumbnail() {
	let d = decode_inline(&png(500, 20)).unwrap();
	assert!(d.size[0].max(d.size[1]) <= 384);
}

#[test]
fn over_the_inline_cap_is_refused_first() {
	assert_eq!(
		decode_inline(&vec![0u8; 15 * 1024 + 1]).err(),
		Some(ImageProblem::TooLarge)
	);
}

#[test]
fn garbage_is_invalid() {
	assert_eq!(
		decode_inline(b"not an image at all").err(),
		Some(ImageProblem::Invalid)
	);
}

/// A tiny file whose header claims 5000 x 5000 pixels (25 MP): refused from
/// the header, nothing is allocated.
#[test]
fn a_pixel_bomb_header_is_refused_without_decoding() {
	let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
	b.extend_from_slice(&13u32.to_be_bytes());
	b.extend_from_slice(b"IHDR");
	b.extend_from_slice(&5000u32.to_be_bytes());
	b.extend_from_slice(&5000u32.to_be_bytes());
	b.extend_from_slice(&[8, 6, 0, 0, 0]);
	b.extend_from_slice(&[0; 4]);
	let err = decode_inline(&b).err();
	assert!(
		matches!(
			err,
			Some(ImageProblem::TooManyPixels | ImageProblem::Invalid)
		),
		"{err:?}"
	);
}
