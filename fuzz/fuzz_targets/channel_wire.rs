#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::channel::limits::MESSAGE_MAX_BYTES;
use relay_net::channel::wire;

fuzz_target!(|data: &[u8]| {
	let result = wire::decode(data);
	if data.len() > MESSAGE_MAX_BYTES {
		assert!(result.is_err());
	}
	if let Ok(message) = result {
		let _ = wire::validate(&message);
	}
});
