#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_messages::markdown::{parse, parse_http};

fuzz_target!(|data: &[u8]| {
	if let Ok(text) = std::str::from_utf8(data) {
		let _ = parse(text);
		let _ = parse_http(text);
		for word in text.split_whitespace() {
			let _ = parse_http(word);
		}
	}
});
