use std::net::IpAddr;

use relay_engine::peers::Subnet;

fn ip(text: &str) -> IpAddr {
	text.parse().unwrap()
}

#[test]
fn contains_is_family_and_prefix_aware() {
	let net = Subnet::new(ip("192.168.1.20"), 24);
	assert!(net.contains(ip("192.168.1.255")));
	assert!(!net.contains(ip("192.168.2.1")));
	assert!(!net.contains(ip("::ffff:192.168.1.1")));

	let odd = Subnet::new(ip("10.0.0.0"), 9);
	assert!(odd.contains(ip("10.127.255.255")));
	assert!(!odd.contains(ip("10.128.0.0")));

	let v6 = Subnet::new(ip("fd00::1"), 64);
	assert!(v6.contains(ip("fd00::ffff")));
	assert!(!v6.contains(ip("fd00:0:0:1::1")));
}

#[test]
fn oversized_prefix_is_clamped_to_a_host_range() {
	let host = Subnet::new(ip("10.0.0.7"), 200);
	assert!(host.contains(ip("10.0.0.7")));
	assert!(!host.contains(ip("10.0.0.8")));
	assert!(Subnet::new(ip("10.0.0.7"), 0).contains(ip("8.8.8.8")));
}
