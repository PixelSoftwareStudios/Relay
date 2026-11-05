#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_protocol::rtp::RtpProfile;

fuzz_target!(|data: &[u8]| {
	if let Ok(packet) = RtpProfile::V1.parse(data) {
		// A parsed payload borrows from the datagram and stays within the profile bound.
		assert!(packet.payload.len() <= data.len());
	}
});
