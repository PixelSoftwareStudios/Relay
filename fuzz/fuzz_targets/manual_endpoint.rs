#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::AddressPolicy;

fuzz_target!(|data: &[u8]| {
	let Ok(text) = std::str::from_utf8(data) else {
		return;
	};
	let policy = AddressPolicy::lan_only();
	if let Ok(addr) = policy.parse_manual(text) {
		// Whatever parses must satisfy the policy that parsed it.
		assert!(policy.check_socket(addr).is_ok());
	}
	let _ = AddressPolicy::lan_and_loopback().parse_manual(text);
});
