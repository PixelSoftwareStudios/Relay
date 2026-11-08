#![no_main]

use std::net::IpAddr;

use libfuzzer_sys::fuzz_target;
use relay_net::AddressPolicy;
use relay_net::discovery::{PlatformRecord, parse_platform_record};

// The input is split on NUL into: host name, port bytes, then `key=value` TXT pairs.
fuzz_target!(|data: &[u8]| {
	let mut parts = data.split(|b| *b == 0);
	let text = |part: Option<&[u8]>| String::from_utf8_lossy(part.unwrap_or_default()).into_owned();
	let fullname = text(parts.next());
	let port = parts
		.next()
		.and_then(|p| <[u8; 2]>::try_from(p).ok())
		.map_or(47800, u16::from_be_bytes);
	let mut txt = Vec::new();
	let mut addresses: Vec<IpAddr> = Vec::new();
	for part in parts {
		let entry = String::from_utf8_lossy(part).into_owned();
		if let Ok(ip) = entry.parse::<IpAddr>() {
			addresses.push(ip);
		} else if let Some((key, value)) = entry.split_once('=') {
			txt.push((key.to_owned(), value.to_owned()));
		}
	}
	let record = PlatformRecord {
		fullname,
		port,
		addresses,
		txt,
	};
	for policy in [AddressPolicy::lan_only(), AddressPolicy::lan_and_loopback()] {
		let _ = parse_platform_record(&record, &policy);
	}
});
