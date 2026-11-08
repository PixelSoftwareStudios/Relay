#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::channel::{Delegation, InviteToken, MembershipCert, Revocation};

fuzz_target!(|data: &[u8]| {
	if let Ok(text) = std::str::from_utf8(data) {
		if let Ok(token) = InviteToken::parse(text) {
			// A parsed token re-encodes to text that parses to the same token.
			let again = InviteToken::parse(&token.encode()).expect("own encoding parses");
			assert_eq!(again.encode(), token.encode());
		}
	}
	if let Ok(cert) = MembershipCert::from_bytes(data) {
		assert_eq!(cert.to_bytes(), data);
	}
	if let Ok(revocation) = Revocation::from_bytes(data) {
		assert_eq!(revocation.to_bytes(), data);
	}
	let _ = Delegation::from_bytes(data);
});
