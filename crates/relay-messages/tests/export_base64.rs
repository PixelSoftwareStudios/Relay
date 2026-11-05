//! Embedded image bytes survive the export round trip and hostile payloads are rejected.

mod common;

use std::sync::Arc;

use common::*;
use proptest::prelude::*;
use relay_messages::store::{Export, ExportContent, ExportOptions};

fn image_with(data: Option<&str>) -> ExportContent {
	ExportContent::Image {
		media_type: "image/png".into(),
		width: 1,
		height: 1,
		frames: 1,
		bytes_len: 0,
		data_base64: data.map(str::to_owned),
	}
}

#[test]
fn embedded_image_round_trips_through_json() {
	let (mut s, _clock) = store();
	let bytes = png(33, 17);
	s.compose_image(Arc::from(bytes.clone())).unwrap();
	let json = s.export_json(ExportOptions { embed_images: true });
	let parsed = Export::from_json(&json).unwrap();
	assert_eq!(
		Export::decode_image(&parsed.messages[0].content),
		Some(bytes)
	);
}

#[test]
fn rfc4648_vectors_decode() {
	for (text, plain) in [
		("", ""),
		("Zg==", "f"),
		("Zm8=", "fo"),
		("Zm9v", "foo"),
		("Zm9vYg==", "foob"),
		("Zm9vYmE=", "fooba"),
		("Zm9vYmFy", "foobar"),
	] {
		assert_eq!(
			Export::decode_image(&image_with(Some(text))),
			Some(plain.as_bytes().to_vec()),
			"{text}"
		);
	}
}

#[test]
fn hostile_payloads_are_rejected() {
	for text in [
		"Zg=",       // short padding
		"Zg",        // missing padding
		"Zg==Zg==",  // padding in the middle
		"Zh==",      // non-canonical trailing bits
		"Zm9=",      // non-canonical trailing bits
		"Zm9v\n",    // trailing whitespace
		" Zm9v",     // leading whitespace
		"Zm 9v",     // embedded whitespace
		"Zm9v====",  // excess padding
		"Zm9*",      // outside the alphabet
		"Zm9-",      // url-safe alphabet
		"Zm9_",      // url-safe alphabet
		"Zm9\u{e9}", // non-ASCII
		"====",      // padding only
		"\u{0}\u{0}\u{0}\u{0}",
	] {
		assert_eq!(
			Export::decode_image(&image_with(Some(text))),
			None,
			"{text:?}"
		);
	}
	assert_eq!(Export::decode_image(&image_with(None)), None);
}

proptest! {
	#[test]
	fn arbitrary_bytes_round_trip(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
		use base64::Engine as _;
		let text = base64::engine::general_purpose::STANDARD.encode(&bytes);
		prop_assert_eq!(Export::decode_image(&image_with(Some(&text))), Some(bytes));
	}

	#[test]
	fn arbitrary_text_never_panics(text in ".{0,200}") {
		let _ = Export::decode_image(&image_with(Some(&text)));
	}
}
