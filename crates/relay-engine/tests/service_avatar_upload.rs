#![cfg(unix)]
//! What `UploadAvatar` accepts and the exact words it refuses with.

#[macro_use]
mod service_support;
mod support_daemon;

use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use relay_api::RelayClient;
use relay_api::pb::{Operation, UploadAvatarRequest};
use relay_engine::daemon::DaemonHandle;
use service_support::*;
use support_daemon::engine_options;
use tonic::Code;

fn encoded(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
	let image = match format {
		ImageFormat::Jpeg | ImageFormat::Bmp | ImageFormat::Tiff => DynamicImage::ImageRgb8(
			RgbImage::from_pixel(width, height, image::Rgb([20, 90, 200])),
		),
		_ => DynamicImage::ImageRgba8(RgbaImage::from_pixel(
			width,
			height,
			image::Rgba([200, 30, 30, 255]),
		)),
	};
	let mut out = std::io::Cursor::new(Vec::new());
	image.write_to(&mut out, format).expect("encode");
	out.into_inner()
}

async fn upload(cli: &mut RelayClient, op: u8, bytes: Vec<u8>) -> Result<Operation, tonic::Status> {
	let request = UploadAvatarRequest {
		context: Some(ctx(cli, op)),
		encoded_image: bytes,
		declared_media_type: String::new(),
	};
	cli.raw()
		.upload_avatar(request)
		.await
		.map(|r| r.into_inner())
}

#[tokio::test]
async fn six_formats_are_accepted_and_refusals_name_them() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.unwrap();
	let (options, _sim) = engine_options(home.path(), 440.0);
	let daemon = DaemonHandle::start(options).await.unwrap();
	let mut cli = RelayClient::connect(daemon.endpoint()).await.unwrap();
	cli.register("relay-cli", "test", &[]).await.unwrap();

	let formats = [
		ImageFormat::Png,
		ImageFormat::Jpeg,
		ImageFormat::WebP,
		ImageFormat::Gif,
		ImageFormat::Bmp,
		ImageFormat::Tiff,
	];
	for (n, format) in formats.into_iter().enumerate() {
		let op = upload(&mut cli, 10 + n as u8, encoded(format, 120, 90))
			.await
			.unwrap();
		assert_applied(&op);
	}

	let words = "PNG, JPEG, WebP, GIF, BMP or TIFF";
	let garbage = upload(&mut cli, 30, b"definitely not an image".to_vec())
		.await
		.unwrap();
	assert_failed(&garbage, "INVALID_REQUEST");
	let message = &garbage.problem.unwrap().message;
	assert!(message.contains(words), "{message}");

	for (op, bytes) in [(31, Vec::new()), (32, vec![0u8; 1024 * 1024 + 1])] {
		let refused = upload(&mut cli, op, bytes).await.unwrap_err();
		assert_eq!(refused.code(), Code::InvalidArgument);
		assert!(
			refused.message().contains("1 byte to 1 MiB"),
			"{}",
			refused.message()
		);
		assert!(refused.message().contains(words), "{}", refused.message());
	}

	let wide = upload(&mut cli, 33, encoded(ImageFormat::Png, 3000, 40))
		.await
		.unwrap();
	assert_failed(&wide, "INVALID_REQUEST");
	let message = wide.problem.unwrap().message;
	assert!(message.contains("2048"), "{message}");
	daemon.shutdown().await.unwrap();
}
