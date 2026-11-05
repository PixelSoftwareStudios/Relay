mod common;

use std::sync::Arc;

use common::*;
use relay_messages::StoreError;
use relay_messages::image::*;
use relay_messages::limits::*;

#[test]
fn accepts_each_format_and_reads_dimensions() {
	let i = inspect(&png(640, 480)).unwrap();
	assert_eq!(
		(i.format, i.width, i.height, i.frames),
		(ImageFormat::Png, 640, 480, 1)
	);
	let i = inspect(&jpeg(1920, 1080)).unwrap();
	assert_eq!(
		(i.format, i.width, i.height),
		(ImageFormat::Jpeg, 1920, 1080)
	);
	let i = inspect(&gif(100, 50, 1)).unwrap();
	assert_eq!(
		(i.format, i.width, i.height, i.frames),
		(ImageFormat::Gif, 100, 50, 1)
	);
	let i = inspect(&webp_vp8x(300, 200, 0)).unwrap();
	assert_eq!(
		(i.format, i.width, i.height, i.frames),
		(ImageFormat::Webp, 300, 200, 1)
	);
	assert_eq!(i.format.media_type(), "image/webp");
}

#[test]
fn pixel_ceiling_is_checked_from_the_header_before_decoding() {
	// Exactly 16 Mi pixels passes, one more column fails.
	assert!(inspect(&png(4096, 4096)).is_ok());
	assert_eq!(
		inspect(&png(4097, 4096)),
		Err(ImageError::TooManyPixels {
			width: 4097,
			height: 4096
		})
	);
	// A 40-byte file declaring a 100000 x 100000 image (a classic bomb) is refused without any decoder.
	let bomb = png(100_000, 100_000);
	assert!(bomb.len() < 64);
	assert!(matches!(
		inspect(&bomb),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&png(u32::MAX, u32::MAX)),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&png(0, 10)),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&png(20_000, 10)),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&jpeg(65_535, 65_535)),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&gif(65_535, 65_535, 1)),
		Err(ImageError::TooManyPixels { .. })
	));
	assert!(matches!(
		inspect(&webp_vp8x(16_000_000, 16_000_000, 0)),
		Err(ImageError::TooManyPixels { .. })
	));
	// 64 megapixels (the rejected suggestion) is refused.
	assert!(matches!(
		inspect(&png(8192, 8192)),
		Err(ImageError::TooManyPixels { .. })
	));
}

#[test]
fn compressed_size_limit() {
	let mut ok = png(10, 10);
	ok.resize(MAX_IMAGE_BYTES, 0);
	assert!(inspect(&ok).is_ok());
	ok.push(0);
	assert_eq!(inspect(&ok), Err(ImageError::TooLarge));
}

#[test]
fn animation_limits() {
	assert_eq!(inspect(&apng(100, 100, 100)).unwrap().frames, 100);
	assert_eq!(
		inspect(&apng(100, 100, 101)),
		Err(ImageError::AnimationTooLarge)
	);
	assert_eq!(inspect(&apng(100, 100, 0)), Err(ImageError::Malformed));
	assert_eq!(inspect(&gif(10, 10, 100)).unwrap().frames, 100);
	assert_eq!(
		inspect(&gif(10, 10, 101)),
		Err(ImageError::AnimationTooLarge)
	);
	assert_eq!(inspect(&webp_vp8x(10, 10, 100)).unwrap().frames, 100);
	assert_eq!(
		inspect(&webp_vp8x(10, 10, 101)),
		Err(ImageError::AnimationTooLarge)
	);
	// Total decoded memory: 100 frames of 1900x1900 RGBA = 1.4 GB.
	assert_eq!(
		inspect(&apng(1900, 1900, 100)),
		Err(ImageError::AnimationTooLarge)
	);
	assert!(inspect(&apng(1000, 1000, 30)).is_ok());
	assert!(inspect(&gif(1000, 1000, 30)).unwrap().animated());
	// 33 x 1000x1000x4 = 132 MB exceeds 128 MiB? 128 MiB = 134 MB, so 33 passes and 34 fails.
	assert!(inspect(&apng(1000, 1000, 33)).is_ok());
	assert_eq!(
		inspect(&apng(1000, 1000, 34)),
		Err(ImageError::AnimationTooLarge)
	);
}

#[test]
fn malformed_and_unsupported() {
	assert_eq!(inspect(b""), Err(ImageError::Unsupported));
	assert_eq!(inspect(b"BM12345678"), Err(ImageError::Unsupported));
	assert_eq!(
		inspect(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
		Err(ImageError::Unsupported)
	);
	assert_eq!(inspect(b"\x89PNG\r\n\x1a\n"), Err(ImageError::Malformed));
	assert_eq!(inspect(&png(10, 10)[..20]), Err(ImageError::Malformed));
	assert_eq!(inspect(&[0xFF, 0xD8, 0xFF]), Err(ImageError::Malformed));
	assert_eq!(inspect(b"GIF89a"), Err(ImageError::Malformed));
	assert_eq!(inspect(b"RIFF\0\0\0\0WEBP"), Err(ImageError::Malformed));
	let mut bad = png(10, 10);
	bad[25] = 9;
	assert_eq!(inspect(&bad), Err(ImageError::Malformed));
}

#[test]
fn truncated_prefixes_never_panic() {
	for img in [
		png(50, 50),
		apng(50, 50, 3),
		jpeg(50, 50),
		gif(50, 50, 3),
		webp_vp8x(50, 50, 3),
	] {
		for n in 0..img.len() {
			let _ = inspect(&img[..n]);
		}
	}
}

#[test]
fn store_keeps_original_bytes_and_refuses_bad_images() {
	let (mut s, _) = store();
	let original = jpeg(800, 600);
	let w = s.compose_image(Arc::from(original.clone())).unwrap();
	assert!(matches!(&w.payload, relay_messages::Payload::Image(b) if **b == *original));
	match &s.messages()[0].body {
		relay_messages::MessageBody::Image(i) => {
			assert_eq!(&*i.bytes, &original[..]);
			assert_eq!((i.info.width, i.info.height), (800, 600));
		}
		b => panic!("{b:?}"),
	}
	let e = s
		.compose_image(Arc::from(png(100_000, 100_000)))
		.unwrap_err();
	assert!(matches!(
		e,
		StoreError::Image(ImageError::TooManyPixels { .. })
	));
	assert_eq!(e.reject_reason(), relay_messages::RejectReason::TooLarge);
	let e = s
		.compose_image(Arc::from(b"not an image".to_vec()))
		.unwrap_err();
	assert_eq!(e.reject_reason(), relay_messages::RejectReason::Unsupported);
	assert_eq!(s.messages().len(), 1);
}
