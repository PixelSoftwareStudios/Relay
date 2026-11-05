use relay_desktop::host::read_picked;
use relay_ui::controller::AVATAR_MAX_BYTES;

fn temp(name: &str) -> std::path::PathBuf {
	let dir = std::env::temp_dir().join(format!("relay-picker-{}", std::process::id()));
	std::fs::create_dir_all(&dir).unwrap();
	dir.join(name)
}

fn image(format: image::ImageFormat, w: u32, h: u32) -> Vec<u8> {
	let mut bytes = std::io::Cursor::new(Vec::new());
	image::DynamicImage::ImageRgb8(image::RgbImage::new(w, h))
		.write_to(&mut bytes, format)
		.unwrap();
	bytes.into_inner()
}

#[tokio::test]
async fn all_six_formats_are_read_whatever_the_name() {
	use image::ImageFormat::{Bmp, Gif, Jpeg, Png, Tiff, WebP};
	for (format, name) in [
		(Png, "a.png"),
		(Jpeg, "a.jpg"),
		(WebP, "a.webp"),
		(Gif, "a.gif"),
		(Bmp, "a.bmp"),
		(Tiff, "a.tiff"),
		(Png, "no-extension"),
	] {
		let path = temp(name);
		let bytes = image(format, 8, 8);
		std::fs::write(&path, &bytes).unwrap();
		let file = read_picked(&path).await.unwrap();
		assert!(file.name.ends_with(name));
		assert_eq!(file.bytes, bytes);
	}
}

#[tokio::test]
async fn refusals_use_the_documented_text() {
	let path = temp("wide.png");
	std::fs::write(&path, image(image::ImageFormat::Png, 2049, 1)).unwrap();
	let err = read_picked(&path).await.unwrap_err();
	assert_eq!(
		err,
		"avatar rejected: avatar dimensions 2049x1 exceed the 2048 pixel limit"
	);
	let path = temp("text.png");
	std::fs::write(&path, b"not an image").unwrap();
	let err = read_picked(&path).await.unwrap_err();
	assert_eq!(
		err,
		"avatar rejected: unsupported or unrecognised image format (use PNG, JPEG, WebP, GIF, BMP or TIFF)"
	);
	let path = temp("empty.png");
	std::fs::write(&path, []).unwrap();
	let err = read_picked(&path).await.unwrap_err();
	assert_eq!(
		err,
		"avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)"
	);
}

#[tokio::test]
async fn an_oversized_file_is_refused_before_reading() {
	let path = temp("big.jpg");
	std::fs::write(&path, vec![0u8; AVATAR_MAX_BYTES + 5]).unwrap();
	let err = read_picked(&path).await.unwrap_err();
	assert!(err.contains("1 byte to 1 MiB"), "{err}");
}

#[tokio::test]
async fn missing_files_are_refused() {
	assert!(read_picked(&temp("missing.png")).await.is_err());
}
