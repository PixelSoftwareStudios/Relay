#![no_main]

use libfuzzer_sys::fuzz_target;
use prost::Message;
use relay_net::control::session_msg::validate;
use relay_net::proto::SessionMessage;

fuzz_target!(|data: &[u8]| {
	if let Ok(message) = SessionMessage::decode(data) {
		let _ = validate(&message);
	}
});
