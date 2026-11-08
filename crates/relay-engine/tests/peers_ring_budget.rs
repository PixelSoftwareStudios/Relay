//! SR-04: invite budgets for unknown callers, on a fake clock.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use relay_engine::peers::RingBudget;

fn ip(text: &str) -> IpAddr {
	text.parse().unwrap()
}

#[test]
fn one_source_is_capped_whatever_keys_it_uses() {
	let t0 = Instant::now();
	let mut budget = RingBudget::default();
	let attacker = ip("192.168.1.66");
	let admitted = (0..40).filter(|_| budget.admit(attacker, t0)).count();
	assert_eq!(admitted, 6);
	// A different device is untouched.
	assert!(budget.admit(ip("192.168.1.10"), t0));
	// The window rolls over.
	assert!(budget.admit(attacker, t0 + Duration::from_secs(61)));
}

#[test]
fn all_strangers_together_share_a_global_budget() {
	let t0 = Instant::now();
	let mut budget = RingBudget::default();
	let admitted = (0..100u8)
		.filter(|n| budget.admit(ip(&format!("10.0.0.{}", n + 1)), t0))
		.count();
	assert_eq!(admitted, 24);
	assert!(!budget.admit(ip("10.0.1.1"), t0 + Duration::from_secs(59)));
	assert!(budget.admit(ip("10.0.1.1"), t0 + Duration::from_secs(61)));
}

#[test]
fn a_decline_silences_that_source_for_a_short_while_only() {
	let t0 = Instant::now();
	let mut budget = RingBudget::default();
	let caller = ip("192.168.1.66");
	assert!(budget.admit(caller, t0));
	budget.declined(caller, t0 + Duration::from_secs(2));
	assert!(!budget.admit(caller, t0 + Duration::from_secs(10)));
	assert!(budget.admit(caller, t0 + Duration::from_secs(18)));
	// Another device on the LAN is not affected.
	assert!(budget.admit(ip("192.168.1.67"), t0 + Duration::from_secs(3)));
}

#[test]
fn an_ipv6_host_owns_its_whole_slash_64() {
	let t0 = Instant::now();
	let mut budget = RingBudget::default();
	let admitted = (0..20u16)
		.filter(|n| budget.admit(ip(&format!("fd00:1:2:3::{n:x}")), t0))
		.count();
	assert_eq!(admitted, 6);
	assert!(budget.admit(ip("fd00:1:2:4::1"), t0));
}

#[test]
fn a_flood_of_sources_cannot_grow_the_table_without_bound() {
	let t0 = Instant::now();
	let mut budget = RingBudget::default();
	for a in 0..4u8 {
		for b in 0..=255u8 {
			let _ = budget.admit(ip(&format!("10.{a}.{b}.1")), t0);
			budget.declined(ip(&format!("10.{a}.{b}.2")), t0);
		}
	}
	// Still answers, and still forgets once everything has expired.
	assert!(budget.admit(ip("10.9.9.9"), t0 + Duration::from_secs(120)));
}
