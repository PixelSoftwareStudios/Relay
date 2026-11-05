//! `relay avatar set|clear`: the pre-check mirrors the engine's six formats, size and
//! dimension limits, and clearing goes through `ClearAvatar`.

mod common;

use common::{FakeControl, run_with};
use image::{DynamicImage, ImageFormat, RgbImage};
use relay_cli::cli::exit;

fn encoded(format: ImageFormat, w: u32, h: u32) -> Vec<u8> {
	let mut bytes = std::io::Cursor::new(Vec::new());
	DynamicImage::ImageRgb8(RgbImage::new(w, h))
		.write_to(&mut bytes, format)
		.unwrap();
	bytes.into_inner()
}

async fn set(c: &mut FakeControl, name: &str, bytes: &[u8]) -> Result<String, String> {
	let dir = tempfile::tempdir().unwrap();
	let file = dir.path().join(name);
	std::fs::write(&file, bytes).unwrap();
	run_with(c, &["avatar", "set", file.to_str().unwrap()], false)
		.await
		.map_err(|e| e.to_string())
}

#[tokio::test]
async fn all_six_formats_are_accepted_whatever_the_extension() {
	let mut c = FakeControl::new();
	for (format, name) in [
		(ImageFormat::Png, "a.png"),
		(ImageFormat::Jpeg, "a.jpg"),
		(ImageFormat::WebP, "a.webp"),
		(ImageFormat::Gif, "a.gif"),
		(ImageFormat::Bmp, "a.bmp"),
		(ImageFormat::Tiff, "a.tif"),
		// The format is sniffed from the bytes, not the name.
		(ImageFormat::Png, "photo.dat"),
	] {
		set(&mut c, name, &encoded(format, 8, 8)).await.unwrap();
	}
	assert_eq!(c.log.len(), 7, "{:?}", c.log);
}

#[tokio::test]
async fn refusals_use_the_engines_wording_and_never_reach_it() {
	let mut c = FakeControl::new();
	let err = set(&mut c, "a.png", &[]).await.unwrap_err();
	assert_eq!(
		err,
		"avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)"
	);
	let big = vec![0u8; 1024 * 1024 + 1];
	let err = set(&mut c, "big.png", &big).await.unwrap_err();
	assert!(err.starts_with("avatar must be 1 byte to 1 MiB"), "{err}");
	let err = set(&mut c, "a.png", b"not an image").await.unwrap_err();
	assert_eq!(
		err,
		"avatar rejected: unsupported or unrecognised image format (use PNG, JPEG, WebP, GIF, BMP or TIFF)"
	);
	let err = set(&mut c, "wide.png", &encoded(ImageFormat::Png, 2049, 2))
		.await
		.unwrap_err();
	assert_eq!(
		err,
		"avatar rejected: avatar dimensions 2049x2 exceed the 2048 pixel limit"
	);
	assert!(c.log.is_empty(), "{:?}", c.log);
}

#[tokio::test]
async fn clear_uses_clear_avatar() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["avatar", "clear"], false).await.unwrap();
	assert_eq!(c.log, ["clear_avatar"]);
	let mut failing = FakeControl::new();
	failing.result.state = relay_api::pb::OperationState::Failed as i32;
	failing.result.problem = Some(relay_api::pb::Problem {
		code: "BUSY".into(),
		..Default::default()
	});
	let err = run_with(&mut failing, &["avatar", "clear"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::REJECTED);
}
