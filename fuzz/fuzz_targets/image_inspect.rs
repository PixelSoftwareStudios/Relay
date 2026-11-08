#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_messages::image::inspect;
use relay_messages::limits::MAX_IMAGE_PIXELS;

fuzz_target!(|data: &[u8]| {
	if let Ok(info) = inspect(data) {
		// Accepted headers stay inside the pixel and frame limits.
		assert!(info.width > 0 && info.height > 0 && info.frames > 0);
		assert!(u64::from(info.width) * u64::from(info.height) <= MAX_IMAGE_PIXELS);
	}
});
