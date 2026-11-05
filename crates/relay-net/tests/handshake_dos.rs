//! Pre-authentication resource limits: per-source slots, eviction, preface deadline, accept
//! backoff and the cap on authenticated-but-untrusted links.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use relay_net::handshake::{
	AcceptBackoff, HandshakeError, HandshakeGate, UntrustedLinks, respond, respond_gated,
};
use relay_net::identity::Identity;

fn ip(text: &str) -> IpAddr {
	text.parse().unwrap()
}

#[test]
fn one_host_cannot_hold_more_than_its_share() {
	let gate = HandshakeGate::new();
	let attacker = ip("192.168.1.66");
	let held: Vec<_> = (0..2).map(|_| gate.try_enter(attacker).unwrap()).collect();
	for _ in 0..50 {
		assert!(matches!(
			gate.try_enter(attacker),
			Err(HandshakeError::Overloaded)
		));
	}
	// Honest sources are untouched.
	assert!(gate.try_enter(ip("192.168.1.10")).is_ok());
	drop(held);
}

#[test]
fn ipv6_hosts_are_grouped_by_slash_64_and_mapped_addresses_unwrap() {
	let gate = HandshakeGate::new();
	let _a = gate.try_enter(ip("fd00:1:2:3::1")).unwrap();
	let _b = gate.try_enter(ip("fd00:1:2:3:ffff::9")).unwrap();
	assert!(gate.try_enter(ip("fd00:1:2:3::77")).is_err());
	assert!(gate.try_enter(ip("fd00:1:2:4::1")).is_ok());
	let _c = gate.try_enter(ip("192.168.1.5")).unwrap();
	let _d = gate.try_enter(ip("::ffff:192.168.1.5")).unwrap();
	assert!(gate.try_enter(ip("192.168.1.5")).is_err());
}

#[test]
fn several_attacker_addresses_cannot_starve_a_newcomer() {
	let gate = HandshakeGate::new();
	// Four attacker hosts fill all eight slots with silent connections.
	let attackers: Vec<_> = (0..4)
		.flat_map(|h| {
			let addr = ip(&format!("10.0.0.{}", 100 + h));
			[gate.try_enter(addr).unwrap(), gate.try_enter(addr).unwrap()]
		})
		.collect();
	assert_eq!(gate.in_use(), 8);
	// A legitimate peer still gets in, and exactly one attacker slot was evicted.
	let friend = gate.try_enter(ip("10.0.0.5")).unwrap();
	assert!(!friend.is_evicted());
	assert_eq!(attackers.iter().filter(|p| p.is_evicted()).count(), 1);
	assert_eq!(gate.in_use(), 8);
}

#[tokio::test]
async fn an_evicted_handshake_gives_up() {
	let gate = HandshakeGate::with_limits(1, 1);
	let identity = Identity::generate().unwrap();
	let (mut quiet, _peer) = tokio::io::duplex(64);
	let permit = gate.try_enter(ip("192.168.1.2")).unwrap();
	let task = async { respond_gated(&mut quiet, &identity, &permit).await };
	let evict = async {
		tokio::time::sleep(Duration::from_millis(50)).await;
		gate.try_enter(ip("192.168.1.3")).unwrap()
	};
	let (result, _newcomer) = tokio::join!(task, evict);
	assert!(matches!(result, Err(HandshakeError::Overloaded)));
}

#[tokio::test]
async fn a_silent_connection_is_dropped_after_the_short_preface_deadline() {
	let identity = Identity::generate().unwrap();
	let (mut quiet, _peer) = tokio::io::duplex(64);
	let start = Instant::now();
	let result = respond(&mut quiet, &identity).await;
	assert!(matches!(result, Err(HandshakeError::Timeout)));
	assert!(
		start.elapsed() < Duration::from_millis(2500),
		"{:?}",
		start.elapsed()
	);
}

#[test]
fn accept_errors_back_off_and_reset() {
	let mut backoff = AcceptBackoff::new();
	let delays: Vec<_> = (0..10).map(|_| backoff.on_error()).collect();
	assert_eq!(delays[0], Duration::from_millis(20));
	assert_eq!(delays[1], Duration::from_millis(40));
	assert!(delays.windows(2).all(|w| w[1] >= w[0]));
	assert_eq!(*delays.last().unwrap(), Duration::from_secs(2));
	backoff.reset();
	assert_eq!(backoff.on_error(), Duration::from_millis(20));
}

#[test]
fn untrusted_links_are_bounded_per_source_and_overall() {
	let mut links: UntrustedLinks<u32> = UntrustedLinks::with_limits(4, 2);
	let attacker = ip("10.0.0.9");
	assert!(links.admit(1, attacker).is_empty());
	assert!(links.admit(2, attacker).is_empty());
	// The third link from one source evicts that source's own oldest.
	assert_eq!(links.admit(3, attacker), vec![1]);
	assert!(links.admit(10, ip("10.0.0.1")).is_empty());
	assert!(links.admit(11, ip("10.0.0.2")).is_empty());
	assert_eq!(links.len(), 4);
	// Full: the oldest overall goes, the newcomer is kept.
	assert_eq!(links.admit(12, ip("10.0.0.3")), vec![2]);
	assert_eq!(links.len(), 4);
	links.release(&12);
	assert_eq!(links.len(), 3);
	// A flood of identities from one address never exceeds its share.
	for k in 100..200 {
		links.admit(k, attacker);
	}
	assert!(links.len() <= 4);
}
