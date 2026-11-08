//! The libFuzzer targets for image headers and markdown, run as plain property loops.

use proptest::prelude::*;
use relay_messages::image::inspect;
use relay_messages::limits::MAX_IMAGE_PIXELS;
use relay_messages::markdown::{parse, parse_http};

const MAGICS: [&[u8]; 4] = [
	b"\x89PNG\r\n\x1a\n",
	b"\xFF\xD8\xFF",
	b"GIF89a",
	b"RIFF\0\0\0\0WEBP",
];

fn headerish() -> impl Strategy<Value = Vec<u8>> {
	(
		0..MAGICS.len(),
		proptest::collection::vec(any::<u8>(), 0..200),
	)
		.prop_map(|(i, rest)| [MAGICS[i], rest.as_slice()].concat())
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(1500))]

	#[test]
	fn image_headers_never_panic_and_stay_in_limits(
		data in prop_oneof![headerish(), proptest::collection::vec(any::<u8>(), 0..300)],
	) {
		if let Ok(info) = inspect(&data) {
			prop_assert!(info.width > 0 && info.height > 0 && info.frames > 0);
			prop_assert!(u64::from(info.width) * u64::from(info.height) <= MAX_IMAGE_PIXELS);
		}
	}

	#[test]
	fn markdown_and_links_never_panic(text in "\\PC{0,600}") {
		let _ = parse(&text);
		let _ = parse_http(&text);
		for word in text.split_whitespace() {
			let _ = parse_http(word);
		}
	}

	#[test]
	fn markdown_syntax_soup_never_panics(text in "[ -~\\n\\t*_`\\[\\]()<>#!~|\\\\:/.@-]{0,400}") {
		let _ = parse(&text);
	}
}
