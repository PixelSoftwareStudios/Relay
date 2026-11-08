#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::transfer::fuzzing::{decode_frame, encode_frame};

fuzz_target!(|data: &[u8]| {
	if let Ok(message) = decode_frame(data) {
		// Frames are canonical: what decodes encodes back to the same bytes.
		assert_eq!(encode_frame(&message).expect("a decoded frame encodes"), data);
	}
});
