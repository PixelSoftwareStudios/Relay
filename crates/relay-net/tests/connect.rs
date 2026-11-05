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
			"OverlayNetwork",
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
fn shared_address_space_is_deliberately_not_lan() {
	// Tailscale and CGNAT use 100.64.0.0/10. They are refused with their own error.
	let p = AddressPolicy::lan_only();
	for ip in ["100.64.0.1", "100.127.255.254", "100.100.100.100"] {
		assert_eq!(
			p.check_ip(ip.parse().unwrap()),
			Err(relay_net::AddressError::OverlayNetwork),
			"{ip}"
		);
	}
	// The block's edges are ordinary public space.
	for ip in ["100.63.255.255", "100.128.0.1"] {
		assert_eq!(
			p.check_ip(ip.parse().unwrap()),
			Err(relay_net::AddressError::NotLan),
			"{ip}"
		);
	}
	assert!(relay_net::is_shared_address_space(
		"100.64.0.1".parse().unwrap()
	));
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
