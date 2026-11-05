#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_config::avatar::{parse_hash, process_avatar, validate_canonical};

fuzz_target!(|data: &[u8]| {
	if let Ok(avatar) = process_avatar(data) {
		// Our own re-encoded output must validate as canonical.
		assert!(validate_canonical(&avatar.image).is_ok());
	}
	let _ = validate_canonical(data);
	if let Ok(text) = std::str::from_utf8(data) {
		let _ = parse_hash(text);
	}
});
