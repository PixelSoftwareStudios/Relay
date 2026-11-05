#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::record::RecordHeader;

fuzz_target!(|data: &[u8]| {
	if let Ok(header) = RecordHeader::parse(data) {
		assert_eq!(&header.encode()[..], &data[..32]);
	}
});
