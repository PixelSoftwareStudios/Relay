mod common;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use proptest::prelude::*;
use relay_net::AddressPolicy;
use relay_net::connect::{ConnectError, ConnectionString, LocalAddress, encode};
use relay_net::discovery::InterfacePolicy;
use relay_net::handshake::{HandshakeError, initiate, respond};
use relay_net::identity::{Fingerprint, Identity};
use tokio::net::{TcpListener, TcpStream};

fn fp(n: u8) -> Fingerprint {
	Fingerprint::from_bytes([n; 32])
}

fn hex(f: &Fingerprint) -> String {
	f.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn parse(text: &str) -> Result<ConnectionString, ConnectError> {
	ConnectionString::parse(text, &AddressPolicy::lan_only())
}

fn local(interface: &str, ip: &str) -> LocalAddress {
	LocalAddress {
		interface: interface.into(),
		ip: ip.parse().unwrap(),
	}
}

#[test]
fn ipv4_and_ipv6_forms_parse() {
	let f = fp(7);
	let v4 = parse(&format!("relay://192.168.1.20:7000#{}", hex(&f))).unwrap();
	assert_eq!(v4.addr, "192.168.1.20:7000".parse().unwrap());
	assert_eq!(v4.expected, f);
	let v6 = parse(&format!("relay://[fd12:3456::1]:7000#{}", hex(&f))).unwrap();
	assert_eq!(v6.addr, "[fd12:3456::1]:7000".parse().unwrap());
	// Scheme case and whitespace are forgiven; fingerprint hex case too.
	assert!(
		parse(&format!(
			"  RELAY://10.0.0.5:1#{}\n",
			hex(&f).to_uppercase()
		))
		.is_ok()
	);
}

#[test]
fn display_is_the_canonical_form() {
	let c = ConnectionString::new("[fd00::9]:4000".parse().unwrap(), fp(1));
	assert_eq!(
		c.to_string(),
		format!("relay://[fd00::9]:4000#{}", hex(&fp(1)))
	);
	assert_eq!(parse(&c.to_string()).unwrap(), c);
}

#[test]
fn the_fingerprint_is_mandatory_and_exact() {
	assert_eq!(
		parse("relay://192.168.1.20:7000"),
		Err(ConnectError::Fingerprint)
	);
	assert_eq!(
		parse("relay://192.168.1.20:7000#"),
		Err(ConnectError::Fingerprint)
	);
	let short = &hex(&fp(1))[..62];
	assert_eq!(
		parse(&format!("relay://192.168.1.20:7000#{short}")),
		Err(ConnectError::Fingerprint)
	);
	let spaced = format!("relay://192.168.1.20:7000#{}", Fingerprint::full(&fp(1)));
	assert_eq!(parse(&spaced), Err(ConnectError::Fingerprint));
	let bad = format!("relay://192.168.1.20:7000#{}zz", &hex(&fp(1))[..62]);
	assert_eq!(parse(&bad), Err(ConnectError::Fingerprint));
}

#[test]
fn non_lan_hosts_names_and_extras_are_refused() {
	let h = hex(&fp(1));
	for (text, expected) in [
		(format!("relay://8.8.8.8:7000#{h}"), "NotLan"),
		(format!("relay://127.0.0.1:7000#{h}"), "Loopback"),
		(
			format!("relay://100.101.102.103:7000#{h}"),
			"OverlayManualDisabled",
		),
		(format!("relay://192.168.1.20:0#{h}"), "ZeroPort"),
		(format!("relay://[fe80::1]:7000#{h}"), "MissingScope"),
		(format!("relay://printer.local:7000#{h}"), "Malformed"),
		(format!("relay://user:pw@192.168.1.20:7000#{h}"), "Extra"),
		(format!("relay://192.168.1.20:7000/x#{h}"), "Extra"),
		(format!("relay://192.168.1.20:7000?x=1#{h}"), "Extra"),
		(format!("http://192.168.1.20:7000#{h}"), "Scheme"),
		(format!("192.168.1.20:7000#{h}"), "Scheme"),
		(format!("relay://[::ffff:8.8.8.8]:7000#{h}"), "NotLan"),
	] {
		let err = parse(&text).unwrap_err();
		assert!(format!("{err:?}").contains(expected), "{text}: {err:?}");
	}
	assert!(parse(&format!("relay://192.168.1.20:7000#{}", "a".repeat(300))).is_err());
}

#[test]
fn scoped_link_local_is_accepted_when_typed_by_hand() {
	let c = parse(&format!("relay://[fe80::1%3]:7000#{}", hex(&fp(1)))).unwrap();
	assert!(c.to_string().contains("%3"));
}

#[test]
fn overlay_space_is_manual_only_and_opt_in() {
	use relay_net::AddressError;
	let off = AddressPolicy::lan_only();
	let on = AddressPolicy::lan_only().with_overlay_manual(true);
	let h = hex(&fp(1));
	// Typed addresses: refused with an actionable error unless the flag is on.
	for ip in ["100.64.0.1", "100.127.255.254", "100.100.100.100"] {
		assert_eq!(
			off.parse_manual(&format!("{ip}:7000")),
			Err(AddressError::OverlayManualDisabled),
			"{ip}"
		);
		assert!(on.parse_manual(&format!("{ip}:7000")).is_ok(), "{ip}");
		let text = format!("relay://{ip}:7000#{h}");
		assert!(ConnectionString::parse(&text, &off).is_err());
		assert_eq!(
			ConnectionString::parse(&text, &on)
				.unwrap()
				.addr
				.ip()
				.to_string(),
			ip
		);
	}
	assert_eq!(on.parse_manual("100.64.0.1:0"), Err(AddressError::ZeroPort));
	assert!(on.parse_manual("[::ffff:100.64.0.1]:7000").is_ok());
	// The flag does not widen anything else, and the block's neighbours stay refused.
	for ip in ["100.63.255.255", "100.128.0.1", "8.8.8.8"] {
		assert_eq!(
			on.parse_manual(&format!("{ip}:7000")),
			Err(AddressError::NotLan),
			"{ip}"
		);
	}
	// Network-sourced checks never honour the flag.
	for ip in ["100.64.0.1", "100.100.100.100"] {
		let addr: SocketAddr = format!("{ip}:7000").parse().unwrap();
		assert_eq!(on.check_ip(addr.ip()), Err(AddressError::OverlayNetwork));
		assert_eq!(on.check_socket(addr), Err(AddressError::OverlayNetwork));
		let endpoint = relay_net::endpoint_to_proto(addr);
		assert_eq!(
			on.candidate_from_proto(&endpoint),
			Err(AddressError::OverlayNetwork)
		);
	}
	assert!(relay_net::is_shared_address_space(
		"100.64.0.1".parse().unwrap()
	));
	assert!(!relay_net::is_shared_address_space(
		"100.63.255.255".parse().unwrap()
	));
	assert!(!relay_net::is_shared_address_space(
		"100.128.0.1".parse().unwrap()
	));
}

#[test]
fn encode_and_discovery_never_include_overlay_even_with_the_flag() {
	let on = AddressPolicy::lan_only().with_overlay_manual(true);
	let out = encode(
		7000,
		&fp(5),
		&[local("en5", "100.101.1.1"), local("en0", "192.168.1.20")],
		&on,
		&InterfacePolicy::default(),
	);
	assert_eq!(out.len(), 1);
	assert_eq!(out[0].addr.ip().to_string(), "192.168.1.20");
}

#[test]
fn encode_prefers_physical_lan_and_excludes_tunnels_and_public() {
	let addrs = [
		local("utun3", "192.168.77.2"),
		local("tailscale0", "100.101.1.1"),
		local("en0", "169.254.8.8"),
		local("en0", "fe80::1"),
		local("en0", "fd12:3456::1"),
		local("docker0", "172.17.0.1"),
		local("lo0", "127.0.0.1"),
		local("en1", "2001:db8::5"),
		local("en0", "192.168.1.20"),
		local("wlan0", "10.0.0.7"),
	];
	let out = encode(
		7000,
		&fp(5),
		&addrs,
		&AddressPolicy::lan_only(),
		&InterfacePolicy::default(),
	);
	let got: Vec<String> = out.iter().map(|c| c.addr.to_string()).collect();
	assert_eq!(
		got,
		[
			"192.168.1.20:7000",
			"10.0.0.7:7000",
			"[fd12:3456::1]:7000",
			"169.254.8.8:7000"
		]
	);
	assert!(out.iter().all(|c| c.expected == fp(5)));
	// Everything it emits parses back under the same policy.
	for c in &out {
		assert_eq!(&parse(&c.to_string()).unwrap(), c);
	}
}

#[test]
fn encode_honours_an_explicit_interface_choice() {
	let interfaces = InterfacePolicy {
		allowed: vec!["utun3".into()],
		include_loopback: false,
	};
	let out = encode(
		7000,
		&fp(5),
		&[local("utun3", "192.168.77.2"), local("en0", "192.168.1.20")],
		&AddressPolicy::lan_only(),
		&interfaces,
	);
	assert_eq!(out.len(), 1);
	assert_eq!(out[0].addr.ip().to_string(), "192.168.77.2");
}

#[tokio::test]
async fn the_expected_fingerprint_pins_the_handshake() {
	let server = Identity::generate().unwrap();
	let client = Identity::generate().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let right = ConnectionString::new(addr, server.fingerprint());
	let wrong = ConnectionString::new(addr, fp(9));

	for (target, ok) in [(&wrong, false), (&right, true)] {
		let serve = async {
			let (mut s, _) = listener.accept().await.unwrap();
			let _ = respond(&mut s, &server).await;
		};
		let dial = async {
			let mut s = TcpStream::connect(target.addr).await.unwrap();
			initiate(&mut s, &client, Some(&target.expected)).await
		};
		let (_, result) = tokio::join!(serve, dial);
		match (ok, result) {
			(true, Ok(session)) => assert_eq!(session.remote_fingerprint(), target.expected),
			(false, Err(HandshakeError::UnexpectedPeer)) => {}
			(_, other) => panic!("unexpected outcome: {other:?}"),
		}
	}
}

fn lan_ip() -> impl Strategy<Value = IpAddr> {
	prop_oneof![
		(10u8..=10, any::<u8>(), any::<u8>(), any::<u8>())
			.prop_map(|(a, b, c, d)| IpAddr::V4(Ipv4Addr::new(a, b, c, d))),
		(any::<u8>(), any::<u8>()).prop_map(|(c, d)| IpAddr::V4(Ipv4Addr::new(192, 168, c, d))),
		(16u8..=31, any::<u8>(), any::<u8>())
			.prop_map(|(b, c, d)| IpAddr::V4(Ipv4Addr::new(172, b, c, d))),
		(any::<[u16; 7]>(), 0xfc00u16..=0xfdff).prop_map(|(s, first)| {
			let mut seg = [0u16; 8];
			seg[0] = first;
			seg[1..].copy_from_slice(&s);
			IpAddr::V6(Ipv6Addr::from(seg))
		}),
	]
}

proptest! {
	#[test]
	fn roundtrip(ip in lan_ip(), port in 1u16.., bytes in any::<[u8; 32]>()) {
		let c = ConnectionString::new(SocketAddr::new(ip, port), Fingerprint::from_bytes(bytes));
		prop_assert_eq!(parse(&c.to_string()).unwrap(), c);
	}

	#[test]
	fn no_network_path_yields_overlay_space(a in any::<u8>(), b in 64u8..=127, c in any::<u8>(), d in any::<u8>(), port in 1u16..) {
		let ip = IpAddr::V4(Ipv4Addr::new(100, b, c, d));
		let _ = a;
		let on = AddressPolicy::lan_only().with_overlay_manual(true);
		let addr = SocketAddr::new(ip, port);
		prop_assert!(on.check_ip(ip).is_err());
		prop_assert!(on.check_socket(addr).is_err());
		prop_assert!(on.candidate_from_proto(&relay_net::endpoint_to_proto(addr)).is_err());
		let mapped = IpAddr::V6(Ipv4Addr::new(100, b, c, d).to_ipv6_mapped());
		prop_assert!(on.check_ip(mapped).is_err());
		let out = encode(port, &fp(1), &[LocalAddress { interface: "en0".into(), ip }], &on, &InterfacePolicy::default());
		prop_assert!(out.is_empty());
		// Only the user-typed path may accept it, and only with the flag.
		prop_assert!(AddressPolicy::lan_only().parse_manual(&addr.to_string()).is_err());
		prop_assert!(on.parse_manual(&addr.to_string()).is_ok());
	}

	#[test]
	fn arbitrary_text_never_panics_and_accepted_text_is_lan(text in ".{0,300}") {
		if let Ok(c) = parse(&text) {
			prop_assert!(AddressPolicy::lan_only().check_socket(c.addr).is_ok());
		}
	}

	#[test]
	fn mutated_strings_that_still_parse_stay_lan_only(
		ip in lan_ip(), port in 1u16.., bytes in any::<[u8; 32]>(), at in 0usize..200, byte in any::<u8>()
	) {
		let c = ConnectionString::new(SocketAddr::new(ip, port), Fingerprint::from_bytes(bytes));
		let mut raw = c.to_string().into_bytes();
		let i = at % raw.len();
		raw[i] = byte;
		if let Ok(text) = String::from_utf8(raw)
			&& let Ok(parsed) = parse(&text)
		{
			// Whatever is accepted is itself canonical and LAN-only.
			prop_assert!(AddressPolicy::lan_only().check_socket(parsed.addr).is_ok());
		}
	}
}
