#![no_main]

use libfuzzer_sys::fuzz_target;
use relay_net::channel::InviteToken;
use relay_net::channel::cert::{MembershipCert, Revocation};
use relay_net::channel::tag::ChannelAdvert;

fuzz_target!(|data: &[u8]| {
	// Binary certificate and revocation encodings.
	let _ = MembershipCert::from_bytes(data);
	let _ = Revocation::from_bytes(data);
	// Text forms: invite strings and mDNS TXT values.
	if let Ok(text) = std::str::from_utf8(data) {
		let _ = InviteToken::parse(text);
		let (epoch, rest) = text.split_once('\n').unwrap_or((text, ""));
		let (tags, rest) = rest.split_once('\n').unwrap_or((rest, ""));
		let listed: Vec<&str> = rest.split('\n').take(8).collect();
		let _ = ChannelAdvert::from_txt(Some(epoch), Some(tags), &listed);
	}
});
