//! IP address detection for the redactor, built on the std parsers.

use std::net::{IpAddr, SocketAddr};

/// True for an IP literal as it appears in logs: `10.0.0.1`, `10.0.0.1:7000`, `fe80::1`,
/// `::1`, `::ffff:10.0.0.1`, `fe80::1%en0`, `[fd00::1]` and `[fd00::1]:7000`.
pub(super) fn looks_like_ip(s: &str) -> bool {
	// A token without a hex digit (such as a bare `::`) is punctuation, not an address.
	if !s.bytes().any(|b| b.is_ascii_hexdigit()) {
		return false;
	}
	if s.parse::<SocketAddr>().is_ok() {
		return true;
	}
	let (host, port) = match s.strip_prefix('[') {
		Some(rest) => match rest.split_once(']') {
			Some((host, "")) => (host, None),
			Some((host, tail)) => match tail.strip_prefix(':') {
				Some(port) => (host, Some(port)),
				None => return false,
			},
			None => return false,
		},
		None => (s, None),
	};
	if port.is_some_and(|p| p.parse::<u16>().is_err()) {
		return false;
	}
	let host = match host.split_once('%') {
		Some((addr, zone)) if is_zone(zone) => addr,
		Some(_) => return false,
		None => host,
	};
	host.parse::<IpAddr>().is_ok()
}

fn is_zone(zone: &str) -> bool {
	!zone.is_empty()
		&& zone
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}
