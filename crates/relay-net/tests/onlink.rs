//! SR-01: inbound sources must be on-link with an eligible interface, not merely private.

use std::net::SocketAddr;

use relay_net::discovery::InterfacePolicy;
use relay_net::onlink::Iface;
use relay_net::{AddressError, AddressPolicy, InboundScope};

fn sock(text: &str) -> SocketAddr {
	text.parse().unwrap()
}

fn iface(name: &str, ip: &str, prefix: u8, index: u32) -> Iface {
	Iface {
		name: name.into(),
		ip: ip.parse().unwrap(),
		prefix,
		index: Some(index),
		loopback: false,
	}
}

/// A laptop with Wi-Fi, Ethernet, a corporate VPN, a Tailscale tunnel and a cellular modem.
fn host() -> Vec<Iface> {
	vec![
		iface("en0", "192.168.1.20", 24, 4),
		iface("en0", "fd12:3456:789a:1::5", 64, 4),
		iface("en0", "fe80::1", 64, 4),
		iface("eth0", "10.1.0.5", 16, 5),
		iface("utun3", "10.8.0.2", 24, 9),
		iface("tailscale0", "100.64.0.5", 10, 10),
		iface("rmnet_data0", "10.77.3.9", 24, 11),
		Iface {
			name: "lo0".into(),
			ip: "127.0.0.1".parse().unwrap(),
			prefix: 8,
			index: Some(1),
			loopback: true,
		},
	]
}

fn tcp(policy: &AddressPolicy, local: &str, peer: &str) -> Result<(), AddressError> {
	InboundScope::default().admit_tcp_in(&host(), policy, sock(local), sock(peer))
}

#[test]
fn on_subnet_sources_are_accepted_on_every_eligible_interface() {
	let p = AddressPolicy::lan_only();
	assert_eq!(tcp(&p, "192.168.1.20:47800", "192.168.1.77:51000"), Ok(()));
	// Multi-NIC: Ethernet works at the same time as Wi-Fi, each with its own subnet.
	assert_eq!(tcp(&p, "10.1.0.5:47800", "10.1.200.9:51000"), Ok(()));
	assert_eq!(
		tcp(
			&p,
			"[fd12:3456:789a:1::5]:47800",
			"[fd12:3456:789a:1::9]:51000"
		),
		Ok(())
	);
}

#[test]
fn private_but_off_subnet_sources_are_refused() {
	let p = AddressPolicy::lan_only();
	for peer in ["192.168.7.9:51000", "10.0.0.3:51000", "172.16.4.4:51000"] {
		assert_eq!(
			tcp(&p, "192.168.1.20:47800", peer),
			Err(AddressError::OffLink),
			"{peer}"
		);
	}
	// A source in the other NIC's subnet does not get in through this NIC's address.
	assert_eq!(
		tcp(&p, "192.168.1.20:47800", "10.1.0.9:51000"),
		Err(AddressError::OffLink)
	);
	assert_eq!(
		tcp(
			&p,
			"[fd12:3456:789a:1::5]:47800",
			"[fd12:3456:789a:2::9]:51000"
		),
		Err(AddressError::OffLink)
	);
}

#[test]
fn vpn_tunnel_and_cellular_interfaces_are_refused_even_on_their_own_subnet() {
	let p = AddressPolicy::lan_only();
	assert_eq!(
		tcp(&p, "10.8.0.2:47800", "10.8.0.9:51000"),
		Err(AddressError::OffLink)
	);
	assert_eq!(
		tcp(&p, "10.77.3.9:47800", "10.77.3.1:51000"),
		Err(AddressError::OffLink)
	);
	// An address that is no interface's at all.
	assert_eq!(
		tcp(&p, "192.168.1.99:47800", "192.168.1.77:51000"),
		Err(AddressError::OffLink)
	);
	// Naming the interface is the user's deliberate choice.
	let chosen = InboundScope::new(InterfacePolicy {
		allowed: vec!["utun3".into()],
		include_loopback: false,
	});
	assert_eq!(
		chosen.admit_tcp_in(&host(), &p, sock("10.8.0.2:47800"), sock("10.8.0.9:51000")),
		Ok(())
	);
	assert_eq!(
		chosen.admit_tcp_in(
			&host(),
			&p,
			sock("192.168.1.20:47800"),
			sock("192.168.1.9:51000")
		),
		Err(AddressError::OffLink)
	);
}

