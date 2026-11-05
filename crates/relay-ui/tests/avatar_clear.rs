//! Clearing the stored avatar, the six-format upload pre-check, and short fingerprints.

mod common;

use common::*;
use image::{DynamicImage, ImageFormat, RgbImage};
use relay_api::pb::{self, CallState};
use relay_ui::convert;
use relay_ui::engine::Command;
use relay_ui::host_options::{LoginStatus, login_text};
use relay_ui::imaging::avatar_check;
use relay_ui::model::{AppState, Peer};
use relay_ui::viewmodel::{call_cards, peer_rows};

fn encoded(format: ImageFormat, w: u32, h: u32) -> Vec<u8> {
	let mut bytes = std::io::Cursor::new(Vec::new());
	DynamicImage::ImageRgb8(RgbImage::new(w, h))
		.write_to(&mut bytes, format)
		.unwrap();
	bytes.into_inner()
}

#[test]
fn all_six_formats_pass_the_precheck_by_content() {
	for (format, media) in [
		(ImageFormat::Png, "image/png"),
		(ImageFormat::Jpeg, "image/jpeg"),
		(ImageFormat::WebP, "image/webp"),
		(ImageFormat::Gif, "image/gif"),
		(ImageFormat::Bmp, "image/bmp"),
		(ImageFormat::Tiff, "image/tiff"),
	] {
		assert_eq!(avatar_check(&encoded(format, 16, 8)), Ok(media));
	}
	assert!(avatar_check(&encoded(ImageFormat::Png, 2048, 2048)).is_ok());
}

#[test]
fn refusals_use_the_documented_texts() {
	let size = "avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)";
	assert_eq!(avatar_check(&[]), Err(size.to_owned()));
	assert_eq!(
		avatar_check(&vec![0; 1024 * 1024 + 1]),
		Err(size.to_owned())
	);
	assert_eq!(
		avatar_check(b"hello, not an image"),
		Err("avatar rejected: unsupported or unrecognised image format (use PNG, JPEG, WebP, GIF, BMP or TIFF)".to_owned())
	);
	assert_eq!(
		avatar_check(&encoded(ImageFormat::Png, 2049, 10)),
		Err("avatar rejected: avatar dimensions 2049x10 exceed the 2048 pixel limit".to_owned())
	);
}

#[tokio::test]
async fn clearing_the_stored_avatar_is_its_own_engine_command() {
	let (ctl, _hub, ops) = controller_with(snapshot(1, vec![]), 0);
	ctl.clear_avatar().await.unwrap();
	assert_eq!(ops.recorded(), [Command::ClearAvatar]);
}

#[test]
fn peers_carry_a_sanitised_short_fingerprint_into_the_prompt_and_the_list() {
	let mut wire = pb_peer("Ada");
	wire.fingerprint_short = "a1b2-c3d4-e5f6-0718-293a\u{1b}[31m<script>".into();
	let peer: Peer = convert::peer(&wire);
	assert_eq!(peer.fingerprint_short, "A1B2-C3D4-E5F6-0718-293A");
	let mut state = AppState::default();
	state.peers.push(peer);
	let rows = peer_rows(&state);
	assert_eq!(rows[0].fingerprint, "A1B2-C3D4-E5F6-0718-293A");

	let mut call = pb_call(CallState::IncomingRinging, None);
	call.peer_id = wire.peer_id.clone();
	let snap = snapshot(1, vec![call]);
	state.load_snapshot(&snap);
	state.peers = vec![convert::peer(&wire)];
	let card = &call_cards(&state, &Default::default())[0];
	assert!(card.incoming);
	assert_eq!(card.fingerprint, "A1B2-C3D4-E5F6-0718-293A");
	assert_eq!(card.trust, "Verified");
	let _ = pb::TrustState::Verified;
}

#[test]
fn login_status_text_is_honest_about_every_state() {
	assert!(login_text(&LoginStatus::Off).starts_with("Off"));
	assert!(login_text(&LoginStatus::On).starts_with("On"));
	assert!(login_text(&LoginStatus::Failed("denied".into())).contains("denied"));
	assert!(login_text(&LoginStatus::Unsupported("no".into())).contains("Not available"));
}
