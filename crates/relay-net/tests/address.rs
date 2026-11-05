use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use relay_net::proto::Endpoint;
use relay_net::{AddressError, AddressPolicy, endpoint_to_proto, is_lan_ip};

fn ip(text: &str) -> IpAddr {
	text.parse().unwrap()
}

#[test]
fn lan_ranges_are_accepted_and_public_ranges_rejected() {
	for ok in [
		"10.0.0.5",
		"172.16.3.4",
		"172.31.255.1",
		"192.168.1.20",
		"169.254.10.1",
		"fe80::1",
		"fd12:3456::1",
	] {
		assert!(is_lan_ip(ip(ok)), "{ok}");
	}
	for bad in [
		"8.8.8.8",
		"172.32.0.1",
		"11.0.0.1",
		"100.64.0.1",
		"2001:db8::1",
		"2606:4700::1111",
		"0.0.0.0",
		"224.0.0.1",
		"::",
	] {
		assert!(!is_lan_ip(ip(bad)), "{bad}");
	}
}

#[test]
fn ipv4_mapped_public_addresses_do_not_bypass_the_policy() {
	assert!(!is_lan_ip(ip("::ffff:8.8.8.8")));
	assert!(is_lan_ip(ip("::ffff:192.168.1.1")));
}

#[test]
fn loopback_requires_the_explicit_policy() {
	assert_eq!(
		AddressPolicy::lan_only().check_ip(ip("127.0.0.1")),
		Err(AddressError::Loopback)
	);
	assert_eq!(
		AddressPolicy::lan_only().check_ip(ip("::1")),
		Err(AddressError::Loopback)
	);
	assert!(
		AddressPolicy::lan_and_loopback()
			.check_ip(ip("127.0.0.1"))
			.is_ok()
	);
}

#[test]
fn manual_entry_parses_endpoints_and_refuses_everything_else() {
	let p = AddressPolicy::lan_only();
	assert_eq!(
		p.parse_manual("192.168.1.5:5000").unwrap(),
		"192.168.1.5:5000".parse::<SocketAddr>().unwrap()
	);
	let v6 = p.parse_manual("[fe80::1%4]:5000").unwrap();
	assert!(matches!(v6, SocketAddr::V6(a) if a.scope_id() == 4));
	assert!(p.parse_manual("[fd00::9]:80").is_ok());

	assert_eq!(
		p.parse_manual("[fe80::1]:5000"),
		Err(AddressError::MissingScope)
	);
	assert_eq!(p.parse_manual("8.8.8.8:80"), Err(AddressError::NotLan));
	assert_eq!(p.parse_manual("192.168.1.5:0"), Err(AddressError::ZeroPort));
	assert_eq!(
		p.parse_manual("http://192.168.1.5:80"),
		Err(AddressError::NotBareEndpoint)
	);
	assert_eq!(
		p.parse_manual("user:pw@192.168.1.5:80"),
		Err(AddressError::NotBareEndpoint)
	);
	assert_eq!(
		p.parse_manual("192.168.1.5:80/path"),
		Err(AddressError::NotBareEndpoint)
	);
	assert_eq!(
		p.parse_manual("printer.local:80"),
		Err(AddressError::Malformed)
	);
	assert_eq!(p.parse_manual("192.168.1.5"), Err(AddressError::Malformed));
	assert_eq!(
		p.parse_manual("192.168.1.5:99999"),
		Err(AddressError::Malformed)
	);
	assert_eq!(p.parse_manual(""), Err(AddressError::Malformed));
}

#[test]
fn remote_candidates_are_validated_and_remote_scope_ids_not_trusted() {
	let p = AddressPolicy::lan_only();
	let good = endpoint_to_proto("192.168.1.9:6000".parse().unwrap());
	assert_eq!(
		p.candidate_from_proto(&good).unwrap(),
		"192.168.1.9:6000".parse::<SocketAddr>().unwrap()
	);

	let public = endpoint_to_proto("8.8.8.8:6000".parse().unwrap());
	assert_eq!(p.candidate_from_proto(&public), Err(AddressError::NotLan));

	let link_local = Endpoint {
		ip_address: "fe80::1"
			.parse::<std::net::Ipv6Addr>()
			.unwrap()
			.octets()
			.to_vec(),
		port: 6000,
		ipv6_scope_id: 7,
	};
	assert_eq!(
		p.candidate_from_proto(&link_local),
		Err(AddressError::MissingScope)
	);

	for bad in [
		Endpoint {
			ip_address: vec![1, 2, 3],
			port: 80,
			ipv6_scope_id: 0,
		},
		Endpoint {
			ip_address: Ipv4Addr::new(10, 0, 0, 1).octets().to_vec(),
			port: 70_000,
			ipv6_scope_id: 0,
		},
		Endpoint {
			ip_address: Ipv4Addr::new(10, 0, 0, 1).octets().to_vec(),
			port: 0,
			ipv6_scope_id: 0,
		},
	] {
		assert!(p.candidate_from_proto(&bad).is_err());
	}
}

#[test]
fn manual_parsing_is_strict_and_normalised() {
	let p = AddressPolicy::lan_only();
	assert!(p.parse_manual("192.168.1.1:+80").is_err());
	assert!(p.parse_manual("[fe80::1%+3]:80").is_err());
	assert!(
		p.parse_manual("[fd00::1%5]:80").is_err(),
		"scope on a non-link-local"
	);
	assert!(p.parse_manual("[::ffff:192.168.1.1%2]:80").is_err());
	// A mapped literal is the IPv4 host and compares equal to it.
	assert_eq!(
		p.parse_manual("[::ffff:192.168.1.1]:80").unwrap(),
		p.parse_manual("192.168.1.1:80").unwrap()
	);
	assert!(p.parse_manual("[fe80::1%3]:80").is_ok());
}

#[test]
fn metadata_addresses_are_not_media_candidates_and_manual_checks_honour_the_flag() {
	let p = AddressPolicy::lan_only();
	for bad in ["169.254.169.254:9", "[fd00:ec2::254]:9"] {
		let addr: std::net::SocketAddr = bad.parse().unwrap();
		assert_eq!(
			p.candidate_from_proto(&endpoint_to_proto(addr)),
			Err(AddressError::Reserved)
		);
	}
	let overlay: std::net::SocketAddr = "100.64.0.9:7".parse().unwrap();
	assert_eq!(
		p.check_manual_socket(overlay),
		Err(AddressError::OverlayManualDisabled)
	);
	assert!(
		p.with_overlay_manual(true)
			.check_manual_socket(overlay)
			.is_ok()
	);
	assert_eq!(
		p.with_overlay_manual(true).check_socket(overlay),
		Err(AddressError::OverlayNetwork)
	);
}