#[test]
fn public_sources_and_link_local_scopes() {
	let p = AddressPolicy::lan_only();
	assert_eq!(
		tcp(&p, "192.168.1.20:47800", "8.8.8.8:51000"),
		Err(AddressError::NotLan)
	);
	assert_eq!(tcp(&p, "[fe80::1%4]:47800", "[fe80::2%4]:51000"), Ok(()));
	assert_eq!(
		tcp(&p, "[fe80::1%4]:47800", "[fe80::2%9]:51000"),
		Err(AddressError::OffLink)
	);
	assert_eq!(
		tcp(&p, "[fe80::1%4]:47800", "[fe80::2]:51000"),
		Err(AddressError::MissingScope)
	);
}

#[test]
fn loopback_is_a_test_path_only() {
	assert_eq!(
		tcp(
			&AddressPolicy::lan_only(),
			"127.0.0.1:47800",
			"127.0.0.1:51000"
		),
		Err(AddressError::Loopback)
	);
	let test = AddressPolicy::lan_and_loopback();
	assert_eq!(tcp(&test, "127.0.0.1:47800", "127.0.0.1:51000"), Ok(()));
	// Even the test policy never lets a loopback source reach a LAN-facing address.
	assert_eq!(
		tcp(&test, "192.168.1.20:47800", "127.0.0.1:51000"),
		Err(AddressError::OffLink)
	);
}

#[test]
fn overlay_stays_governed_by_its_own_opt_in() {
	let off = AddressPolicy::lan_only();
	assert!(tcp(&off, "100.64.0.5:47800", "100.64.0.9:51000").is_err());
	assert!(
		tcp(
			&off.with_overlay_manual(true),
			"100.64.0.5:47800",
			"100.64.0.9:51000"
		)
		.is_err()
	);
	// The one authenticated overlay peer of an opted-in link is admitted as before.
	let peer = off.with_overlay_peer("100.64.0.9".parse().unwrap());
	assert_eq!(tcp(&peer, "100.64.0.5:47800", "100.64.0.9:51000"), Ok(()));
	assert!(tcp(&peer, "100.64.0.5:47800", "100.64.0.10:51000").is_err());
	let udp = InboundScope::default().admit_udp_in(&host(), &peer, sock("100.64.0.9:51000"));
	assert_eq!(udp, Ok(()));
}

#[test]
fn media_sources_follow_the_same_rule() {
	let p = AddressPolicy::lan_only();
	let udp = |peer: &str| InboundScope::default().admit_udp_in(&host(), &p, sock(peer));
	assert_eq!(udp("192.168.1.31:40000"), Ok(()));
	assert_eq!(udp("10.1.3.3:40000"), Ok(()));
	assert_eq!(udp("192.168.9.9:40000"), Err(AddressError::OffLink));
	// On the VPN's own subnet, but that interface is not eligible.
	assert_eq!(udp("10.8.0.9:40000"), Err(AddressError::OffLink));
	assert_eq!(udp("10.77.3.1:40000"), Err(AddressError::OffLink));
}

#[test]
fn discovery_addresses_must_be_on_link_with_the_answering_interface() {
	let scope = InboundScope::default();
	let on = |ip: &str, scope_id: u32, via: &[u32]| {
		scope.on_link_in(&host(), ip.parse().unwrap(), scope_id, via)
	};
	assert!(on("192.168.1.50", 0, &[]));
	assert!(on("192.168.1.50", 0, &[4]));
	// Answer came in on Ethernet but names a Wi-Fi neighbour: spoofed or off-link.
	assert!(!on("192.168.1.50", 0, &[5]));
	assert!(!on("192.168.55.50", 0, &[]));
	assert!(!on("10.8.0.9", 0, &[]));
	assert!(on("fe80::7", 4, &[4]));
	assert!(!on("fe80::7", 9, &[]));
}
