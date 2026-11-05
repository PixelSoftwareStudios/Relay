#![no_main]

use libfuzzer_sys::fuzz_target;
use prost::Message;
use relay_net::control::validate_envelope;
use relay_net::proto::Envelope;

fuzz_target!(|data: &[u8]| {
	if let Ok(envelope) = Envelope::decode(data) {
		let _ = validate_envelope(&envelope);
	}
});
